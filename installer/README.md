# Lone Echo OpenXR — preview

Play your own copies of Lone Echo and Lone Echo II through OpenXR.
This package contains compatibility software and empty game folders. It contains no game content.

**Validation:** Both games have working gameplay reports on Quest 3 through Linux/WiVRn and Windows/VDXR. LE2's latest runtime has positive visual feedback on both platforms: VDXR supplied working hidden-area masks, while WiVRn correctly used the fallback for empty masks. LE1 previously passed on both platforms but still needs a headset retest with this shared runtime update. SteamVR remains untested. See [release notes](RELEASE_NOTES.md) for test status and known issues.

## Requirements

- 64-bit Windows 10/11, or Linux with a working Vulkan driver and GE-Proton with OpenXR support.
- Windows: no Python installation needed; `setup.exe` uses the included private runtime. Linux: Python 3.10 or newer. No pip packages are required.
- A connected headset and a working OpenXR runtime: WiVRn on Linux, or SteamVR/Virtual Desktop VDXR on Windows. SteamVR on Linux can also be selected but is untested.
- Your own supported game installation. The installer checks the original executable and platform DLL against known SHA-256 hashes. Other builds stop before modification.

Extract this package into its permanent, writable location. Keep its `state` folder: it contains the original binary backups needed to uninstall. Uninstall before moving the package or game folders.

## Copy the games

Copy the **contents of the game installation folder**, including its data directories. Copying only the executable is insufficient. Do not copy the outer installer/download folder. The game folder's name and its parent folders do not matter: select the folder that directly contains `_data` and `bin`.

| Game | Folder to copy from | Destination in this package | Expected executable |
| --- | --- | --- | --- |
| Lone Echo | The folder containing `_data`, `bin`, `header`, `lvllist`, and `sourcedb` (for example `Lone.Echo` or `ready-at-dawn-lone-echo`) | `games/lone-echo/` | `games/lone-echo/bin/win7/LoneEcho.exe` |
| Lone Echo II | The game folder containing `_data` and `bin` (often named `ready-at-dawn-lone-echo-2`) | `games/lone-echo-2/` | `games/lone-echo-2/bin/win10/loneecho2.exe` |

Alternatively, use `--game-dir "/path/to/your/game"` to install into an existing copy. The installer backs up the two binaries it patches and any files it replaces. Original game installations outside that chosen folder are untouched.

## Linux setup

Close Steam first if you want its VR library entries added automatically. Connect the headset in WiVRn, then run from this package directory:

```sh
./setup.sh install le1 --runtime wivrn
./setup.sh install le2 --runtime wivrn
```

The installer finds GE-Proton in Steam's `compatibilitytools.d` directory. To specify another installation:

```sh
./setup.sh install le1 --runtime wivrn --proton "/path/to/GE-Proton"
```

Each installed game gets an application-menu entry named **Lone Echo I — Direct** or **Lone Echo II — Direct**, with its own numbered icon. Steam entries are labeled **— Steam** so the launch route is clear. The desktop entry has the `X-WiVRn-VR` category, so WiVRn can list it. Icons are installed beside the desktop entries so WiVRn's Flatpak can read them; uninstall removes them. When Steam is installed and closed, a VR shortcut is also added to its local profile. Restart Steam to refresh the list. With multiple Steam profiles, pass `--steam-user ID`. If registration was deferred because Steam was open, close it and run:

```sh
./setup.sh register-steam le1
./setup.sh register-steam le2
```

Do not enable Steam's “Force the use of a specific Steam Play compatibility tool” for these shortcuts: the launcher runs Proton itself. Each game has a separate prefix under `userdata/<game>/proton`.

Flatpak Steam may need host access to this package, Python, and the selected Proton installation to run the shortcut. The native desktop/WiVRn entry launches outside Steam's sandbox. No Flatpak permissions are changed by this installer.

## Windows setup

Extract the complete ZIP, then double-click **setup.exe** for the text menu.
Windows does not need Python installed. Keep the included `windows-python`
folder beside `setup.exe`; it is also used by the game shortcuts.
The bundled runtime does not change PATH, file associations or the system
Python installation. It is based on Python's [official embeddable distribution](https://docs.python.org/3.13/using/windows.html#the-embeddable-package).

Connect the headset through Virtual Desktop or SteamVR. You can also open a
terminal in this package directory and run:

```bat
.\setup.exe install le1 --runtime vdxr
.\setup.exe install le2 --runtime vdxr
```

For SteamVR, substitute `--runtime steamvr`. For the currently selected OpenXR runtime, use `--runtime auto` (the default). Windows receives Desktop and Start Menu shortcuts. It does not use Proton or add Steam library entries.

VDXR is the OpenXR runtime supplied with the Virtual Desktop Streamer. Keep the Streamer running and connect from the headset. For SteamVR, start SteamVR and connect the headset before launching. This software uses OpenXR with either choice.

## Replacing a preview build

Close the game. Use the existing installer to uninstall its compatibility
files first (`.\setup.exe uninstall le1`, or `./setup.sh uninstall le1` on
Linux). This preserves the game and saves. Extract the new ZIP into the same
package location, replacing package files, then install again with the same
game folder and runtime. For an external game folder, supply `--game-dir` again.
Do not replace DLLs directly in an installed game: the launcher checks them
against its installation record. Repeat for each game you want to update.

If you manually installed a test DLL, restore the DLL you backed up before
uninstalling. The uninstaller preserves changed files and reports them instead
of deleting them. Keep `state` and its backups until uninstall succeeds.

## Launch and change runtime

Use the installed shortcuts, or:

```sh
python echo_setup.py launch le1
python echo_setup.py launch le2
python echo_setup.py runtime le1 steamvr
python echo_setup.py runtime le2 auto
```

On Windows, replace `python echo_setup.py` with `.\setup.exe` in these commands.
`setup.cmd` is also supported and uses the bundled executable when present.

Runtime choices are `auto`, `wivrn`, `steamvr`, `vdxr`, or an absolute runtime JSON path. Named runtimes must already be installed. A specific choice sets `XR_RUNTIME_JSON` for that game's process only. `auto` follows the OpenXR loader's normal runtime selection, including an inherited override. The installer does not modify the system OpenXR registry or Linux runtime symlink.

## Uninstall and cleanup

Exit the game. Close Steam if a Steam shortcut was registered, then run:

```sh
python echo_setup.py uninstall le1
python echo_setup.py uninstall le2
```

Windows users can select Uninstall in the `setup.exe` menu, or run
`.\setup.exe uninstall le1` / `.\setup.exe uninstall le2`.

Uninstall restores original patched binaries and replaced files, removes generated compatibility DLLs and shortcuts, and removes the install record and backups after successful restoration. Steam entries added by other software are preserved. Files edited after installation are retained with their backups for manual review; the command reports their paths instead of overwriting them.

**Games and saves are preserved.** Linux saves may live inside `userdata/<game>/proton`; keep this directory if you want your progress. Logs are in `userdata/<game>/logs`. After uninstalling all games, you may remove the package folder after copying out any games and saves you want to retain. Windows saves remain in the game's normal user save location.

## Diagnostics and known limits

```sh
python echo_setup.py launch le1 --check
python echo_setup.py launch le1 --diagnostics
python echo_setup.py status le1
```

`--check` shows launch settings; it does not prove that the headset/runtime works. Launch output is saved to `userdata/<game>/logs/launch.log`. With diagnostics enabled, the runtime writes `libovr-openxr.log` beside the game executable. Normal launches leave per-frame diagnostics disabled.

- First launch starts with nominal field-of-view values. Once a session produces valid views, the shim writes `libovr-openxr-hmd-cache.toml` beside the executable. Restart after the first successful session if the image appears narrow. Delete this cache when changing headsets.
- Controller aim poses and velocities come from standard OpenXR APIs. No WiVRn-specific motion API, smoothing, or asymmetric throw correction is used.
- Lone Echo II's FOV-stencil calls now return valid native OpenXR masks or a defined unsupported result. The PC picture is confirmed stable on Linux/WiVRn, and the latest Windows/VDXR LE2 test looked good with working hidden-area masks. Performance gains have not been measured. The installer does not force display or fullscreen changes. Lone Echo I's desktop view is confirmed working on Linux/WiVRn; its previous Windows flicker needs a retest with this build.
- Platform services are local substitutes for single-player initialization. Online services are not implemented.
- This compatibility runtime still contains incomplete CAPI functions. A successful startup is not a claim that every game feature or OpenXR runtime is supported.

## Architecture and source

Game → `LibOVRRT64_1.dll` → standard `openxr_loader.dll` → selected OpenXR runtime.
`LibOVRPlatform64_1.dll` forwards platform exports into the same runtime DLL so both interfaces share state. Linux additionally uses Proton to run the Windows game.

`source/runtime` contains the runtime source and locked Rust dependencies. `licenses/` contains third-party license notices. See `source/BUILDING.md` for build instructions.

Runtime selection follows the [Khronos OpenXR loader specification](https://registry.khronos.org/OpenXR/specs/1.1/loader.html). WiVRn registration follows its [documented application discovery](https://github.com/WiVRn/WiVRn#start-an-application).
