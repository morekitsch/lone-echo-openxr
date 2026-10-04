#!/usr/bin/env python3
"""Launch the prepared Lone Echo II copy using GE-Proton and WiVRn.

Settings, saves, and logs stay in this workspace. Use --check for preflight
and --timeout SECONDS for a bounded startup diagnostic.
"""
import argparse
from datetime import datetime
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
GAME = ROOT / "working/lone-echo-2"
BIN = GAME / "bin/win10"
STEAM = Path.home() / ".local/share/Steam"
PROTON = STEAM / "compatibilitytools.d/Proton-GE Latest"
RUNTIME = Path("/var/lib/flatpak/app/io.github.wivrn.wivrn/x86_64/stable/active/files/share/openxr/1/openxr_wivrn.json")
PREFIX = ROOT / "working/proton-le2"
LOADER_HASH = "66946ea6e9cc2649e299ec412521c5eafe56c635d05055097fe4db81e11932df"


def preflight(proton, runtime):
    for path in (proton / "proton", runtime, BIN / "openxr_loader.dll"):
        if not path.is_file():
            raise ValueError(f"Required file is missing: {path}")
    description = json.loads(runtime.read_text())["runtime"]
    library = (runtime.parent / description["library_path"]).resolve()
    if not library.is_file():
        raise ValueError(f"OpenXR runtime library is missing: {library}")
    manifest = json.loads((ROOT / "reports/le2-patch-manifest.json").read_text())
    hashes = dict(manifest["compatibility_files"])
    hashes.setdefault("openxr_loader.dll", LOADER_HASH)
    hashes.update({p["file"]: p["output_sha256"] for p in manifest["patches"]})
    for name, expected in hashes.items():
        if hashlib.sha256((BIN / name).read_bytes()).hexdigest() != expected:
            raise ValueError(f"Prepared file changed: {name}; inspect before launching")
    print(f"Game: {BIN / 'loneecho2.exe'}")
    print(f"Proton: {proton}")
    print(f"OpenXR: {runtime}")
    print(f"Prefix: {PREFIX}")


def launch(proton, runtime, timeout, controller_pose=None, velocity_mode=None):
    run = ROOT / "reports/runs" / datetime.now().strftime("%Y%m%d-%H%M%S-%f")
    run.mkdir(parents=True)
    PREFIX.mkdir(exist_ok=True)
    cache = ROOT / "working/cache"
    cache.mkdir(exist_ok=True)
    environment = os.environ.copy()
    environment.update({
        "STEAM_COMPAT_CLIENT_INSTALL_PATH": str(STEAM),
        "STEAM_COMPAT_DATA_PATH": str(PREFIX),
        "STEAM_COMPAT_INSTALL_PATH": str(GAME),
        "STEAM_COMPAT_APP_ID": "0", "SteamAppId": "0", "SteamGameId": "0",
        "UMU_ID": "umu-default",
        "XR_RUNTIME_JSON": str(runtime),
        "DXVK_NO_VR": "1", "VR_PATHREG_OVERRIDE": os.devnull,
        "PROTON_LOG": "1", "PROTON_LOG_DIR": str(run),
        "PROTON_CRASH_REPORT_DIR": str(run),
        "DXVK_LOG_PATH": str(run), "VKD3D_DEBUG": "warn",
        "XR_LOADER_DEBUG": "all", "LIBOVR_OPENXR_LOG": "1",
        "WINEDEBUG": "+timestamp,+pid,+tid,+seh,+debugstr,+loaddll,+openxr",
        "WINEDLLOVERRIDES": "libovrrt64_1,libovrrt64_upstream,libovrplatform64_1,openxr_loader,openxr_loader_upstream=n",
        "XDG_CACHE_HOME": str(cache),
        "__GL_SHADER_DISK_CACHE_PATH": str(cache / "nvidia"),
    })
    environment.pop("VR_OVERRIDE", None)
    if controller_pose is not None:
        environment["LE2_CONTROLLER_POSE"] = controller_pose
    if velocity_mode is not None:
        environment["LE2_VELOCITY"] = velocity_mode
    print(f"Controller pose: {environment.get('LE2_CONTROLLER_POSE', 'aim')}", flush=True)
    print(f"Hand velocity: {environment.get('LE2_VELOCITY', 'native')}", flush=True)
    print(f"Logs: {run}", flush=True)
    shim_log = BIN / "libovr-openxr.log"
    shim_start = shim_log.stat().st_size if shim_log.exists() else 0
    timed_out = False
    with (run / "console.log").open("wb") as log:
        process = subprocess.Popen(
            [str(proton / "proton"), "run", str(BIN / "loneecho2.exe")],
            cwd=GAME, env=environment, stdout=log, stderr=subprocess.STDOUT,
            start_new_session=True,
        )
        try:
            code = process.wait(timeout=timeout)
        except (subprocess.TimeoutExpired, KeyboardInterrupt):
            timed_out = True
            # This wineserver command targets only this experiment's prefix.
            server_env = environment | {"WINEPREFIX": str(PREFIX / "pfx")}
            subprocess.run([str(proton / "files/bin/wineserver"), "-k"],
                           env=server_env, stdout=log, stderr=log, timeout=10)
            if process.poll() is None:
                os.killpg(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait()
            code = 124
    if shim_log.exists():
        with shim_log.open("rb") as source:
            source.seek(shim_start)
            (run / "libovr-openxr.log").write_bytes(source.read())
    (run / "result.json").write_text(json.dumps({
        "exit_code": code, "timeout_or_interrupted": timed_out,
        "proton": str(proton), "runtime": str(runtime),
        "controller_pose": environment.get("LE2_CONTROLLER_POSE", "aim"),
        "velocity_mode": environment.get("LE2_VELOCITY", "native"),
    }, indent=2) + "\n")
    print(f"Exit: {code}; logs: {run}")
    return code


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--timeout", type=float)
    parser.add_argument("--proton", type=Path, default=PROTON)
    parser.add_argument("--runtime", type=Path, default=RUNTIME)
    parser.add_argument("--controller-pose", choices=("aim", "grip"),
                        help="aim is the default; grip compares the previous controller binding")
    parser.add_argument("--velocity-mode", choices=("native", "legacy"),
                        help="native is the default; legacy restores upstream velocity estimates")
    args = parser.parse_args()
    if args.timeout is not None and args.timeout <= 0:
        parser.error("--timeout must be positive")
    try:
        preflight(args.proton, args.runtime)
        sys.exit(0 if args.check else launch(args.proton, args.runtime, args.timeout,
                                           args.controller_pose, args.velocity_mode))
    except (OSError, ValueError, KeyError) as error:
        parser.exit(1, f"Launch stopped: {error}\n")
