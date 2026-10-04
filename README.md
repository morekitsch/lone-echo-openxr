# Lone Echo OpenXR

An experimental OpenXR compatibility runtime and cross-platform installer for
Lone Echo I and Lone Echo II. Use your own copies of the games.

The runtime translates LibOVR calls to OpenXR, using D3D11 for Lone Echo I and
D3D12 for Lone Echo II. Linux runs the Windows games through GE-Proton;
Windows uses the selected OpenXR runtime directly.

## Install and play

See the [installer and game-copy instructions](installer/README.md).
Release packages include a command-line installer, original numbered icons,
and empty folders for the games. Windows packages include a private Python
runtime and `setup.exe`; Linux uses `setup.sh` with Python 3.10 or newer.

The installer creates game shortcuts and, on Linux, WiVRn discovery entries
and optional Steam VR shortcuts. Uninstall restores backed-up binaries and
removes owned shortcuts while preserving games, saves and Proton prefixes.

This source repository contains no game content, game executables, saves,
private logs or generated release binaries. Build a package using
[BUILDING.md](BUILDING.md). A Git checkout alone is not a ready-to-run installer.

## Current status

This is a preview, with testing on Quest 3:

| Configuration | User test result |
| --- | --- |
| Linux / WiVRn / LE1 | VR view, controllers, gameplay and PC picture working |
| Linux / WiVRn / LE2 | VR gameplay working; PC picture black or flickering |
| Windows / VDXR / LE1 | VR gameplay working; PC flicker reported |
| Windows / VDXR / LE2 | VR gameplay and placement working; updated intro sequence retest successful; PC flicker reported |
| SteamVR | Available as a runtime selection, not yet tested |

On the first launch without saved headset data, the generic startup field of
view can appear narrow. The runtime saves the headset's actual field of view;
relaunching corrected the reported LE1 view. This startup limitation still
needs a code fix. Keep the generated headset cache when updating.

LE2 desktop presentation remains under investigation. Switching its PC window
out of fullscreen did not fix it. Earlier right-hand disk throws were also
inconsistent; an improvement has not been confirmed. Online platform services
are not implemented. See [release notes](installer/RELEASE_NOTES.md).

## Source layout

- `runtime/`: shared Rust LibOVR-to-OpenXR implementation and runtime tests.
- `installer/`: installer, Windows launcher source, game-build checksums,
  patch definitions, icons and user documentation.
- `tools/`: build, packaging and validation tools.
- `tests/`: installer and uninstall tests with temporary fixtures.
- `compat/`: earlier C prototype adapters, retained for reference. The current
  release uses the shared Rust runtime. `tools/prepare_le2.py` and
  `tools/launch_le2.py` belong to that earlier prototype.

Run the tests that do not require games or a headset:

```bash
python3 -m unittest discover -s tests -v
CARGO_HOME="$PWD/working/cargo" cargo +stable test --locked --manifest-path runtime/Cargo.toml
```

Graphics tests and checks against your own game files are described in
[BUILDING.md](BUILDING.md). Generated payloads, dependencies, local game folders,
installer state and diagnostic reports are excluded from Git.

## License and attribution

Local installer code and compatibility changes use the
[Apache License 2.0](LICENSE). The runtime derives from
[TesseractCat/libovr-openxr-rs](https://github.com/TesseractCat/libovr-openxr-rs);
see [upstream attribution](runtime/UPSTREAM.md) and [NOTICE](runtime/NOTICE).
Third-party components retain their licenses, included when packaging releases.
