# FOV stencil support

This branch implements `ovr_GetFovStencil` using `XR_KHR_visibility_mask` when
the selected OpenXR runtime exposes it. It is pending headset validation;
the working preview installation and release archive have not been replaced.

The feature can reduce shading outside the area visible through the headset
lenses. The benefit depends on the runtime's masks and how the game uses them;
no performance improvement has been measured yet.

## Behavior

- Requests the optional extension on the existing D3D11/D3D12 OpenXR session.
- Uses runtime geometry for hidden/visible triangle meshes and border lines.
- Projects eye-space vertices using the **requested** FOV into UV coordinates.
  The bottom-left flag flips Y only. As documented by the Oculus binding,
  `HmdToEyeRotation` is ignored.
- Clips triangles to the requested viewport. Border loops become closed line
  lists. An out-of-viewport border request falls back rather than inventing edges.
- Fits a conservative rectangle inside a convex visible outline for rectangle
  requests. Non-convex or degenerate outlines fall back.
- Supports count queries and caller-owned arrays. Undersized arrays receive
  required counts and an error; neither vertex nor index arrays are modified.
- Limits native counts, retries and output indices; validates finite geometry.
  Raw masks are cached by eye and type, then invalidated by the runtime's mask
  change event. No extra GPU copies, frame waits or rendering layers are added.
- Returns a defined unsupported result when masks are absent or unusable.
  `LIBOVR_OPENXR_VISIBILITY_MASK=0` forces this fallback for comparison.

## Validation before promotion

Portable tests cover the x64 layouts, projection, clipping, winding preservation,
line topology, inscribed rectangles, invalid inputs, bounded native queries,
16-bit limits, buffer sizing and canaries. Validation passed: 36 portable tests
(two existing tests ignored), the Windows release build, and 14 focused Windows
geometry/buffer/C API tests under Wine. Synthetic tests cannot establish that
the runtime provides useful masks or that a game draws them correctly.

### First Linux headset test (2026-10-04)

LE2 ran and exited normally using GE-Proton11-7 and WiVRn 26.9. The user
reported good overall visuals, possibly cleaner moving edges, and a possible
facial-shadow artifact. The complete trace contained three stencil calls, all
returning unsupported, with no successfully fetched masks. This validates the
fallback only; neither visual observation establishes an effect from masks.
The known-working installed DLL was restored and its installer hash verified.

Extension-only probes confirmed `XR_KHR_visibility_mask` is advertised both by
native WiVRn and through the installed Proton Wine/OpenXR bridge. These probes
do not create a graphics session or query headset geometry. WiVRn 26.9 forwards
headset-provided masks and can return empty geometry if none has arrived (see
its [HMD implementation](https://github.com/WiVRn/WiVRn/blob/v26.9/server/driver/wivrn_hmd.cpp)
and [headset client](https://github.com/WiVRn/WiVRn/blob/v26.9/client/scenes/stream.cpp)).
Empty geometry is a plausible cause, but the first trace does not establish it.

The next build logs extension availability, request eye/type/FOV, native query
results and counts, and early fallback reasons. These messages use the existing
opt-in logger. `LIBOVR_OPENXR_LOG=stencil` writes only stencil diagnostics and
startup/shutdown markers directly to the log, so frame traffic cannot evict
startup evidence. A second Linux run exited cleanly, but its general buffered
trace discarded the startup queries; use the focused mode for the next test.
Rendering behavior is unchanged. Do not add a guessed headset
mask or change the user's runtime configuration to force a successful result.

`tools/probe_openxr_extensions.c` is a standalone Linux/Windows extension
enumerator. It dynamically loads the OpenXR loader and creates no OpenXR
instance or session itself. Under Proton, invoke its Wine executable directly
in an isolated prefix to capture console output; the Proton launcher may hide
the program's output. Supply the same native runtime manifest as the game.

For the next headset test:

1. Start LE2 with `LIBOVR_OPENXR_LOG=stencil`. Confirm the trace reports native mask
   query result and counts. If the result is unsupported, resolve the logged
   reason before requesting a visual A/B test. If the runtime supplies an empty
   mask, the unsupported fallback is intentional and no geometry is invented.
   Successful stencil calls are required to validate the optimization.
2. Check the menu, PC picture and gameplay with diagnostics disabled. Look for
   missing pixels or triangles around the edges in both eyes, and check input.
3. Compare the same build with `LIBOVR_OPENXR_VISIBILITY_MASK=0`, then repeat with
   LE1. Compare frame timing only with identical settings and logging disabled.
4. Repeat on Windows/VDXR before claiming that configuration works. Keep the
   current preview available until the new build passes these checks.

## API references

The [OpenXR mask definition](https://registry.khronos.org/OpenXR/specs/1.1/man/html/XrVisibilityMaskKHR.html)
specifies coordinates at eye-space z=-1, and
[the query contract](https://registry.khronos.org/OpenXR/specs/1.1/man/html/xrGetVisibilityMaskKHR.html)
defines the two-buffer count query and absent-mask behavior.

The upstream LWJGL 3.3.3 Oculus definitions document the
[descriptor alignment and fields](https://github.com/LWJGL/lwjgl3/blob/3.3.3/modules/lwjgl/ovr/src/generated/java/org/lwjgl/ovr/OVRFovStencilDesc.java),
[mesh buffer layout](https://github.com/LWJGL/lwjgl3/blob/3.3.3/modules/lwjgl/ovr/src/generated/java/org/lwjgl/ovr/OVRFovStencilMeshBuffer.java),
and [stencil types, origin flag and sizing rules](https://github.com/LWJGL/lwjgl3/blob/3.3.3/modules/lwjgl/ovr/src/templates/kotlin/ovr/templates/OVR.kt).
No downloaded SDK or binding source is included in the runtime.
