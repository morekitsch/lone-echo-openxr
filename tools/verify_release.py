"""Exercise the extracted installer against both supported originals in isolation."""
import importlib.util
import argparse
import json
import os
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import zipfile

ROOT=Path(__file__).resolve().parents[1]
parser=argparse.ArgumentParser(description=__doc__)
for game in ('le1','le2'):
    parser.add_argument(f'--{game}-source',type=Path,help='Original game folder containing _data and bin')
options=parser.parse_args()

def source_folder(game, meta):
    selected=getattr(options,f'{game}_source')
    if selected is None:
        base=ROOT/'originals'/meta['folder']
        candidates=[p.parents[len(Path(meta['bin']).parts)]
                    for p in base.rglob(meta['exe'])
                    if p.parent.as_posix().endswith('/'+meta['bin'])]
        if len(candidates)!=1:
            raise ValueError(f'Found {len(candidates)} original {game} folders; pass --{game}-source PATH.')
        selected=candidates[0]
    selected=selected.expanduser().resolve()
    if not (selected/'_data').is_dir() or not (selected/meta['bin']/meta['exe']).is_file():
        raise ValueError(f'Not a complete game folder: {selected}')
    return selected

with tempfile.TemporaryDirectory(prefix='echo-release-test-') as td:
    temp=Path(td)
    with zipfile.ZipFile(ROOT/'dist/lone-echo-openxr-0.1.0.zip') as z:z.extractall(temp)
    package=temp/'lone-echo-openxr-0.1.0'
    sys.path.insert(0,str(package))
    spec=importlib.util.spec_from_file_location('extracted_setup',package/'echo_setup.py')
    setup=importlib.util.module_from_spec(spec);spec.loader.exec_module(setup)
    import steam_shortcuts as steam
    fake_steam=temp/'Steam';config=fake_steam/'userdata/123/config';config.mkdir(parents=True)
    prior,identity=steam.add(b'','Other game','"/other"','"/"','')
    (config/'shortcuts.vdf').write_bytes(prior)
    setup.steam_root=lambda:fake_steam
    proton=fake_steam/'compatibilitytools.d/GE-Proton';proton.mkdir(parents=True)
    (proton/'proton').write_text('# test fixture; never executed\n')
    os.environ['XDG_DATA_HOME']=str(temp/'xdg')
    originals={game:source_folder(game,meta) for game,meta in setup.GAMES.items()}
    results=[]
    for game,source,layout in [(g,s,l) for g,s in originals.items() for l in ('bundled','custom')]:
        meta=setup.GAMES[game]
        root=package/'games'/meta['folder'] if layout=='bundled' else temp/'Moved library with spaces'/meta['name']
        bindir=root/meta['bin'];bindir.mkdir(parents=True)
        (root/'_data').mkdir()
        original={}
        for patch in meta['patches']:
            data=(source/meta['bin']/patch['file']).read_bytes();original[patch['file']]=data
            (bindir/patch['file']).write_bytes(data)
        # A pre-existing compatibility file must be backed up and restored too.
        old_shim=b'existing third-party shim';(bindir/'LibOVRRT64_1.dll').write_bytes(old_shim)
        (root/'save.dat').write_bytes(b'player progress')
        args=SimpleNamespace(game=game,game_dir=root if layout=='custom' else None,runtime='auto',proton=str(proton),no_shortcuts=False,steam_user=None)
        setup.install(args)
        state=setup.load(game)
        assert Path(state['game_dir'])==root
        assert len(state['files'])==7 and state['complete'] and state.get('steam')
        shortcut=temp/'xdg/applications'/f'lone-echo-openxr-{game}.desktop'
        assert 'X-WiVRn-VR;' in shortcut.read_text()
        icon=shortcut.with_suffix('.png')
        assert icon.read_bytes()==(package/'icons'/f'{game}.png').read_bytes()
        assert f'Icon={icon}\n' in shortcut.read_text()
        entries=steam.shortcut_list((config/'shortcuts.vdf').read_bytes())[1]
        assert any(steam.field(item,'icon')==str(icon) for _,_,item in entries)
        args.check=True;args.diagnostics=False;args.timeout=None;setup.launch(args)
        later_title='Later '+game+' '+layout
        later,identity=steam.add((config/'shortcuts.vdf').read_bytes(),later_title,'"/later-'+game+'-'+layout+'"','"/"','')
        (config/'shortcuts.vdf').write_bytes(later)
        setup.uninstall(args)
        for name,data in original.items():assert (bindir/name).read_bytes()==data
        assert (bindir/'LibOVRRT64_1.dll').read_bytes()==old_shim
        assert not (bindir/'LibOVRPlatform64_1.dll').exists()
        assert not (bindir/'openxr_loader.dll').exists()
        assert not shortcut.exists()
        assert not icon.exists()
        assert (root/'save.dat').read_bytes()==b'player progress'
        titles=[steam.field(item,'AppName') for _,_,item in steam.shortcut_list((config/'shortcuts.vdf').read_bytes())[1]]
        assert 'Other game' in titles and later_title in titles
        assert meta['name']+' — Steam' not in titles
        for name,data in original.items():assert (source/meta['bin']/name).read_bytes()==data
        results.append({'game':game,'source':str(source),'layout':layout,'install':'pass','uninstall':'pass','originals_restored':True,'saves_preserved':True,'unrelated_steam_shortcuts_preserved':True})
    (ROOT/'reports').mkdir(exist_ok=True)
    (ROOT/'reports/release-installer-validation.json').write_text(json.dumps(results,indent=2)+'\n')
    print('Extracted release: both games install/uninstall in bundled and custom folders; originals and saves preserved.')
