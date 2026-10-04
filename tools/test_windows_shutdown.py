"""Run the installed LE2 once with a candidate DLL, then restore the installation."""
import argparse
import contextlib
import datetime
import hashlib
import json
import os
from pathlib import Path
import sys
from types import SimpleNamespace


def digest(data):
    return hashlib.sha256(data).hexdigest()


def tail(path, limit=32000):
    try:
        with path.open('rb') as stream:
            stream.seek(max(0, path.stat().st_size - limit))
            return stream.read().decode('utf-8', errors='replace')
    except OSError as error:
        return str(error)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--package', type=Path, required=True)
    args = parser.parse_args()
    if os.name != 'nt':
        parser.error('Run on Windows with the installed package Python.')
    package = args.package.resolve()
    # Load exactly this package's installer; its lock and launch checks remain
    # in effect. This helper runs with private Python in isolated mode.
    sys.path.insert(0, str(package))
    import echo_setup as setup
    if setup.ROOT != package:
        raise RuntimeError('Unexpected installer module location')
    candidate_dir = Path(__file__).resolve().parent
    candidate = (candidate_dir/'LibOVRRT64_1.dll').read_bytes()
    expected = (candidate_dir/'SHA256SUMS').read_text().split()[0]
    if digest(candidate) != expected:
        raise RuntimeError('Candidate DLL checksum does not match')
    with setup.operation_lock('le2'):
        state_path = package/'state/le2/install.json'
        original_state = state_path.read_bytes()
        state = json.loads(original_state)
        if not state['complete']:
            raise RuntimeError('Install LE2 before running this test')
        game_bin = Path(state['game_dir'])/'bin/win10'
        target = game_bin/'LibOVRRT64_1.dll'
        original = target.read_bytes()
        entry = next(item for item in state['files'] if Path(item['path']) == target)
        if digest(original) != entry['installed']:
            raise RuntimeError('Installed DLL was changed; leaving it untouched')
        stamp = datetime.datetime.now().strftime('%Y%m%d-%H%M%S-%f')
        output = package/'userdata/le2/logs'/('shutdown-check-'+stamp)
        output.mkdir(parents=True)
        (output/'original-runtime.dll').write_bytes(original)
        (output/'original-install.json').write_bytes(original_state)
        entry['installed'] = expected
        candidate_state = (json.dumps(state, indent=2)+'\n').encode()
        report = output/'shutdown-report.txt'
        report.write_text(f'Candidate SHA256: {expected}\nOriginal SHA256: {digest(original)}\n', encoding='utf-8')
        print('Starting LE2 with the shutdown fix for this test only.', flush=True)
        print('Quit normally and dismiss any popup. Wait for restoration before closing this window.', flush=True)
        print(f'Report: {report}', flush=True)
        try:
            setup.atomic(target, candidate)
            setup.atomic(state_path, candidate_state)
            with (output/'launcher-console.txt').open('w', encoding='utf-8') as console:
                with contextlib.redirect_stdout(console):
                    code = setup.launch(SimpleNamespace(game='le2', diagnostics=True, check=False, timeout=None))
            with report.open('a', encoding='utf-8') as log:
                log.write(f'Game exit: {code} (0x{code & 0xffffffff:08X})\n')
        finally:
            # Avoid clobbering any unexpected manual edit. Retain backups and
            # the report even if the launcher or restoration fails.
            try:
                with report.open('a', encoding='utf-8') as log:
                    log.write('\nLauncher console\n'+tail(output/'launcher-console.txt'))
                    log.write('\nGame launch log\n'+tail(package/'userdata/le2/logs/launch.log'))
                    log.write('\nRuntime log tail\n'+tail(game_bin/'libovr-openxr.log'))
            finally:
                if target.read_bytes() not in (candidate, original) or state_path.read_bytes() not in (candidate_state, original_state):
                    raise RuntimeError(f'Files changed unexpectedly; backups retained in {output}')
                setup.atomic(target, original)
                setup.atomic(state_path, original_state)
                with report.open('a', encoding='utf-8') as log:
                    log.write('\nOriginal DLL and installation state restored.\n')
                print('Original DLL and installation state restored.', flush=True)
        print(f'Share this file: {report}', flush=True)


if __name__ == '__main__':
    main()
