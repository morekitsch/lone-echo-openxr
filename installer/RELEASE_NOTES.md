# 0.1.0 preview

OpenXR compatibility runtime and command-line installer for your own copies
of Lone Echo I and Lone Echo II. Windows includes a private Python runtime;
double-click `setup.exe` for a text menu. Linux requires Python 3.10+.
Follow [the setup and game-copy instructions](README.md).

## Included

- Three runtime DLLs shared by both games, with D3D11 support for Lone Echo I
  and D3D12 support for Lone Echo II.
- Linux installer with GE-Proton discovery, separate game prefixes, WiVRn
  discovery entries and optional Steam VR library registration.
- Windows `setup.exe`, bundled CPython 3.13.16, and Desktop/Start Menu shortcuts.
- Per-game OpenXR runtime selection: WiVRn, SteamVR, VDXR or the system default.
- Numbered icons and distinct Direct/Steam labels on Linux. WiVRn's Flatpak
  can read installed icons without additional filesystem permissions.
- Uninstall that restores backed-up binaries and removes owned shortcuts
  and icons. Games, saves and Proton prefixes are preserved.
- Runtime source, dependency lockfile, build instructions and license notices.
- Empty game folders. No game files, saves, account data or local logs.

## Validation

| Configuration | Result |
| --- | --- |
| LE1, Linux, GE-Proton, WiVRn, Quest 3 | User confirmed current preview: wide VR view on repeat launches, menu input and correct desktop picture |
| LE2, same stack, earlier runtime | User confirmed gameplay; right-hand disk throwing remained inconsistent |
| LE2, same stack, this shared runtime | User reports normal VR output and gameplay; desktop output black or flickering |
| Native Windows, LE1, VDXR | User confirmed installer and game work after the D3D11 texture-view correction |
| Native Windows, LE2, VDXR | User confirmed startup, gameplay and placement; reported the latest resolution/statistics build worked great after the requested intro-blackout retest |
| Native Windows, SteamVR | Untested |
| D3D11 texture views under Wine | Default and explicit HDR formats, array/MSAA resources, and ordinary-resource isolation tested without a headset |
| D3D12 graphics under Wine | Default color views, D24S8 depth creation/clear, three frame handoffs and color pixel readback passed without a headset |
| Windows installer EXE under Wine | Private Python, text menu, argument quoting, install/uninstall and launch preflight passed for both games; native Windows shortcuts still need validation |
| Installer and uninstaller on Linux | Extracted package tested with both supported binary builds; original files, saves and unrelated Steam entries preserved |

## Fixed

- After the requested intro-blackout retest, the user reported that the latest
  Windows/VDXR LE2 build worked great. Exact blackout duration was not measured;
  the separate effects of the resolution and statistics fixes are unknown.

- Corrected recommended render sizes: the bridge multiplied the full OpenXR
  image size by the field of view a second time. Default FOV and density now
  use the runtime's recommended size, with matching pixels-per-tangent values.
  This applies to both games and both operating systems. The user confirmed
  the updated LE2 build works on Windows/VDXR and LE1 works on Linux/WiVRn;
  broader gameplay retests remain.
- Performance-statistics output is fully initialized for the negotiated CAPI
  version. It reports no timing samples and a neutral adaptive GPU scale.
- Optional diagnostics include elapsed timestamps, frame numbers, completed
  frame calls, and OpenXR predicted display time/period and render visibility.

- User confirmed the LE2 player-placement correction works on Windows/VDXR.
  Eye/floor tracking and recenter requests now use consistent reference spaces.

- LE2 now starts and reaches gameplay on Windows/VDXR after the D3D12 texture
  and depth-resource fixes; the user confirmed this startup improvement.

- LE1 rendered black on Windows/VDXR because its D3D11 texture views needed
  explicit compatible formats. The bridge now handles default views and the
  R11G11B10F to RGBA16F fallback. The user confirmed the updated build works.

## Known issues

- Broader gameplay regression tests are still needed after the shared runtime
  updates. Linux LE1 wide headset output and PC picture were confirmed on
  repeat launches.
- A first launch without saved headset data can use a generic, narrow field
  of view. The runtime saves the real headset values; restarting corrected
  the reported LE1 square VR view. A first-launch fix remains outstanding.
- LE2 PC output can be black or flickering while VR works. Windowed mode showed
  background color without the menu and flickered after Start. Windows PC
  flicker was also reported for LE1 and LE2. Desktop presentation remains
  unresolved; available LE2 traces do not call Oculus mirror-texture APIs.
- The earlier LE2 build's right-hand disk throws were inconsistent. The shared
  runtime uses native OpenXR velocities, but an improvement is not confirmed.
- Other game builds are rejected by checksum before patching. The accepted
  binary hashes are listed in `games.json`.
- Broader gameplay coverage and additional OpenXR runtimes need testing before
  this can be considered a stable cross-platform release.
- Online platform services are not implemented.

No headset tracking configuration is changed. Controller motion and velocity
come from standard OpenXR interfaces, with no added smoothing. Older Oculus
API clients receive compatible virtual sensor status to avoid a false
external-sensor warning on headsets such as Quest 3.
