"""Bundle pinned official CPython and build a native Windows console launcher."""
from pathlib import Path
import hashlib
import json
import os
import shutil
import subprocess
import zipfile

ROOT=Path(__file__).resolve().parents[1]
VERSION='3.13.16'
ARCHIVE=f'python-{VERSION}-embed-amd64.zip'
URL=f'https://www.python.org/ftp/python/{VERSION}/{ARCHIVE}'
SHA256='97dae5274cc54867065e8d5a3226e48c35017ed332a0fdb0e27d5b5821961297'
source=ROOT/'working'/ARCHIVE
if not source.is_file():
    raise SystemExit(f'Download {URL} to {source} first.')
if hashlib.sha256(source.read_bytes()).hexdigest()!=SHA256:
    raise SystemExit('Bundled Python download checksum does not match python.org.')
destination=ROOT/'installer/windows-python'
if destination.exists(): shutil.rmtree(destination)
destination.mkdir()
with zipfile.ZipFile(source) as z:
    for entry in z.infolist():
        if Path(entry.filename).name!=entry.filename:
            raise ValueError(f'Unexpected archive path: {entry.filename}')
        (destination/entry.filename).write_bytes(z.read(entry))
# Keep isolated imports; allow our installer module beside the runtime folder.
(destination/'python313._pth').write_text('python313.zip\n.\n..\n',encoding='utf-8')
toolchain=Path(os.environ.get('LLVM_MINGW_DIR',ROOT/'working/llvm-mingw-20260922-ucrt-ubuntu-22.04-x86_64'))
build=ROOT/'working/windows-installer-build';build.mkdir(exist_ok=True)
resource=build/'launcher.rc'
application_manifest=build/'launcher.manifest'
application_manifest.write_text('''<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
<trustInfo xmlns="urn:schemas-microsoft-com:asm.v3"><security><requestedPrivileges>
<requestedExecutionLevel level="asInvoker" uiAccess="false"/>
</requestedPrivileges></security></trustInfo>
</assembly>
''')
resource.write_text('1 ICON "'+(ROOT/'installer/icons/le1.ico').as_posix()+'"\n1 24 "'+application_manifest.as_posix()+'"\n')
obj=build/'launcher.res'
subprocess.run([str(toolchain/'bin/x86_64-w64-mingw32-windres'),str(resource),'-O','coff','-o',str(obj)],check=True)
exe=ROOT/'installer/setup.exe'
subprocess.run([str(toolchain/'bin/x86_64-w64-mingw32-clang'),'-std=c11','-Wall','-Wextra','-Werror','-O2','-municode','-static','-Wl,--no-insert-timestamp',str(ROOT/'installer/windows_launcher.c'),str(obj),'-o',str(exe)],check=True)
files={'setup.exe':hashlib.sha256(exe.read_bytes()).hexdigest()}
files.update({'windows-python/'+p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(destination.iterdir())})
manifest={'python_version':VERSION,'source':URL,'archive_sha256':SHA256,'files':files}
(ROOT/'installer/windows-bundle.json').write_text(json.dumps(manifest,indent=2)+'\n')
print(f'Built setup.exe with isolated CPython {VERSION}; {len(files)} bundled files.')
