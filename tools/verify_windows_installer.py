"""Run the packaged Windows launcher with private CPython under isolated Wine."""
from pathlib import Path
import argparse
import json
import os
import subprocess
import tempfile
import zipfile

ROOT=Path(__file__).resolve().parents[1]
parser=argparse.ArgumentParser(description=__doc__)
for game in ('le1','le2'):
    parser.add_argument(f'--{game}-source',type=Path,help='Folder containing original bin and _data directories')
options=parser.parse_args()
env=os.environ | {'WINEPREFIX':str(ROOT/'working/windows-installer-prefix'),'WINEDEBUG':'-all'}
# These intentionally invalid values must not affect the isolated runtime.
env.update(PYTHONHOME=r'C:\missing-python',PYTHONPATH=r'C:\missing-modules')
results=[]
with tempfile.TemporaryDirectory(prefix='windows-package-check-',dir=ROOT/'working') as td:
    temp=Path(td)/'Installer with spaces Ω';temp.mkdir()
    with zipfile.ZipFile(ROOT/'dist/lone-echo-openxr-0.1.0-preview.zip') as z:z.extractall(temp)
    package=temp/'lone-echo-openxr-0.1.0-preview'
    executable=package/'setup.exe'
    def run(*args, input=None, expected=0):
        result=subprocess.run(['wine',str(executable),*args],env=env,cwd=temp,input=input,
                              text=True,encoding='utf-8',capture_output=True,timeout=45)
        if result.returncode!=expected:
            raise AssertionError((args,result.returncode,result.stdout,result.stderr))
        return result.stdout
    assert 'uninstall' in run('--help')
    assert 'Install a game' in run(input='0\n')
    # Invalid commands retain an error exit code for scripts.
    run('not-a-command',expected=2)
    games=json.loads((package/'games.json').read_text())
    for game,meta in games.items():
        selected=getattr(options,f'{game}_source')
        if selected is None:
            matches=list((ROOT/'originals'/meta['folder']).rglob(meta['exe']))
            if len(matches)!=1:
                raise ValueError(f'Found {len(matches)} original {game} binaries; pass --{game}-source PATH.')
            source=matches[0].parent
        else:
            selected=selected.expanduser().resolve()
            if not (selected/'_data').is_dir() or not (selected/meta['bin']/meta['exe']).is_file():
                raise ValueError(f'Not a game folder: {selected}')
            source=selected/meta['bin']
        root=temp/'Games with spaces Ω'/meta['name'];bindir=root/meta['bin']
        bindir.mkdir(parents=True);(root/'_data').mkdir()
        original={}
        for patch in meta['patches']:
            data=(source/patch['file']).read_bytes();original[patch['file']]=data
            (bindir/patch['file']).write_bytes(data)
        (root/'save.dat').write_bytes(b'preserve player progress')
        windows_root='Z:'+str(root).replace('/','\\')
        run('install',game,'--game-dir',windows_root+'\\','--runtime','auto','--no-shortcuts')
        state=json.loads(run('status',game))
        assert state['complete'] and state['proton'] is None
        launch=json.loads(run('launch',game,'--check'))
        assert len(launch['command'])==1 and launch['command'][0].endswith(meta['exe'])
        run('uninstall',game)
        for name,data in original.items():assert (bindir/name).read_bytes()==data
        for name in ('LibOVRRT64_1.dll','LibOVRPlatform64_1.dll','openxr_loader.dll'):
            assert not (bindir/name).exists()
        assert (root/'save.dat').read_bytes()==b'preserve player progress'
        assert not (package/'state'/game).exists()
        results.append({'game':game,'windows_exe_install_uninstall_under_wine':'pass',
                        'unicode_and_spaces':'pass','system_python_required':False,
                        'originals_and_saves_preserved':True,'native_windows_headset_test':False})
    (ROOT/'reports').mkdir(exist_ok=True)
    (ROOT/'reports/windows-installer-validation.json').write_text(json.dumps(results,indent=2)+'\n')
print('Windows setup.exe: help/menu, exit codes, both game installs/preflights/uninstalls, Unicode/spaces and private Python isolation passed under Wine.')
