"""Collect only redistributable DLLs; pefile is a build-time dependency."""
from pathlib import Path
import hashlib
import json
import subprocess
import zipfile
import pefile
ROOT=Path(__file__).resolve().parents[1]
PAYLOAD=ROOT/'installer/payload'
PAYLOAD.mkdir(exist_ok=True)
runtime=(ROOT/'runtime/target/x86_64-pc-windows-gnullvm/release/libovr_openxr.dll').read_bytes()
with pefile.PE(data=runtime) as pe:
    exports=sorted(e.name.decode() for e in pe.DIRECTORY_ENTRY_EXPORT.symbols if e.name)
    assert not any(i.dll.lower() in (b'libunwind.dll', b'libgcc_s_seh-1.dll') for i in pe.DIRECTORY_ENTRY_IMPORT), 'Build with static compiler support'
build=ROOT/'working/unified-build';build.mkdir(exist_ok=True)
definition=build/'platform.def'
definition.write_text('LIBRARY LibOVRPlatform64_1\nEXPORTS\n'+''.join(f' {name}=LibOVRRT64_1.{name}\n' for name in exports))
anchor=build/'forwarder.c'
anchor.write_text('const char lone_echo_platform_forwarder = 0;\n')
obj=build/'forwarder.obj'
subprocess.run(['clang','--target=x86_64-pc-windows-msvc','-ffreestanding','-c',str(anchor),'-o',str(obj)],check=True)
subprocess.run(['lld-link','/dll','/noentry','/nodefaultlib','/machine:x64','/timestamp:0',f'/def:{definition}',f'/out:{PAYLOAD}/LibOVRPlatform64_1.dll',str(obj)],check=True)
assert (PAYLOAD/'LibOVRPlatform64_1.dll').is_file()
with pefile.PE(str(PAYLOAD/'LibOVRPlatform64_1.dll')) as pe:
    assert {e.name.decode() for e in pe.DIRECTORY_ENTRY_EXPORT.symbols if e.name} == set(exports)
    assert all(e.forwarder and e.forwarder.startswith(b'LibOVRRT64_1.') for e in pe.DIRECTORY_ENTRY_EXPORT.symbols)
(PAYLOAD/'LibOVRRT64_1.dll').write_bytes(runtime)
with zipfile.ZipFile(ROOT/'working/openxr_loader_windows-1.1.63.zip') as z:
    loader=z.read('x64/bin/openxr_loader.dll')
    assert hashlib.sha256(loader).hexdigest()=='66946ea6e9cc2649e299ec412521c5eafe56c635d05055097fe4db81e11932df'
    (PAYLOAD/'openxr_loader.dll').write_bytes(loader)
    licenses=ROOT/'installer/licenses';licenses.mkdir(exist_ok=True)
    (licenses/'OpenXR-loader-LICENSE.txt').write_bytes(z.read('share/doc/openxr/LICENSE'))
(PAYLOAD/'manifest.json').write_text(json.dumps({p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(PAYLOAD.glob('*.dll'))},indent=2)+'\n')
print('Payload assembled: shared runtime, platform export forwarder, standard OpenXR loader.')
