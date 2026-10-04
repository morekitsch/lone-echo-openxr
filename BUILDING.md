# Building the preview

The runtime is derived from libovr-openxr-rs revision `49c48292510b771e68ba8a85b45c7b2c35c0654b`.
Changes are in `runtime/src`; all dependencies are pinned by `runtime/Cargo.lock`.

The Linux build used Rust stable 1.93.1, the `x86_64-pc-windows-gnullvm` target,
and LLVM-MinGW `20260922-ucrt-ubuntu-22.04-x86_64` from
https://github.com/mstorsjo/llvm-mingw/releases/tag/20260922.

1. Install Rust and `rustup target add x86_64-pc-windows-gnullvm`.
2. Download and extract the LLVM-MinGW toolchain. Set `LLVM_MINGW_DIR` to its directory.
3. Install Clang, lld-link, Python 3.11+, and Python's `pefile` package for build-time PE validation.
4. Download Khronos OpenXR SDK release `1.1.63`'s `openxr_loader_windows-1.1.63.zip` to `working/` from https://github.com/KhronosGroup/OpenXR-SDK-Source/releases/tag/release-1.1.63.
5. Run `bash tools/build_runtime.sh`. This builds with `+crt-static` so the DLL does not require a separate compiler unwind DLL.
6. Run `python -m unittest discover -s tests -v` and `CARGO_HOME="$PWD/working/cargo" cargo +stable test --manifest-path runtime/Cargo.toml`.
7. Download [CPython 3.13.16 for embedding, Windows x64](https://www.python.org/ftp/python/3.13.16/python-3.13.16-embed-amd64.zip) to `working/`. SHA-256: `97dae5274cc54867065e8d5a3226e48c35017ed332a0fdb0e27d5b5821961297`. Run `python tools/build_windows_installer.py`; it verifies the download and compiles `setup.exe` with LLVM-MinGW. The private Python directory keeps isolated imports and includes its license.
8. Run `python tools/build_release.py`. The builder verifies the three game DLLs and every Windows installer/runtime file against their manifests. It packages only allowlisted files and never walks the game directories.

When starting from a release archive, copy `source/runtime`, `source/tools`, and `source/tests` to a new build workspace. Copy the top-level installer files, `payload`, `icons`, and `licenses` into `installer/` in that workspace. Copy `source/windows_launcher.c` into `installer/` and this document to the workspace root. Create `working/` for downloaded build inputs. No game files are needed to compile or package the runtime.

The game patches are guarded by complete original-file hashes in `installer/games.json`.
Patch validation against a different game build must be done explicitly; do not remove the hash checks.

For timing investigations, `LIBOVR_OPENXR_LOG=buffered` keeps up to 32 MiB of
recent complete trace records in memory and writes them at `ovr_Shutdown`.
Quit the game normally to save the trace; a crash can lose it. Overflow drops
the oldest records and reports the count. This avoids per-call file writes,
but formatting and locking still affect timing. The normal logging modes
remain disabled by default, or synchronous with `LIBOVR_OPENXR_LOG=1`.
Do not combine buffered mode with the installer's `--diagnostics` flag, which
selects synchronous logging.
For stencil queries, use `LIBOVR_OPENXR_LOG=stencil`. It records only stencil
diagnostics and startup/shutdown markers directly, retaining startup evidence
through long gameplay runs. It does not measure rendering performance.
Native Windows builds can use the standard MSVC Rust target, but that build path has not been tested here. The user confirmed this cross-compiled build works for LE1 on Windows with VDXR.

The optional Windows graphics test is
`cargo test --lib d3d11_views::tests::real_d3d11_swapchain_views -- --ignored --nocapture`.
It requires D3D11 but no headset. It checks typed views, negotiated HDR formats,
pixel readback, array/MSAA textures and isolation of ordinary resources. A
cross-compiled Windows test executable can also be run with Wine.

The corresponding D3D12 check is
`cargo test --lib d3d12_views::tests::real_d3d12_color_depth_and_handoff -- --ignored --nocapture`.
It exercises default color views, D24S8 depth creation/clear, repeated image
state transitions and pixel readback without a headset.

Optional package checks with supported game binaries under `originals/`:
`python tools/verify_release.py` checks Linux install/uninstall in default and
custom folders. `python tools/verify_windows_installer.py` runs `setup.exe`
under Wine with a dedicated prefix, including spaces/Unicode in paths and
invalid system Python environment settings. Its game fixtures disable shortcut
creation. The user separately confirmed the Windows installer and LE1/VDXR work; this automated check does not validate PowerShell shortcuts or headset output.
