#!/usr/bin/env python3
"""Install, launch, and uninstall Lone Echo OpenXR. Windows bundles Python; Linux needs Python 3.10+."""
import argparse
from contextlib import contextmanager
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import struct
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parent
GAMES = json.loads((ROOT / 'games.json').read_text())
STATE = ROOT / 'state'


@contextmanager
def operation_lock(game):
    STATE.mkdir(parents=True,exist_ok=True)
    with (STATE/f'.{game}.lock').open('a+b') as stream:
        stream.seek(0)
        if stream.read(1)==b'':
            stream.write(b'0'); stream.flush()
        stream.seek(0)
        try:
            if os.name=='nt':
                import msvcrt
                msvcrt.locking(stream.fileno(),msvcrt.LK_NBLCK,1)
            else:
                import fcntl
                fcntl.flock(stream,fcntl.LOCK_EX|fcntl.LOCK_NB)
        except OSError:
            raise ValueError('This game is running or another installer operation is active.') from None
        try:
            yield
        finally:
            if os.name=='nt':
                stream.seek(0);msvcrt.locking(stream.fileno(),msvcrt.LK_UNLCK,1)
            else:
                fcntl.flock(stream,fcntl.LOCK_UN)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def atomic(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, name = tempfile.mkstemp(prefix='.' + path.name, dir=path.parent)
    try:
        with os.fdopen(fd, 'wb') as f:
            f.write(data)
            f.flush()
            os.fsync(f.fileno())
        os.replace(name, path)
    finally:
        if os.path.exists(name):
            os.unlink(name)


def save(game, state):
    atomic(STATE / game / 'install.json', (json.dumps(state, indent=2) + '\n').encode())


def load(game):
    p = STATE / game / 'install.json'
    if not p.exists():
        raise ValueError(f'{game} is not installed. Run install first.')
    return json.loads(p.read_text())


def rva_offset(data, rva):
    if data[:2] != b'MZ':
        raise ValueError('Not a Windows executable')
    pe = struct.unpack_from('<I', data, 0x3c)[0]
    if data[pe:pe+4] != b'PE\0\0':
        raise ValueError('Invalid PE header')
    count = struct.unpack_from('<H', data, pe+6)[0]
    optional = struct.unpack_from('<H', data, pe+20)[0]
    for i in range(count):
        start = pe + 24 + optional + 40*i
        size, address, raw_size, raw = struct.unpack_from('<IIII', data, start+8)
        if address <= rva < address + raw_size:
            return raw + rva-address
    raise ValueError(f'RVA {rva:#x} is outside the executable')


def patch(data, spec):
    if digest(data) != spec['sha256']:
        raise ValueError(f"Unsupported or already modified {spec['file']}. Use the supported unmodified game build.")
    out = bytearray(data)
    for rva, before, after in spec['edits']:
        before, after = bytes.fromhex(before), bytes.fromhex(after)
        offset = rva_offset(data, rva)
        if len(before) != len(after) or data[offset:offset+len(before)] != before:
            raise ValueError(f"Unexpected instructions in {spec['file']} at {rva:#x}")
        out[offset:offset+len(before)] = after
    return bytes(out)


def put(game, state, path, data):
    path = path.absolute()
    if path.is_symlink():
        raise ValueError(f'Refusing to replace a symlink: {path}')
    old = path.read_bytes() if path.exists() else None
    entry = {'path': str(path), 'installed': digest(data), 'original': digest(old) if old is not None else None}
    if old is not None:
        backup = STATE / game / 'backups' / str(len(state['files']))
        atomic(backup, old)
        entry['backup'] = str(backup)
    state['files'].append(entry)
    save(game, state)  # Record recovery information before replacing a file.
    atomic(path, data)


def steam_root():
    candidates = [Path.home()/'.local/share/Steam', Path.home()/'.steam/steam', Path.home()/'.var/app/com.valvesoftware.Steam/.local/share/Steam']
    if os.name == 'nt':
        import winreg
        try:
            with winreg.OpenKey(winreg.HKEY_CURRENT_USER, r'Software\Valve\Steam') as k:
                candidates.insert(0, Path(winreg.QueryValueEx(k, 'SteamPath')[0]))
        except OSError:
            pass
    return next((p.resolve() for p in candidates if p.is_dir()), None)


def runtime_path(choice):
    if choice == 'auto':
        return None
    candidates = []
    steam = steam_root()
    if choice == 'steamvr' and steam:
        suffix = 'win64' if os.name == 'nt' else 'linux64'
        candidates.append(steam / f'steamapps/common/SteamVR/steamxr_{suffix}.json')
        # SteamVR can live in another Steam library.
        libraries = steam / 'steamapps/libraryfolders.vdf'
        if libraries.exists():
            import re
            for path in re.findall(r'"path"\s*"([^"]+)"', libraries.read_text()):
                candidates.append(Path(path.replace('\\\\', '\\')) / f'steamapps/common/SteamVR/steamxr_{suffix}.json')
    elif choice == 'vdxr' and os.name == 'nt':
        candidates.append(Path(os.environ.get('ProgramFiles', r'C:\Program Files')) / 'Virtual Desktop Streamer/OpenXR/virtualdesktop-openxr.json')
    elif choice == 'wivrn' and os.name != 'nt':
        candidates = [Path('/usr/share/openxr/1/openxr_wivrn.json'), Path('/usr/local/share/openxr/1/openxr_wivrn.json')]
        for base in [Path('/var/lib/flatpak'), Path.home()/'.local/share/flatpak']:
            candidates.extend(base.glob('app/io.github.wivrn.wivrn/*/stable/active/files/share/openxr/1/openxr_wivrn.json'))
    elif choice not in ('steamvr', 'vdxr', 'wivrn'):
        candidates = [Path(choice).expanduser().resolve()]
    for p in candidates:
        if p.is_file():
            description = json.loads(p.read_text())
            library = Path(description['runtime']['library_path'])
            if not library.is_absolute():
                library = p.parent / library
            if not library.is_file():
                raise ValueError(f'Runtime library is missing: {library}')
            return str(p)
    raise ValueError(f'Cannot find the {choice} OpenXR runtime. Install it, use auto, or pass its manifest JSON path.')


def proton_path(choice=None):
    if choice:
        p = Path(choice).expanduser().resolve()
        p = p.parent if p.name == 'proton' else p
        if (p/'proton').is_file():
            return str(p)
        raise ValueError(f'No Proton launcher in {p}')
    steam = steam_root()
    if steam:
        preferred = steam/'compatibilitytools.d/Proton-GE Latest'
        candidates = [preferred] + sorted((steam/'compatibilitytools.d').glob('*'), key=lambda p:p.stat().st_mtime, reverse=True)
        for p in candidates:
            if (p/'proton').is_file():
                return str(p.resolve())
    raise ValueError('Install GE-Proton, then pass --proton /path/to/Proton. Linux needs a Proton build with OpenXR support.')


def desktop_quote(s):
    # Desktop Entry quoting differs from shell quoting. Percent is a field code.
    return '"' + str(s).replace('\\', '\\\\').replace('"', '\\"').replace('`', '\\`').replace('$', '\\$').replace('%', '%%') + '"'


def steam_or_desktop_icon(game, state):
    source = ROOT / 'icons' / f'{game}.png'
    if os.name == 'nt':
        return source
    # WiVRn's Flatpak can read application entries but not arbitrary package
    # directories. Keep the icon beside the entry, without changing permissions.
    data_home = Path(os.environ.get('XDG_DATA_HOME', Path.home()/'.local/share'))
    icon = (data_home/'applications'/f'lone-echo-openxr-{game}.png').absolute()
    if not any(entry['path'] == str(icon) for entry in state['files']):
        put(game, state, icon, source.read_bytes())
    elif not icon.is_file() or digest(icon.read_bytes()) != digest(source.read_bytes()):
        raise ValueError(f'Installed icon changed or is missing: {icon}')
    return icon


def shortcuts(game, state):
    title = GAMES[game]['name'] + (' (OpenXR)' if os.name=='nt' else ' — Direct')
    if os.name == 'nt':
        # WScript resolves redirected/localized Desktop and Start Menu paths.
        script = "[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false); $w=New-Object -ComObject WScript.Shell; $w.SpecialFolders('Desktop'); $w.SpecialFolders('Programs')"
        folders = subprocess.check_output(['powershell', '-NoProfile', '-NonInteractive', '-Command', script], text=True, encoding='utf-8').splitlines()
        for folder in folders:
            dest = Path(folder) / (title + '.lnk')
            with tempfile.TemporaryDirectory() as td:
                out = Path(td)/'shortcut.lnk'
                launcher=ROOT/'setup.exe'
                target=str(launcher) if launcher.is_file() else sys.executable
                arguments=['launch',game] if launcher.is_file() else [str(ROOT/'echo_setup.py'),'launch',game]
                env = os.environ | {'LE_LINK':str(out), 'LE_TARGET':target, 'LE_ARGUMENTS':subprocess.list2cmdline(arguments), 'LE_ROOT':str(ROOT), 'LE_ICON':str(ROOT/'icons'/f'{game}.ico')}
                script = '$w=New-Object -ComObject WScript.Shell; $s=$w.CreateShortcut($env:LE_LINK); $s.TargetPath=$env:LE_TARGET; $s.Arguments=$env:LE_ARGUMENTS; $s.WorkingDirectory=$env:LE_ROOT; $s.IconLocation=$env:LE_ICON; $s.Save()'
                subprocess.run(['powershell','-NoProfile','-NonInteractive','-Command',script],env=env,check=True)
                put(game,state,dest,out.read_bytes())
    else:
        data_home = Path(os.environ.get('XDG_DATA_HOME', Path.home()/'.local/share'))
        command = ' '.join(desktop_quote(x) for x in (sys.executable, ROOT/'echo_setup.py', 'launch', game))
        icon = steam_or_desktop_icon(game, state)
        content = f'[Desktop Entry]\nType=Application\nName={title}\nExec={command}\nTerminal=false\nCategories=Game;X-WiVRn-VR;\nIcon={icon}\nComment=Launch directly with the selected OpenXR runtime\nStartupNotify=false\n'
        put(game,state,data_home/'applications'/f'lone-echo-openxr-{game}.desktop',content.encode())


def register_steam(game, state, user=None):
    import steam_shortcuts as steam_vdf
    steam_vdf.check_closed()
    if state.get('steam'):
        print('Steam shortcut is already registered.')
        return
    steam = steam_root()
    if not steam:
        raise ValueError('Steam was not found. Desktop and WiVRn launch entries are available independently.')
    profiles = [p for p in (steam/'userdata').glob('*') if p.name.isdigit() and p.name!='0' and (p/'config').is_dir()]
    if user:
        profiles = [p for p in profiles if p.name==user]
    if len(profiles)!=1:
        raise ValueError('Select a Steam user with --steam-user ID. Available: '+', '.join(p.name for p in profiles))
    path=profiles[0]/'config/shortcuts.vdf'
    old=path.read_bytes() if path.exists() else b''
    import shlex
    exe='"'+sys.executable+'"'
    options=shlex.join([str(ROOT/'echo_setup.py'),'launch',game])
    icon = steam_or_desktop_icon(game, state)
    data,identity=steam_vdf.add(old,GAMES[game]['name']+' — Steam',exe,'"'+str(ROOT)+'"',options,icon=str(icon))
    backup=STATE/game/'steam-shortcuts.original.vdf'
    atomic(backup,old)
    state['steam']={'path':str(path),'identity':identity,'backup':str(backup),'existed':path.exists()}
    save(game,state)
    atomic(path,data)
    print('Registered Steam VR shortcut. Restart Steam to refresh the VR library.')


def install(args):
    game = args.game
    meta = GAMES[game]
    if (STATE/game/'install.json').exists():
        raise ValueError('Already installed, or an incomplete install needs recovery. Run uninstall before reinstalling.')
    root = (args.game_dir or ROOT/'games'/meta['folder']).expanduser().resolve()
    if not (root/'_data').is_dir():
        raise ValueError(f'Missing game data folder: {root / "_data"}. Copy the complete game installation folder.')
    bindir = root/meta['bin']
    prepared = [(bindir/spec['file'], patch((bindir/spec['file']).read_bytes(),spec)) for spec in meta['patches']]
    payload = json.loads((ROOT/'payload/manifest.json').read_text())
    if set(payload) != {'LibOVRRT64_1.dll','LibOVRPlatform64_1.dll','openxr_loader.dll'}:
        raise ValueError('The release payload is incomplete or unexpected')
    for name, sha in payload.items():
        data = (ROOT/'payload'/name).read_bytes()
        if digest(data) != sha:
            raise ValueError(f'Payload checksum failed: {name}')
        prepared.append((bindir/name,data))
    runtime = runtime_path(args.runtime)
    proton = proton_path(args.proton) if os.name != 'nt' else None
    state = {'version':1,'game_dir':str(root),'runtime':runtime,'runtime_choice':args.runtime,'proton':proton,'files':[], 'complete':False}
    save(game,state)
    try:
        for path,data in prepared:
            put(game,state,path,data)
        if not args.no_shortcuts:
            shortcuts(game,state)
        if os.name != 'nt' and not args.no_shortcuts:
            try:
                register_steam(game,state,getattr(args,'steam_user',None))
            except ValueError as error:
                print(f'Steam registration pending: {error}')
        state['complete'] = True
        save(game,state)
    except Exception:
        print('Installation interrupted. Run uninstall to restore recorded changes.',file=sys.stderr)
        raise
    launcher='.\\setup.exe' if os.name=='nt' and (ROOT/'setup.exe').is_file() else 'python echo_setup.py'
    print(f'Installed {meta["name"]}: {root}\nRuntime: {args.runtime}\nLaunch: {launcher} launch {game}')


def uninstall(args):
    game = args.game
    state = load(game)
    if state.get('steam'):
        import steam_shortcuts as steam_vdf
        steam_vdf.check_closed()
        entry=state['steam']
        path=Path(entry['path'])
        if path.exists():
            updated=steam_vdf.remove(path.read_bytes(),entry['identity'])
            if not entry['existed'] and not steam_vdf.shortcut_list(updated)[1]:
                path.unlink()
            else:
                atomic(path,updated)
        del state['steam']
        save(game,state)
    remaining = []
    for entry in reversed(state['files']):
        path = Path(entry['path'])
        data = path.read_bytes() if path.is_file() and not path.is_symlink() else None
        current = digest(data) if data is not None else None
        if current == entry['original']:
            continue
        if path.is_symlink() or current != entry['installed']:
            # A missing generated file is already removed. A missing original needs restoration.
            if path.exists() or path.is_symlink():
                print(f'Preserved changed file: {path}')
                remaining.append(entry)
                continue
        if entry['original'] is not None:
            backup = Path(entry['backup']).read_bytes()
            if digest(backup) != entry['original']:
                raise ValueError(f'Backup checksum failed: {entry["backup"]}')
            atomic(path,backup)
        elif path.exists():
            path.unlink()
    if remaining:
        state['files'] = list(reversed(remaining))
        state['complete'] = False
        save(game,state)
        raise ValueError('Some installed files changed after installation. Their backups and uninstall record were retained for manual review.')
    shutil.rmtree(STATE/game)
    print(f'Uninstalled {GAMES[game]["name"]} compatibility files and shortcuts. Game files, saves, and Proton prefix preserved.')


def launch(args):
    game = args.game
    state = load(game)
    if not state['complete']:
        raise ValueError('Installation is incomplete. Uninstall and reinstall.')
    meta = GAMES[game]
    root = Path(state['game_dir'])
    executable = root/meta['bin']/meta['exe']
    for entry in state['files']:
        path=Path(entry['path'])
        if path.parent==executable.parent and (not path.is_file() or digest(path.read_bytes())!=entry['installed']):
            raise ValueError(f'Installed file changed or is missing: {path}')
    env = os.environ.copy()
    env['LONE_ECHO_RENDERER'] = meta['renderer']
    if state['runtime']:
        if not Path(state['runtime']).is_file():
            raise ValueError('The selected OpenXR runtime was moved or removed. Reconfigure with the runtime command.')
        env['XR_RUNTIME_JSON'] = state['runtime']
    # In auto mode honor the loader's normal selection, including user environment overrides.
    if args.diagnostics:
        env.update(LIBOVR_OPENXR_LOG='1',XR_LOADER_DEBUG='all')
    if os.name == 'nt':
        command = [str(executable)]
    else:
        proton = Path(state['proton'])
        prefix = ROOT/'userdata'/game/'proton'
        prefix.mkdir(parents=True,exist_ok=True)
        env.update(STEAM_COMPAT_CLIENT_INSTALL_PATH=str(steam_root() or Path.home()/'.local/share/Steam'), STEAM_COMPAT_DATA_PATH=str(prefix), STEAM_COMPAT_INSTALL_PATH=str(root), STEAM_COMPAT_APP_ID='0',SteamAppId='0',SteamGameId='0', UMU_ID='umu-default', DXVK_NO_VR='1', VR_PATHREG_OVERRIDE=os.devnull, WINEDLLOVERRIDES='libovrrt64_1,libovrplatform64_1,openxr_loader=n', PRESSURE_VESSEL_IMPORT_OPENXR_1_RUNTIMES='1')
        env.pop('VR_OVERRIDE',None)
        command = [str(proton/'proton'),'run',str(executable)]
    if args.check:
        print(json.dumps({'command':command,'runtime':state['runtime'] or 'system default','renderer':meta['renderer']},indent=2))
        return
    logdir = ROOT/'userdata'/game/'logs'
    logdir.mkdir(parents=True,exist_ok=True)
    with (logdir/'launch.log').open('wb') as log:
        process = subprocess.Popen(command,cwd=root,env=env,stdout=log,stderr=subprocess.STDOUT,start_new_session=os.name!='nt')
        try:
            code = process.wait(timeout=args.timeout)
        except (subprocess.TimeoutExpired,KeyboardInterrupt):
            if os.name != 'nt':
                subprocess.run([str(proton/'files/bin/wineserver'),'-k'],env=env|{'WINEPREFIX':str(prefix/'pfx')},stdout=log,stderr=log,timeout=10)
                if process.poll() is None:
                    os.killpg(process.pid,signal.SIGTERM)
            else:
                process.terminate()
            process.wait(timeout=15)
            code = 124
    print(f'Exit: {code}; log: {logdir/"launch.log"}')
    return code


def interactive_menu():
    print('Lone Echo OpenXR setup\nCopy your game files first; see README.md for folder instructions.')
    actions={'1':'install','2':'uninstall','3':'runtime','4':'status'}
    try:
        while True:
            print('\n1. Install a game\n2. Uninstall (keep games and saves)\n3. Change OpenXR runtime\n4. Show installation status\n0. Exit')
            selected=input('Choose: ').strip()
            if selected=='0': return 0
            if selected not in actions: continue
            game=input('Game: 1 = Lone Echo I, 2 = Lone Echo II: ').strip()
            if game not in ('1','2'): continue
            command=actions[selected]; game='le'+game
            arguments=[command,game]
            if command=='install':
                default=ROOT/'games'/GAMES[game]['folder']
                folder=input(f'Game folder [Enter for {default}]: ').strip().strip('"')
                if folder: arguments+=['--game-dir',folder]
            if command in ('install','runtime'):
                choices='auto, steamvr, vdxr' if os.name=='nt' else 'auto, wivrn, steamvr'
                runtime=input(f'OpenXR runtime ({choices}, or JSON path) [auto]: ').strip().strip('"') or 'auto'
                arguments+=['--runtime',runtime] if command=='install' else [runtime]
            try:
                main(arguments)
            except SystemExit:
                pass  # Show errors and return to the menu instead of closing the window.
    except EOFError:
        return 0
    except KeyboardInterrupt:
        print('\nStopped.')
        return 130


def main(argv=None):
    argv=sys.argv[1:] if argv is None else argv
    if not argv: return interactive_menu()
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command',required=True)
    for name in ('install','launch','uninstall','runtime','status','register-steam'):
        p=commands.add_parser(name)
        p.add_argument('game',choices=GAMES)
        if name in ('install','register-steam'):
            p.add_argument('--steam-user')
        if name=='install':
            p.add_argument('--game-dir',type=Path)
            p.add_argument('--runtime',default='auto',help='auto, wivrn, steamvr, vdxr, or an OpenXR JSON manifest')
            p.add_argument('--proton')
            p.add_argument('--no-shortcuts',action='store_true',help=argparse.SUPPRESS)
        elif name=='launch':
            p.add_argument('--check',action='store_true')
            p.add_argument('--diagnostics',action='store_true')
            p.add_argument('--timeout',type=float)
        elif name=='runtime':
            p.add_argument('runtime')
    args=parser.parse_args(argv)
    if args.command=='launch' and args.timeout is not None and args.timeout<=0:
        parser.error('--timeout must be positive')
    try:
        with operation_lock(args.game):
            return dispatch(args)
    except (OSError,ValueError,KeyError,struct.error,subprocess.SubprocessError) as e:
        parser.exit(1,f'Stopped: {e}\n')


def dispatch(args):
    if args.command=='install':return install(args)
    if args.command=='uninstall':return uninstall(args)
    if args.command=='launch':return launch(args)
    state=load(args.game)
    if args.command=='register-steam':return register_steam(args.game,state,args.steam_user)
    if args.command=='runtime':
        state['runtime']=runtime_path(args.runtime)
        state['runtime_choice']=args.runtime
        save(args.game,state)
    print(json.dumps(state,indent=2))

if __name__=='__main__':
    sys.exit(main())
