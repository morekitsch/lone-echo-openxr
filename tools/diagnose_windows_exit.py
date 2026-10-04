"""Collect a local LE2 exit report through the installed Windows launcher.

Run with the package's private Python. Does not change DLLs, registry settings,
runtime selection, or crash handlers. Nothing is uploaded.
"""
import argparse
import datetime
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile


def tail(path, limit=16000):
    try:
        with path.open('rb') as source:
            source.seek(max(0, path.stat().st_size-limit))
            return source.read().decode('utf-8', errors='replace')
    except OSError as error:
        return f'Cannot read {path}: {error}'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--package', type=Path, default=Path(__file__).resolve().parent)
    parser.add_argument('--trace', action='store_true', help='Observe faults with the external exit-trace.exe debugger')
    args = parser.parse_args()
    if os.name != 'nt':
        parser.error('Run this helper on Windows beside setup.exe.')
    package = args.package.resolve()
    launcher = package/'setup.exe'
    state_path = package/'state/le2/install.json'
    if not launcher.is_file() or not state_path.is_file():
        parser.error('Select the package folder with setup.exe and an installed LE2.')
    tracer = package/'exit-trace.exe'
    if args.trace and not tracer.is_file():
        parser.error('Extract exit-trace.exe beside setup.exe before using --trace.')
    state = json.loads(state_path.read_text(encoding='utf-8'))
    game_bin = Path(state['game_dir'])/'bin/win10'
    stamp = datetime.datetime.now().strftime('%Y%m%d-%H%M%S-%f')
    output = package/'userdata/le2/logs'/('exit-check-'+stamp)
    output.mkdir(parents=True)
    report = output/'exit-report.txt'

    def record(title, value):
        with report.open('a', encoding='utf-8') as stream:
            stream.write(f'\n## {title}\n{value}\n')

    record('Started', datetime.datetime.now().astimezone().isoformat())
    record('Runtime choice', state.get('runtime_choice', 'unknown'))
    record('Runtime manifest', state.get('runtime') or 'system default')
    for name in ('TEMP', 'TMP'):
        record(name, os.environ.get(name, '(unset)'))
    try:
        temp = Path(tempfile.gettempdir())
        record('Temporary folder tested (Python selection)', temp)
        free = shutil.disk_usage(temp).free
        record('Temporary folder free bytes', free)
        # Create and delete only our own small probe file.
        with tempfile.TemporaryFile(dir=temp) as probe:
            probe.write(b'Lone Echo exit diagnostic\n')
            probe.flush()
            probe.seek(0)
            if probe.read() != b'Lone Echo exit diagnostic\n':
                raise OSError('Temporary file readback did not match')
        record('Temporary file write/read/delete', 'passed')
    except OSError as error:
        record('Temporary folder check failed', str(error))
    system = Path(os.environ.get('SystemRoot', r'C:\Windows'))/'System32'
    for name in ('dbgcore.dll', 'dbghelp.dll', 'symsrv.dll'):
        record('Debug library '+name,
               f'Game folder: {(game_bin/name).is_file()}\nSystem32: {(system/name).is_file()}')

    print('Starting LE2 using the installed launcher with diagnostics.', flush=True)
    print('Quit from the game menu, dismiss the error popup, and leave this window open.', flush=True)
    print(f'Report folder: {output}', flush=True)
    record('Launch', 'setup.exe launch le2 --diagnostics')
    command = [str(launcher), 'launch', 'le2', '--diagnostics']
    trace = output/'exception-trace.txt'
    if args.trace:
        record('External debugger', 'exit-trace.exe; game exceptions passed to game handlers')
        command = [str(tracer), str(trace), *command]
    console = output/'launcher-console.txt'
    with console.open('wb') as log:
        result = subprocess.run(command, cwd=package, stdout=log, stderr=subprocess.STDOUT)
    record('Launcher exit code', f'{result.returncode} (0x{result.returncode & 0xffffffff:08X})')
    record('Launcher console (includes game exit code)', tail(console))
    record('Finished', datetime.datetime.now().astimezone().isoformat())
    if args.trace:
        record('Exception trace (last 256 KiB)', tail(trace, 256 * 1024))
    for name, path in [('Game launch log', package/'userdata/le2/logs/launch.log'),
                       ('Runtime trace tail', game_bin/'libovr-openxr.log')]:
        record(name, tail(path))

    # Only collect application error events mentioning this game or its reporter.
    # BugSplat may handle a fault itself, so an empty result does not prove no crash.
    script = r"""$ErrorActionPreference='Stop'
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$events = Get-WinEvent -FilterHashtable @{LogName='Application';Id=1000,1001;StartTime=(Get-Date).AddHours(-2)} -ErrorAction SilentlyContinue |
    Where-Object { $_.Message -match 'loneecho2|BugSplat|BsSndRpt64' } |
    Select-Object -First 10 TimeCreated, Id, ProviderName, Message
if ($events) { $events | Format-List | Out-String -Width 240 } else { 'No matching Windows crash events found.' }
"""
    powershell = system/'WindowsPowerShell/v1.0/powershell.exe'
    try:
        events = subprocess.run([str(powershell), '-NoProfile', '-NonInteractive', '-Command', script],
                                capture_output=True, encoding='utf-8', errors='replace', timeout=30)
        record('Windows application events', events.stdout or '(Event query returned no output.)')
        if events.returncode or events.stderr:
            record('Event collection status', f'Exit {events.returncode}\n{events.stderr}')
    except (OSError, subprocess.TimeoutExpired) as error:
        record('Event collection unavailable', str(error))
    print(f'Finished. Share this file: {report}', flush=True)
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
