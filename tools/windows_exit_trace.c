/* Temporary, external LE2 debugger. No injection, patches, dumps, or registry
 * changes. Observe faults before BugSplat, then pass them to the game unchanged.
 * Build for x64 Windows; run: exit-trace.exe report.txt setup.exe launch le2.
 */
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <dbghelp.h>
#include <psapi.h>
#include <stdio.h>
#include <stdlib.h>
#include <wchar.h>

typedef struct {
    DWORD id;
    HANDLE handle; /* Owned by the debug event API until EXIT_PROCESS. */
    BOOL game, initial_breakpoint;
    unsigned faults;
} Process;
static Process processes[64];
static FILE *report;

static void print_wide(const wchar_t *value) {
    char utf8[32768];
    if (WideCharToMultiByte(CP_UTF8, 0, value, -1, utf8, sizeof(utf8), NULL, NULL))
        fputs(utf8, report);
}

static void address(HANDLE process, DWORD64 pc) {
    MEMORY_BASIC_INFORMATION memory;
    wchar_t path[8192];
    fprintf(report, "0x%016llx", (unsigned long long)pc);
    if (VirtualQueryEx(process, (void *)(uintptr_t)pc, &memory, sizeof(memory)) &&
        memory.State != MEM_FREE) {
        fprintf(report, " base=0x%llx offset=0x%llx protect=0x%lx ",
                (unsigned long long)(uintptr_t)memory.AllocationBase,
                (unsigned long long)(pc - (uintptr_t)memory.AllocationBase), memory.Protect);
        DWORD count = GetMappedFileNameW(process, (void *)(uintptr_t)pc, path, 8192);
        if (count && count < 8192) { path[count] = 0; print_wide(path); }
    } else fputs(" (unmapped)", report);
    fputc('\n', report);
}

static void fault(Process *process, const DEBUG_EVENT *event) {
    const EXCEPTION_DEBUG_INFO *info = &event->u.Exception;
    const EXCEPTION_RECORD *exception = &info->ExceptionRecord;
    fprintf(report, "\nException pid=%lu tid=%lu code=0x%08lx %s chance tick=%llu\n",
            event->dwProcessId, event->dwThreadId, exception->ExceptionCode,
            info->dwFirstChance ? "first" : "second", (unsigned long long)GetTickCount64());
    address(process->handle, (DWORD64)(uintptr_t)exception->ExceptionAddress);
    for (DWORD i = 0; i < exception->NumberParameters && i < EXCEPTION_MAXIMUM_PARAMETERS; ++i)
        fprintf(report, "  parameter[%lu]=0x%llx\n", i,
                (unsigned long long)exception->ExceptionInformation[i]);
    HANDLE thread = OpenThread(THREAD_GET_CONTEXT | THREAD_QUERY_INFORMATION, FALSE, event->dwThreadId);
    CONTEXT context = { .ContextFlags = CONTEXT_FULL };
    if (!thread || !GetThreadContext(thread, &context)) {
        fprintf(report, "GetThreadContext failed: %lu\n", GetLastError());
        if (thread) CloseHandle(thread);
        fflush(report);
        return;
    }
    fprintf(report, "RIP=%llx RSP=%llx RBP=%llx RAX=%llx RBX=%llx RCX=%llx RDX=%llx\n"
            "RSI=%llx RDI=%llx R8=%llx R9=%llx R10=%llx R11=%llx R12=%llx R13=%llx R14=%llx R15=%llx\n",
            context.Rip, context.Rsp, context.Rbp, context.Rax, context.Rbx, context.Rcx, context.Rdx,
            context.Rsi, context.Rdi, context.R8, context.R9, context.R10, context.R11,
            context.R12, context.R13, context.R14, context.R15);
    /* Use only local unwind data, with an explicit local symbol search path.
     * This handler runs on one thread; DbgHelp is not thread safe. */
    SymSetOptions(SYMOPT_DEFERRED_LOADS | SYMOPT_FAIL_CRITICAL_ERRORS |
                  SYMOPT_NO_PROMPTS | SYMOPT_IGNORE_NT_SYMPATH);
    if (SymInitialize(process->handle, ".", TRUE)) {
        STACKFRAME64 frame = {0};
        frame.AddrPC.Offset = context.Rip;
        frame.AddrStack.Offset = context.Rsp;
        frame.AddrFrame.Offset = context.Rbp;
        frame.AddrPC.Mode = frame.AddrStack.Mode = frame.AddrFrame.Mode = AddrModeFlat;
        fputs("Stack (local unwind data, module offsets):\n", report);
        DWORD64 previous_pc = 0, previous_sp = 0;
        for (unsigned i = 0; i < 40; ++i) {
            if (!StackWalk64(IMAGE_FILE_MACHINE_AMD64, process->handle, thread, &frame,
                             &context, NULL, SymFunctionTableAccess64, SymGetModuleBase64, NULL) ||
                !frame.AddrPC.Offset ||
                (frame.AddrPC.Offset == previous_pc && frame.AddrStack.Offset == previous_sp)) break;
            fprintf(report, "  #%u ", i);
            address(process->handle, frame.AddrPC.Offset);
            previous_pc = frame.AddrPC.Offset;
            previous_sp = frame.AddrStack.Offset;
        }
        SymCleanup(process->handle);
    } else fprintf(report, "SymInitialize failed: %lu\n", GetLastError());
    CloseHandle(thread);
    fflush(report);
}

/* Windows CRT quoting, including trailing backslashes. */
static wchar_t *quote(wchar_t *out, const wchar_t *arg) {
    *out++ = L'"';
    while (*arg) {
        size_t slashes = 0;
        while (*arg == L'\\') { ++slashes; ++arg; }
        size_t count = (*arg == L'"' || !*arg) ? 2 * slashes : slashes;
        while (count--) *out++ = L'\\';
        if (!*arg) break;
        if (*arg == L'"') *out++ = L'\\';
        *out++ = *arg++;
    }
    *out++ = L'"';
    return out;
}

int wmain(int argc, wchar_t **argv) {
    if (argc < 3) { fputs("Usage: exit-trace.exe report.txt program.exe [arguments]\n", stderr); return 2; }
    report = _wfopen(argv[1], L"wb");
    if (!report) { fputs("Cannot create exception report.\n", stderr); return 2; }
    fputs("LE2 external exit trace v1; first-chance exceptions may be handled by the game.\n", report);
    size_t size = 1;
    for (int i = 2; i < argc; ++i) size += 2 * wcslen(argv[i]) + 4;
    if (size > 32767) { fclose(report); return 2; }
    wchar_t *command = calloc(size, sizeof(wchar_t));
    if (!command) { fclose(report); return 2; }
    wchar_t *end = command;
    for (int i = 2; i < argc; ++i) {
        if (i > 2) *end++ = L' ';
        end = quote(end, argv[i]);
    }
    *end = 0;
    STARTUPINFOW start = { .cb = sizeof(start) };
    PROCESS_INFORMATION launched = {0};
    BOOL ok = CreateProcessW(argv[2], command, NULL, NULL, TRUE, DEBUG_PROCESS,
                             NULL, NULL, &start, &launched);
    free(command);
    if (!ok) {
        fprintf(report, "CreateProcess failed: %lu\n", GetLastError()); fclose(report); return 2;
    }
    CloseHandle(launched.hThread);
    CloseHandle(launched.hProcess);
    if (!DebugSetProcessKillOnExit(FALSE)) {
        fprintf(report, "DebugSetProcessKillOnExit failed: %lu\n", GetLastError());
        DebugActiveProcessStop(launched.dwProcessId);
        fclose(report); return 2;
    }
    DWORD result = 2;
    unsigned active = 0;
    BOOL done = FALSE;
    while (!done) {
        DEBUG_EVENT event;
        if (!WaitForDebugEvent(&event, INFINITE)) {
            fprintf(report, "WaitForDebugEvent failed: %lu\n", GetLastError()); break;
        }
        Process *process = NULL;
        for (unsigned i = 0; i < 64; ++i)
            if (processes[i].id == event.dwProcessId) { process = &processes[i]; break; }
        DWORD status = DBG_CONTINUE;
        switch (event.dwDebugEventCode) {
        case CREATE_PROCESS_DEBUG_EVENT: {
            for (unsigned i = 0; i < 64; ++i)
                if (!processes[i].id) { process = &processes[i]; break; }
            if (!process) {
                fputs("Too many child processes; stopping trace.\n", report);
                DebugActiveProcessStop(event.dwProcessId);
                done = TRUE;
                break;
            }
            wchar_t path[8192] = L"";
            DWORD count = 8192;
            QueryFullProcessImageNameW(event.u.CreateProcessInfo.hProcess, 0, path, &count);
            const wchar_t *base = wcsrchr(path, L'\\');
            *process = (Process){event.dwProcessId, event.u.CreateProcessInfo.hProcess,
                                _wcsicmp(base ? base + 1 : path, L"loneecho2.exe") == 0, FALSE, 0};
            ++active;
            fprintf(report, "Process %lu ", process->id); print_wide(path); fputc('\n', report);
            if (event.u.CreateProcessInfo.hFile) CloseHandle(event.u.CreateProcessInfo.hFile);
            break;
        }
        case LOAD_DLL_DEBUG_EVENT:
            if (process && process->game) {
                fputs("Load ", report);
                address(process->handle, (DWORD64)(uintptr_t)event.u.LoadDll.lpBaseOfDll);
            }
            if (event.u.LoadDll.hFile) CloseHandle(event.u.LoadDll.hFile);
            break;
        case UNLOAD_DLL_DEBUG_EVENT:
            if (process && process->game)
                fprintf(report, "Unload base=0x%llx\n", (unsigned long long)(uintptr_t)event.u.UnloadDll.lpBaseOfDll);
            break;
        case EXCEPTION_DEBUG_EVENT: {
            DWORD code = event.u.Exception.ExceptionRecord.ExceptionCode;
            status = DBG_EXCEPTION_NOT_HANDLED;
            /* Consume only the initial debugger breakpoint. All game faults
             * continue through the game's own exception handlers normally. */
            if (process && !process->initial_breakpoint && code == EXCEPTION_BREAKPOINT &&
                event.u.Exception.dwFirstChance) {
                process->initial_breakpoint = TRUE;
                status = DBG_CONTINUE;
            } else if (process && process->game &&
                       (code == EXCEPTION_ACCESS_VIOLATION || code == EXCEPTION_IN_PAGE_ERROR ||
                        code == EXCEPTION_ILLEGAL_INSTRUCTION || code == EXCEPTION_STACK_OVERFLOW ||
                        code == 0xc0000374 || code == 0xc0000409 || !event.u.Exception.dwFirstChance)) {
                /* Bound stack collection if a game deliberately probes memory. */
                if (process->faults++ < 64 || !event.u.Exception.dwFirstChance) fault(process, &event);
                else if (process->faults == 65) fputs("First-chance stack limit reached.\n", report);
            }
            break;
        }
        case EXIT_PROCESS_DEBUG_EVENT:
            fprintf(report, "Exit pid=%lu code=0x%08lx\n", event.dwProcessId, event.u.ExitProcess.dwExitCode);
            if (event.dwProcessId == launched.dwProcessId) result = event.u.ExitProcess.dwExitCode;
            if (process) { process->id = 0; --active; }
            if (!active) done = TRUE;
            break;
        }
        fflush(report);
        if (!ContinueDebugEvent(event.dwProcessId, event.dwThreadId, status)) {
            fprintf(report, "ContinueDebugEvent failed: %lu\n", GetLastError()); break;
        }
    }
    /* Also detach on an API error. Exiting this helper must not kill the game. */
    for (unsigned i = 0; i < 64; ++i)
        if (processes[i].id) DebugActiveProcessStop(processes[i].id);
    fclose(report);
    return (int)result;
}
