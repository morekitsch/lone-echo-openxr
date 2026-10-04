#!/usr/bin/env python3
"""Prepare the known Lone Echo II build for an experimental local OpenXR shim.

Only working/lone-echo-2 is changed. Originals are read-only inputs.
Requires Python pefile, clang, lld-link and llvm-dlltool.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import zipfile

import pefile

ROOT = Path(__file__).resolve().parents[1]
ORIGINAL = ROOT / "originals/lone-echo-2/bin/win10"
TARGET = ROOT / "working/lone-echo-2/bin/win10"
SHIM = ROOT / "working/shim-0.5.0/LibOVRRT64_1.dll"
SHIM_HASH = "e707a1eee0a07122d7b744fbce667988798e27557ad0347a78ec1bf7784b0663"
LOADER_HASH = "66946ea6e9cc2649e299ec412521c5eafe56c635d05055097fe4db81e11932df"
LOCAL_EXPORTS = {
    "ovr_Entitlement_GetIsViewerEntitled", "ovr_PopMessage", "ovr_Message_GetType",
    "ovr_Message_GetRequestID", "ovr_Message_IsError", "ovr_FreeMessage",
    "ovr_Voip_GetPCMFloat",
}
LOCAL_VR_EXPORTS = {"ovr_GetInputState", "ovr_GetHmdDesc", "ovr_GetTrackingState", "ovr_GetDevicePoses"}
PATCHES = (
    ("loneecho2.exe", "132d973ac5125fb4e37da3fdf94fd022ad2f3aea1af4b5dfef6aa902d175218b",
     0x117FD01, bytes.fromhex("85c07511"), bytes.fromhex("9090eb11")),
    ("pnsovr.dll", "cabecabc571f0624ed91b2d208309c568b13706ef6e547dc32d16e719ef240bc",
     0x98DEA, bytes.fromhex("7427"), bytes.fromhex("eb27")),
)
# The platform SDK verifies its DLL separately from the executable's VR loader.
# Set the caller's status to success and supply INVALID_HANDLE_VALUE instead
# of invoking its signature verifier. Its existing invalid-handle branch then
# calls LoadLibraryW without closing a handle. The actual DLL load and symbol
# lookups still execute. See reports/le2-platform-loader.asm, RVA 0x98823.
PLATFORM_SIGNATURE_PATCH = (
    0x98823,
    bytes.fromhex("c785c0050000ffffffffe8ae000000"),
    bytes.fromhex("c785c0050000000000004883c8ff90"),
)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def patched_image(data, expected_hash, rva, before, after):
    if digest(data) != expected_hash:
        raise ValueError("Unsupported original file: SHA-256 differs")
    if len(before) != len(after):
        raise ValueError("Patch must preserve image size")
    with pefile.PE(data=data) as pe:
        offset = pe.get_offset_from_rva(rva)
    if data[offset:offset + len(before)] != before:
        raise ValueError(f"Unexpected instructions at RVA {rva:#x}")
    result = data[:offset] + after + data[offset + len(before):]
    return result, offset


def exports(data):
    with pefile.PE(data=data) as pe:
        return {s.name.decode("ascii") for s in pe.DIRECTORY_ENTRY_EXPORT.symbols if s.name}


def platform_imports(data):
    with pefile.PE(data=data) as pe:
        return {
            symbol.name.decode("ascii")
            for module in pe.DIRECTORY_ENTRY_IMPORT
            if module.dll.lower() == b"libovrplatform64_1.dll"
            for symbol in module.imports if symbol.name
        }


def build_proxy(shim_exports):
    build = ROOT / "working/platform-proxy"
    build.mkdir(exist_ok=True)
    definition = build / "platform.def"
    definition.write_text("LIBRARY LibOVRPlatform64_1\nEXPORTS\n" + "".join(
        f"    {name}=LibOVRRT64_1.{name}\n" for name in sorted(shim_exports - LOCAL_EXPORTS)
    ) + "".join(f"    {name}\n" for name in sorted(LOCAL_EXPORTS)))
    kernel_def = build / "kernel32.def"
    kernel_def.write_text("LIBRARY KERNEL32.dll\nEXPORTS\n" + "".join(
        f"    {name}\n" for name in (
            "GetModuleHandleA", "GetProcAddress", "GetProcessHeap", "HeapAlloc", "HeapFree",
            "AcquireSRWLockExclusive", "ReleaseSRWLockExclusive", "OutputDebugStringA",
        )
    ))
    kernel_lib = build / "kernel32.lib"
    subprocess.run([
        "llvm-dlltool", "-m", "i386:x86-64", "-d", str(kernel_def), "-l", str(kernel_lib)
    ], check=True)
    obj = build / "platform_proxy.obj"
    output = build / "LibOVRPlatform64_1.dll"
    subprocess.run([
        "clang", "--target=x86_64-pc-windows-msvc", "-O2", "-ffreestanding",
        "-Wall", "-Wextra", "-Werror", "-c",
        str(ROOT / "compat/platform_proxy.c"), "-o", str(obj)
    ], check=True)
    subprocess.run([
        "lld-link", "/dll", "/noentry", "/nodefaultlib", "/machine:x64", "/timestamp:0",
        f"/def:{definition}", f"/out:{output}", str(obj), str(kernel_lib)
    ], check=True)
    return output.read_bytes()


def build_vr_proxy(shim_exports):
    build = ROOT / "working/platform-proxy"
    definition = build / "vr.def"
    definition.write_text("LIBRARY LibOVRRT64_1\nEXPORTS\n" + "".join(
        f"    {name}=LibOVRRT64_upstream.{name}\n"
        for name in sorted(shim_exports - LOCAL_VR_EXPORTS)
    ) + "".join(f"    {name}\n" for name in sorted(LOCAL_VR_EXPORTS)))
    kernel_def = build / "vr-kernel32.def"
    kernel_def.write_text("LIBRARY KERNEL32.dll\nEXPORTS\n" + "".join(
        f"    {name}\n" for name in (
            "GetModuleHandleA", "LoadLibraryA", "GetProcAddress", "OutputDebugStringA",
            "AcquireSRWLockExclusive", "ReleaseSRWLockExclusive",
            "GetEnvironmentVariableA",
        )
    ))
    kernel_lib = build / "vr-kernel32.lib"
    subprocess.run(["llvm-dlltool", "-m", "i386:x86-64", "-d", str(kernel_def),
                    "-l", str(kernel_lib)], check=True)
    obj = build / "vr_proxy.obj"
    output = build / "LibOVRRT64_1.dll"
    subprocess.run([
        "clang", "--target=x86_64-pc-windows-msvc", "-O2", "-ffreestanding",
        "-Wall", "-Wextra", "-Werror", "-c", str(ROOT / "compat/vr_proxy.c"), "-o", str(obj),
    ], check=True)
    subprocess.run([
        "lld-link", "/dll", "/noentry", "/nodefaultlib", "/machine:x64", "/timestamp:0",
        f"/def:{definition}", f"/out:{output}", str(obj), str(kernel_lib),
    ], check=True)
    return output.read_bytes()


def atomic_write(path, data):
    fd, temporary = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    try:
        with os.fdopen(fd, "wb") as stream:
            stream.write(data)
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def build_openxr_proxy(loader_data):
    build = ROOT / "working/platform-proxy"
    local = {"xrGetInstanceProcAddr", "xrSuggestInteractionProfileBindings",
             "xrCreateActionSpace", "xrLocateSpace", "xrDestroySpace", "le2_GetHandVelocity"}
    definition = build / "openxr.def"
    definition.write_text("LIBRARY openxr_loader\nEXPORTS\n" + "".join(
        f"    {name}=openxr_loader_upstream.{name}\n"
        for name in sorted(exports(loader_data) - local)
    ) + "".join(f"    {name}\n" for name in sorted(local)))
    kernel_def = build / "openxr-kernel32.def"
    kernel_def.write_text("LIBRARY KERNEL32.dll\nEXPORTS\n" + "".join(
        f"    {name}\n" for name in (
            "GetModuleHandleA", "LoadLibraryA", "GetProcAddress", "OutputDebugStringA",
            "GetProcessHeap", "HeapAlloc", "HeapFree", "GetEnvironmentVariableA",
            "AcquireSRWLockExclusive", "ReleaseSRWLockExclusive",
        )
    ))
    kernel_lib = build / "openxr-kernel32.lib"
    subprocess.run(["llvm-dlltool", "-m", "i386:x86-64", "-d", str(kernel_def),
                    "-l", str(kernel_lib)], check=True)
    obj = build / "openxr_input_proxy.obj"
    output = build / "openxr_loader.dll"
    subprocess.run([
        "clang", "--target=x86_64-pc-windows-msvc", "-O2", "-ffreestanding",
        "-Wall", "-Wextra", "-Werror", "-I/usr/include/openxr", "-c",
        str(ROOT / "compat/openxr_input_proxy.c"), "-o", str(obj),
    ], check=True)
    subprocess.run([
        "lld-link", "/dll", "/noentry", "/nodefaultlib", "/machine:x64", "/timestamp:0",
        f"/def:{definition}", f"/out:{output}", str(obj), str(kernel_lib),
    ], check=True)
    result = output.read_bytes()
    if exports(result) != exports(loader_data) | {"le2_GetHandVelocity"}:
        raise ValueError("OpenXR proxy exports differ from the pinned loader")
    return result


def prepare(apply=False):
    if TARGET.resolve() == ORIGINAL.resolve() or ORIGINAL.resolve() in TARGET.resolve().parents:
        raise ValueError("Working directory must be separate from originals")
    prepared = {}
    manifest = {"status": "gameplay confirmed; native throw velocity validation pending", "patches": []}
    for name, sha, rva, before, after in PATCHES:
        original = (ORIGINAL / name).read_bytes()
        patched, offset = patched_image(original, sha, rva, before, after)
        earlier_patch_hash = digest(patched)
        extra_changes = []
        if name == "pnsovr.dll":
            extra_rva, extra_before, extra_after = PLATFORM_SIGNATURE_PATCH
            patched, extra_offset = patched_image(
                patched, digest(patched), extra_rva, extra_before, extra_after
            )
            extra_changes.append({
                "rva": hex(extra_rva), "offset": hex(extra_offset),
                "before": extra_before.hex(), "after": extra_after.hex()
            })
        target = TARGET / name
        if target.is_symlink() or os.path.samefile(target, ORIGINAL / name):
            raise ValueError(f"Working file must be an independent copy: {target}")
        if digest(target.read_bytes()) not in {sha, earlier_patch_hash, digest(patched)}:
            raise ValueError(f"Refusing to overwrite an unrecognized working file: {target}")
        prepared[target] = patched
        manifest["patches"].append({
            "file": name, "input_sha256": sha, "output_sha256": digest(patched),
            "rva": hex(rva), "offset": hex(offset), "before": before.hex(), "after": after.hex(),
            "additional_changes": extra_changes,
        })
    shim_data = SHIM.read_bytes()
    if digest(shim_data) != SHIM_HASH:
        raise ValueError("Unexpected upstream shim hash")
    shim_exports = exports(shim_data)
    needed = platform_imports((ORIGINAL / "pnsovr.dll").read_bytes())
    if needed - shim_exports != {"ovr_Voip_GetPCMFloat"}:
        raise ValueError("Unexpected platform export coverage")
    with zipfile.ZipFile(ROOT / "working/openxr_loader_windows-1.1.63.zip") as archive:
        loader_data = archive.read("x64/bin/openxr_loader.dll")
    if digest(loader_data) != LOADER_HASH:
        raise ValueError("Unexpected upstream OpenXR loader hash")
    if apply:
        proxy = build_proxy(shim_exports)
        if needed - exports(proxy):
            raise ValueError("Platform proxy is missing required imports")
        vr_proxy = build_vr_proxy(shim_exports)
        if exports(vr_proxy) != shim_exports:
            raise ValueError("VR proxy export coverage differs from upstream")
        prepared[TARGET / "LibOVRRT64_1.dll"] = vr_proxy
        prepared[TARGET / "LibOVRPlatform64_1.dll"] = proxy
        prepared[TARGET / "LibOVRRT64_upstream.dll"] = shim_data
        prepared[TARGET / "openxr_loader.dll"] = build_openxr_proxy(loader_data)
        prepared[TARGET / "openxr_loader_upstream.dll"] = loader_data
        previous_manifest = ROOT / "reports/le2-patch-manifest.json"
        previous_hashes = (json.loads(previous_manifest.read_text()).get("compatibility_files", {})
                           if previous_manifest.exists() else {})
        # Allow upgrades of our own recorded build, but preserve unknown DLLs.
        for path, data in list(prepared.items())[2:]:
            allowed = {digest(data), previous_hashes.get(path.name)}
            if path.name == "openxr_loader.dll":
                allowed.add(LOADER_HASH)
            if path.is_symlink() or (path.exists() and digest(path.read_bytes()) not in allowed):
                raise ValueError(f"Unrecognized existing compatibility DLL: {path}")
        for path, data in prepared.items():
            atomic_write(path, data)
        manifest["compatibility_files"] = {
            p.name: digest(data) for p, data in list(prepared.items())[2:]
        }
        manifest["platform_imports_resolved"] = len(needed)
        manifest["local_platform_exports"] = sorted(LOCAL_EXPORTS)
        manifest["local_vr_exports"] = sorted(LOCAL_VR_EXPORTS)
        manifest["controller_pose"] = "aim; LE2_CONTROLLER_POSE=grip restores original binding"
        manifest["hand_velocity"] = "runtime XrSpaceVelocity; LE2_VELOCITY=legacy restores upstream estimates"
        (ROOT / "reports/le2-patch-manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(json.dumps(manifest, indent=2))
    print("Applied to working copy." if apply else "Checks passed; no files changed.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--apply", action="store_true")
    mode.add_argument("--check", action="store_true")
    args = parser.parse_args()
    try:
        prepare(args.apply)
    except (OSError, ValueError, pefile.PEFormatError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"Preparation stopped: {error}\n")
