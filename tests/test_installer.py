import importlib.util
import json
from pathlib import Path
import struct
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'installer'))
import echo_setup as setup
import steam_shortcuts as steam


class InstallerTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory()
        self.root=Path(self.temp.name)
        self.patch=patch.object(setup,'STATE',self.root/'state')
        self.patch.start()
        self.state={'files':[],'complete':True}
    def tearDown(self):
        self.patch.stop()
        self.temp.cleanup()
    def uninstall(self):
        from argparse import Namespace
        setup.uninstall(Namespace(game='le1'))
    def test_restore_original_remove_generated_preserve_save(self):
        original=self.root/'game.dll'; original.write_bytes(b'original')
        generated=self.root/'new.dll'
        save=self.root/'save.dat'; save.write_bytes(b'progress')
        setup.put('le1',self.state,original,b'patched')
        setup.put('le1',self.state,generated,b'shim')
        self.uninstall()
        self.assertEqual(original.read_bytes(),b'original')
        self.assertFalse(generated.exists())
        self.assertEqual(save.read_bytes(),b'progress')
        self.assertFalse((setup.STATE/'le1').exists())
    def test_changed_file_is_preserved_with_backup(self):
        p=self.root/'game.dll';p.write_bytes(b'original')
        setup.put('le1',self.state,p,b'patched')
        p.write_bytes(b'newer game update')
        with self.assertRaises(ValueError):self.uninstall()
        self.assertEqual(p.read_bytes(),b'newer game update')
        state=setup.load('le1')
        self.assertEqual(Path(state['files'][0]['backup']).read_bytes(),b'original')
    def test_interrupted_install_recovers_from_journal(self):
        p=self.root/'game.dll';p.write_bytes(b'original')
        real=setup.atomic
        def fail_target(path,data):
            if path==p: raise OSError('simulated write failure')
            return real(path,data)
        with patch.object(setup,'atomic',side_effect=fail_target):
            with self.assertRaises(OSError):setup.put('le1',self.state,p,b'patched')
        self.uninstall()
        self.assertEqual(p.read_bytes(),b'original')
    def test_missing_original_is_restored(self):
        p=self.root/'game.dll';p.write_bytes(b'original')
        setup.put('le1',self.state,p,b'patched');p.unlink()
        self.uninstall()
        self.assertEqual(p.read_bytes(),b'original')
    def test_symlink_not_overwritten(self):
        p=self.root/'actual';p.write_bytes(b'keep')
        link=self.root/'link';link.symlink_to(p)
        with self.assertRaises(ValueError):setup.put('le1',self.state,link,b'bad')
        self.assertEqual(p.read_bytes(),b'keep')
    def test_unknown_game_build_rejected(self):
        with self.assertRaises(ValueError):setup.patch(b'unknown',setup.GAMES['le1']['patches'][0])
    def test_steam_preserves_unrelated_and_later_added_entries(self):
        data,first=steam.add(b'','Existing game','"/x"','"/"','a')
        data,ours=steam.add(data,'Lone Echo','"/python"','"/"','launch le1')
        data,third=steam.add(data,'Added later','"/z"','"/"','b')
        result=steam.remove(data,ours)
        entries=steam.shortcut_list(result)[1]
        self.assertEqual([steam.field(e[2],'AppName') for e in entries],['Existing game','Added later'])
        self.assertEqual(steam.encode(steam.decode(result)),result)
    def test_steam_label_update_keeps_id_and_uninstall_identity(self):
        data,identity=steam.add(b'', 'Lone Echo', '"/python"', '"/"', 'launch le1')
        updated=steam.update_presentation(data,identity,'Lone Echo I — Steam','/icons/le1.png')
        item=steam.shortcut_list(updated)[1][0][2]
        self.assertEqual(steam.field(item,'AppName'),'Lone Echo I — Steam')
        self.assertEqual(steam.field(item,'icon'),'/icons/le1.png')
        self.assertEqual(steam.field(item,'appid'),struct.pack('<I',identity['appid']))
        self.assertEqual(steam.shortcut_list(steam.remove(updated,identity))[1],[])

    def test_truncated_steam_file_rejected(self):
        with self.assertRaises(ValueError):steam.decode(b'\0shortcuts\0\0')
    def test_steam_edited_shortcut_preserved(self):
        data,ours=steam.add(b'','Lone Echo','"/python"','"/"','launch le1')
        data=data.replace(b'launch le1',b'launch le2')
        with self.assertRaises(ValueError):steam.remove(data,ours)
    def test_desktop_arguments_escape_fields_and_metacharacters(self):
        self.assertEqual(setup.desktop_quote('/a %f/$x"'), '"/a %%f/\\$x\\""')

    def test_menu_install_uses_the_same_cli_and_returns_to_menu(self):
        with patch('builtins.input',side_effect=['1','1','"/Games/Lone Echo"','auto','0']), patch.object(setup,'main') as main:
            self.assertEqual(setup.interactive_menu(),0)
        main.assert_called_once_with(['install','le1','--game-dir','/Games/Lone Echo','--runtime','auto'])

    def test_menu_stays_open_after_a_command_error(self):
        with patch('builtins.input',side_effect=['2','2','0']), patch.object(setup,'main',side_effect=SystemExit(1)) as main:
            self.assertEqual(setup.interactive_menu(),0)
        main.assert_called_once_with(['uninstall','le2'])

if __name__=='__main__': unittest.main()
