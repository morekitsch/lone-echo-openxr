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

For tomorrow's headset test:

1. Start LE2 with buffered diagnostics. Confirm the trace reports native mask
   counts and successful stencil calls; an unsupported result means the fallback
   is working but does not validate the optimization.
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
