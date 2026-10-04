"""Steam's binary KeyValues shortcut file, preserving unknown supported fields.

Edits require Steam to be closed. Uninstall removes only our exact shortcut.
"""
import os
from pathlib import Path
import struct
import zlib


def decode(data):
    cursor = 0
    def take(count):
        nonlocal cursor
        if cursor + count > len(data):
            raise ValueError('Truncated Steam shortcuts file')
        value = data[cursor:cursor+count]
        cursor += count
        return value
    def string():
        nonlocal cursor
        end = data.find(b'\0',cursor)
        if end < 0:
            raise ValueError('Unterminated Steam shortcut string')
        value = data[cursor:end].decode('utf-8',errors='surrogateescape')
        cursor = end + 1
        return value
    def obj():
        fields = []
        while True:
            kind = take(1)[0]
            if kind == 8:
                return fields
            key = string()
            if kind == 0: value = obj()
            elif kind == 1: value = string()
            elif kind in (2,3,4): value = take(4)
            elif kind == 7: value = take(8)
            elif kind == 6: value = take(4)
            else: raise ValueError(f'Unsupported Steam binary type {kind}; shortcuts left untouched')
            fields.append((kind,key,value))
    result = obj()
    if cursor != len(data):
        raise ValueError('Unexpected trailing Steam shortcut data')
    return result


def encode(fields):
    out = bytearray()
    for kind,key,value in fields:
        out += bytes([kind]) + key.encode('utf-8',errors='surrogateescape') + b'\0'
        if kind == 0: out += encode(value)
        elif kind == 1: out += value.encode('utf-8',errors='surrogateescape') + b'\0'
        else: out += value
    return bytes(out) + b'\x08'


def field(fields,key):
    return next((v for _,k,v in fields if k.lower()==key.lower()),None)


def check_closed():
    if os.name == 'nt':
        import subprocess
        running = 'steam.exe' in subprocess.check_output(['tasklist','/FI','IMAGENAME eq steam.exe'],text=True).lower()
    else:
        running = False
        for p in Path('/proc').glob('[0-9]*/comm'):
            try:
                if p.read_text().strip()=='steam': running=True
            except OSError: pass
    if running:
        raise ValueError('Exit Steam before changing its shortcuts, then run register-steam again.')


def shortcut_list(data):
    root = decode(data) if data else [(0,'shortcuts',[])]
    entries = field(root,'shortcuts')
    if not isinstance(entries,list):
        raise ValueError('Steam shortcuts root is invalid')
    return root,entries


def add(data,title,exe,start,options,icon=""):
    root,entries = shortcut_list(data)
    appid = zlib.crc32((exe+title).encode()) | 0x80000000
    identity = {'appid':appid,'exe':exe,'options':options}
    for _,_,item in entries:
        if field(item,'appid') == struct.pack('<I',appid):
            raise ValueError('Steam shortcut ID already exists; refusing to replace it')
    strings = {'AppName':title,'Exe':exe,'StartDir':start,'icon':icon,'ShortcutPath':'','LaunchOptions':options}
    integers = {'appid':appid,'IsHidden':0,'AllowDesktopConfig':1,'AllowOverlay':1,'OpenVR':1,'Devkit':0,'DevkitOverrideAppID':0,'LastPlayTime':0}
    item = [(1,k,v) for k,v in strings.items()] + [(2,k,struct.pack('<I',v)) for k,v in integers.items()] + [(0,'tags',[(1,'0','VR')])]
    used={key for _,key,_ in entries}
    key=next(str(i) for i in range(len(entries)+1) if str(i) not in used)
    entries.append((0,key,item))
    return encode(root),identity


def remove(data,identity):
    root,entries = shortcut_list(data)
    kept=[]
    for entry in entries:
        item=entry[2]
        matches=field(item,'appid')==struct.pack('<I',identity['appid'])
        if matches:
            if field(item,'exe')!=identity['exe'] or field(item,'LaunchOptions')!=identity['options']:
                raise ValueError('The installed Steam shortcut was edited; preserved it for manual removal.')
        else: kept.append(entry)
    entries[:]=[(kind,str(i),item) for i,(kind,_,item) in enumerate(kept)]
    return encode(root)


def update_presentation(data,identity,title,icon):
    root,entries=shortcut_list(data)
    for _,_,item in entries:
        if field(item,'appid')==struct.pack('<I',identity['appid']):
            if field(item,'exe')!=identity['exe'] or field(item,'LaunchOptions')!=identity['options']:
                raise ValueError('Steam shortcut command changed; preserved it for review.')
            for key,value in [('AppName',title),('icon',icon)]:
                found=False
                for index,(kind,name,previous) in enumerate(item):
                    if name.lower()==key.lower():
                        item[index]=(1,name,value);found=True;break
                if not found:item.append((1,key,value))
            return encode(root)
    raise ValueError('Installed Steam shortcut is missing')
