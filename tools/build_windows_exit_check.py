"""Build the separate Windows exit tracer ZIP; never modifies release payloads."""
from pathlib import Path
import hashlib
import os
import subprocess
import zipfile

ROOT = Path(__file__).resolve().parents[1]
toolchain = Path(os.environ.get('LLVM_MINGW_DIR', ROOT/'working/llvm-mingw-20260922-ucrt-ubuntu-22.04-x86_64'))
build = ROOT/'working/windows-exit-trace-build'
build.mkdir(parents=True, exist_ok=True)
exe = build/'exit-trace.exe'
subprocess.run([
    str(toolchain/'bin/x86_64-w64-mingw32-clang'), '-std=c11', '-Wall', '-Wextra', '-Werror',
    '-O2', '-municode', '-static', '-Wl,--no-insert-timestamp',
    str(ROOT/'tools/windows_exit_trace.c'), '-ldbghelp', '-lpsapi', '-o', str(exe),
], check=True)
readme = '''Lone Echo II Windows exit trace

1. Extract these files beside your installed setup.exe and windows-python.
   Replace the earlier diagnose_windows_exit.py if asked.
2. Connect your headset, then double-click Trace-LE2-Exit.cmd.
3. Quit from the game menu, dismiss any error popup, and wait for the helper.
4. Share exit-report.txt from the folder printed in the window:
   userdata\\le2\\logs\\exit-check-<date and time>\\exit-report.txt

This launches the existing setup program with --diagnostics under a temporary
external debugger. It records LE2 fault addresses and stacks before the game's
crash reporter handles them. First-chance faults may be handled by the game;
their presence alone does not prove they caused the exit failure.

No game DLLs, runtime settings, registry settings, or crash handlers are changed.
No uploads or symbol-server downloads. It uses the bundled Python and Windows
debug APIs. The debugger is active only for this launch; normal shortcuts are
unchanged. A debugger may affect timing or the game's error-reporting behavior.

Reports contain local file paths, register values, and diagnostic logs. Review
before sharing. Remove the three helper files and EXIT-TRACE-README.txt after
investigation. Reports may be deleted from their exit-check folder as well.
'''
archive = ROOT/'dist/lone-echo-windows-exit-trace.zip'
archive.parent.mkdir(exist_ok=True)
with zipfile.ZipFile(archive, 'w', zipfile.ZIP_DEFLATED) as output:
    output.write(exe, 'exit-trace.exe')
    for name in ('Trace-LE2-Exit.cmd', 'diagnose_windows_exit.py'):
        output.writestr(name, (ROOT/'tools'/name).read_text().replace('\n', '\r\n'))
    output.writestr('EXIT-TRACE-README.txt', readme)
with zipfile.ZipFile(archive) as output:
    assert output.testzip() is None
print(f'{archive}\nSHA256: {hashlib.sha256(archive.read_bytes()).hexdigest()}')
