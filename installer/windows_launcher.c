/* Windows console entry point. Uses only the private bundled Python runtime. */
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <stdio.h>
#include <stdlib.h>
#include <wchar.h>

/* Quote one argument using the Windows CRT backslash/quote rules. */
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
    wchar_t root[32768], python[32768], script[32768];
    DWORD length = GetModuleFileNameW(NULL, root, 32768);
    if (!length || length >= 32768) return 1;
    wchar_t *last = wcsrchr(root, L'\\');
    if (!last) return 1;
    *last = 0;
    if (wcslen(root) + 64 >= 32768) return 1;
    swprintf(python, 32768, L"%ls\\windows-python\\python.exe", root);
    swprintf(script, 32768, L"%ls\\echo_setup.py", root);
    if (GetFileAttributesW(python) == INVALID_FILE_ATTRIBUTES ||
        GetFileAttributesW(script) == INVALID_FILE_ATTRIBUTES) {
        fwprintf(stderr, L"Installer files are missing. Extract the complete ZIP before running setup.exe.\n");
        if (argc == 1) { fputws(L"Press Enter to close.\n", stderr); getchar(); }
        return 1;
    }
    size_t size = 2 * (wcslen(python) + wcslen(script)) + 64;
    for (int i = 1; i < argc; ++i) size += 2 * wcslen(argv[i]) + 4;
    wchar_t *command = calloc(size, sizeof(wchar_t));
    if (!command) return 1;
    wchar_t *end = quote(command, python);
    wcscpy(end, L" -X utf8 "); end += wcslen(end);
    end = quote(end, script);
    for (int i = 1; i < argc; ++i) { *end++ = L' '; end = quote(end, argv[i]); }
    *end = 0;
    STARTUPINFOW start = { .cb = sizeof(start) };
    PROCESS_INFORMATION process = {0};
    /* No shell, PATH lookup, Python installation, or registry changes. */
    BOOL ok = CreateProcessW(python, command, NULL, NULL, TRUE, 0, NULL, NULL, &start, &process);
    free(command);
    if (!ok) {
        fwprintf(stderr, L"Could not start the bundled runtime (Windows error %lu).\n", GetLastError());
        return 1;
    }
    CloseHandle(process.hThread);
    WaitForSingleObject(process.hProcess, INFINITE);
    DWORD result = 1;
    GetExitCodeProcess(process.hProcess, &result);
    CloseHandle(process.hProcess);
    return (int)result;
}
