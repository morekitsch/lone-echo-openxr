#!/usr/bin/env python3
"""Build a game-free preview from an explicit allowlist. Never archive a game tree."""
from pathlib import Path
import hashlib
import json
import os
import shutil
import subprocess
import tomllib
import zipfile

ROOT=Path(__file__).resolve().parents[1]
INSTALLER=ROOT/'installer'
LICENSES=INSTALLER/'licenses'
LICENSES.mkdir(exist_ok=True)
shutil.copyfile(ROOT/'runtime/LICENSE-APACHE', LICENSES/'Apache-2.0.txt')
cargo=Path(os.environ.get('CARGO_HOME',ROOT/'working/cargo'))
lock=tomllib.loads((ROOT/'runtime/Cargo.lock').read_text())
notices=[]
for package in lock['package']:
    if 'source' not in package: continue
    name=f"{package['name']}-{package['version']}"
    matches=list((cargo/'registry/src').glob(f'*/{name}'))
    if len(matches)!=1:raise RuntimeError(f'Cannot find dependency source: {name}')
    source=matches[0]
    meta=tomllib.loads((source/'Cargo.toml').read_text())['package']
    notices.append(f"{name}\nLicense: {meta.get('license','see license file')}\nSource: {meta.get('repository','https://crates.io/crates/'+package['name'])}\n")
    for path in source.iterdir():
        if path.name.upper().startswith(('LICENSE','COPYING','COPYRIGHT','NOTICE')):
            if path.is_file():
                dest=LICENSES/'dependencies'/name/path.name
                dest.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(path,dest)
            elif path.is_dir():
                shutil.copytree(path,LICENSES/'dependencies'/name/path.name,dirs_exist_ok=True)
(LICENSES/'DEPENDENCIES.txt').write_text('\n'.join(notices))
sysroot=Path(subprocess.check_output(['rustc','+stable','--print','sysroot'],text=True).strip())
rustdoc=sysroot/'share/doc/rust'
shutil.copyfile(rustdoc/'COPYRIGHT-library.html',LICENSES/'Rust-standard-library-COPYRIGHT.html')
shutil.copytree(rustdoc/'licenses',LICENSES/'rust',dirs_exist_ok=True)
llvm=Path(os.environ.get('LLVM_MINGW_DIR',ROOT/'working/llvm-mingw-20260922-ucrt-ubuntu-22.04-x86_64'))
shutil.copyfile(llvm/'LICENSE.TXT',LICENSES/'LLVM-MinGW-LICENSE.txt')

entries={}
for name in ['echo_setup.py','steam_shortcuts.py','games.json','setup.sh','setup.cmd','README.md','RELEASE_NOTES.md','LICENSE.txt']:
    entries[name]=INSTALLER/name
manifest=json.loads((INSTALLER/'payload/manifest.json').read_text())
assert set(manifest)=={'LibOVRRT64_1.dll','LibOVRPlatform64_1.dll','openxr_loader.dll'}
for name,sha in manifest.items():
    path=INSTALLER/'payload'/name
    assert hashlib.sha256(path.read_bytes()).hexdigest()==sha
    entries['payload/'+name]=path
entries['payload/manifest.json']=INSTALLER/'payload/manifest.json'
windows=json.loads((INSTALLER/'windows-bundle.json').read_text())
for name,sha in windows['files'].items():
    assert name=='setup.exe' or (Path(name).parts[0]=='windows-python' and len(Path(name).parts)==2)
    path=INSTALLER/name
    assert hashlib.sha256(path.read_bytes()).hexdigest()==sha
    entries[name]=path
entries['windows-bundle.json']=INSTALLER/'windows-bundle.json'
for path in (INSTALLER/'icons').iterdir():
    if path.suffix in ('.svg','.png','.ico'):
        entries['icons/'+path.name]=path
for path in LICENSES.rglob('*'):
    if path.is_file():entries[path.relative_to(INSTALLER).as_posix()]=path
for name in ['Cargo.toml','Cargo.lock','UPSTREAM.md','LICENSE-APACHE','NOTICE']:
    entries['source/runtime/'+name]=ROOT/'runtime'/name
for folder in ['src','tests']:
    for path in (ROOT/'runtime'/folder).glob('*.rs'):
        entries['source/runtime/'+folder+'/'+path.name]=path
for name in ['build_runtime.sh','assemble_payload.py','build_release.py','build_windows_installer.py','verify_release.py','verify_windows_installer.py']:
    entries['source/tools/'+name]=ROOT/'tools'/name
entries['source/BUILDING.md']=ROOT/'BUILDING.md'
entries['source/windows_launcher.c']=INSTALLER/'windows_launcher.c'
entries['source/tests/test_installer.py']=ROOT/'tests/test_installer.py'

out=ROOT/'dist';out.mkdir(exist_ok=True)
archive=out/'lone-echo-openxr-0.1.0-preview.zip'
prefix='lone-echo-openxr-0.1.0-preview/'
with zipfile.ZipFile(archive,'w',compression=zipfile.ZIP_DEFLATED,compresslevel=9) as z:
    for folder in ['games/lone-echo/','games/lone-echo-2/']:
        z.writestr(prefix+folder,b'')
    for name,path in sorted(entries.items()):
        assert not any(part in ('originals','working','state','userdata','target','reports','__pycache__') for part in Path(name).parts)
        assert not name.startswith('games/')
        z.write(path,prefix+name)
with zipfile.ZipFile(archive) as z:
    assert z.testzip() is None
    executables={n for n in z.namelist() if n.lower().endswith('.exe')}
    assert executables=={prefix+n for n in windows['files'] if n.lower().endswith('.exe')}
    dlls=[n for n in z.namelist() if n.lower().endswith('.dll')]
    assert set(dlls)==({prefix+'payload/'+name for name in manifest} | {prefix+n for n in windows['files'] if n.lower().endswith('.dll')})
sha=hashlib.sha256(archive.read_bytes()).hexdigest()
(archive.with_suffix('.zip.sha256')).write_text(f'{sha}  {archive.name}\n')
(out/'release-audit.json').write_text(json.dumps({'archive':archive.name,'sha256':sha,'files':len(entries),'game_files':0,'empty_game_folders':['games/lone-echo','games/lone-echo-2'],'payload':manifest,'validation':{'le1_linux_wivrn':'User confirmed current preview wide VR view on repeat launches, input and correct desktop picture','le2_linux_wivrn':'VR gameplay working; user confirmed stable PC picture with diagnostics off after FOV-stencil correction','windows_le1_vdxr':'User confirmed installer and game work after D3D11 typed-view correction','windows_le2_vdxr':'User confirmed startup, gameplay and placement; latest resolution/statistics build reported working great after requested intro-blackout retest','d3d12_graphics_wine':'Optimized GPU test passed: color views, depth create/clear, three handoffs and pixel readback','tracking_origin':'Eye/floor/recenter math tests passed; user confirmed Windows LE2 placement fix; Linux headset retest pending','render_resolution_and_perf_stats':'20 portable tests passed; Windows build passed; user confirmed Windows/VDXR LE2 update; Linux LE1 repeat launches confirmed; broader gameplay tests pending', 'windows_steamvr':'Untested','d3d11_views_wine':'Optimized real-device test passed: default/explicit HDR views, pixel readback, array/MSAA and untagged isolation','le2_desktop_mirror':'FOV-stencil placeholder returned logging-dependent values; explicit unsupported result confirmed to stop PC flicker on Linux/WiVRn with diagnostics off; Windows retest pending'}},indent=2)+'\n')
print(f'{archive}\nSHA-256: {sha}\n{len(entries)} allowlisted files; no game files, saves, backups, or logs.')
