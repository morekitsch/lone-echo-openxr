//! Convert runtime-owned OpenXR visibility geometry to the CAPI stencil ABI.
//! No masks are guessed for a headset; unavailable or unusable data falls back.
#![cfg_attr(not(any(windows, test)), allow(dead_code))]
use crate::abi::{OvrFovStencilDesc, OvrFovStencilMeshBuffer, OvrVector2f};

pub(crate) const INVALID: i32 = -1005;
pub(crate) const UNSUPPORTED: i32 = -1009;
pub(crate) const MAX_VERTICES: usize = 65536;
pub(crate) const MAX_INDICES: usize = 1_048_576;

pub(crate) struct Mesh {
    pub vertices: Vec<OvrVector2f>,
    pub indices: Vec<u16>,
}

/// Query the native two-buffer interface with bounded allocations and retries.
/// The callback must honor OpenXR's capacities and only write inside its arrays.
pub(crate) fn fetch(
    mut query: impl FnMut(&mut openxr::sys::VisibilityMaskKHR) -> openxr::sys::Result,
) -> Result<openxr::VisibilityMask, i32> {
    for _ in 0..3 {
        let mut info = openxr::sys::VisibilityMaskKHR {
            ty: openxr::sys::VisibilityMaskKHR::TYPE,
            next: core::ptr::null_mut(),
            vertex_capacity_input: 0,
            vertex_count_output: 0,
            vertices: core::ptr::null_mut(),
            index_capacity_input: 0,
            index_count_output: 0,
            indices: core::ptr::null_mut(),
        };
        if query(&mut info) != openxr::sys::Result::SUCCESS {
            return Err(UNSUPPORTED);
        }
        let (vertices, indices) = (
            info.vertex_count_output as usize,
            info.index_count_output as usize,
        );
        if vertices == 0 || indices == 0 || vertices > MAX_VERTICES || indices > MAX_INDICES {
            return Err(UNSUPPORTED);
        }
        let mut mask = openxr::VisibilityMask {
            vertices: vec![openxr::Vector2f { x: 0.0, y: 0.0 }; vertices],
            indices: vec![0; indices],
        };
        info.vertex_capacity_input = vertices as u32;
        info.vertices = mask.vertices.as_mut_ptr();
        info.index_capacity_input = indices as u32;
        info.indices = mask.indices.as_mut_ptr();
        let result = query(&mut info);
        if result == openxr::sys::Result::ERROR_SIZE_INSUFFICIENT {
            continue;
        }
        if result != openxr::sys::Result::SUCCESS
            || info.vertex_count_output == 0
            || info.index_count_output == 0
            || info.vertex_count_output as usize > vertices
            || info.index_count_output as usize > indices
        {
            return Err(UNSUPPORTED);
        }
        mask.vertices.truncate(info.vertex_count_output as usize);
        mask.indices.truncate(info.index_count_output as usize);
        return Ok(mask);
    }
    Err(UNSUPPORTED)
}

pub(crate) fn validate_desc(desc: &OvrFovStencilDesc) -> Result<(), i32> {
    let f = desc.fov;
    if !(0..=3).contains(&desc.stencil_type)
        || !(0..=1).contains(&desc.eye)
        || desc.stencil_flags & !1 != 0
        || ![f.left_tan, f.right_tan, f.up_tan, f.down_tan]
            .iter()
            .all(|x| x.is_finite() && *x > 0.0)
        || !(f.left_tan + f.right_tan).is_finite()
        || !(f.up_tan + f.down_tan).is_finite()
    {
        return Err(INVALID);
    }
    Ok(())
}

type Point = [f64; 2];
fn cross(a: Point, b: Point, c: Point) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

// Clip a triangle or convex polygon to the caller's actual viewport. Clamping
// individual vertices would change the shape and could hide visible pixels.
fn clip(mut polygon: Vec<Point>) -> Vec<Point> {
    for (axis, boundary, sign) in [(0, 0.0, 1.0), (0, 1.0, -1.0), (1, 0.0, 1.0), (1, 1.0, -1.0)] {
        let input = std::mem::take(&mut polygon);
        if input.is_empty() {
            break;
        }
        let mut a = *input.last().unwrap();
        for b in input.iter().copied() {
            let da = sign * (a[axis] - boundary);
            let db = sign * (b[axis] - boundary);
            if (da >= 0.0) != (db >= 0.0) {
                let t = da / (da - db);
                let mut p = [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])];
                p[axis] = boundary;
                polygon.push(p);
            }
            if db >= 0.0 {
                polygon.push(b);
            }
            a = b;
        }
    }
    polygon
}

fn vertex(p: Point) -> OvrVector2f {
    OvrVector2f {
        x: p[0] as f32,
        y: p[1] as f32,
    }
}

// This deliberately fits a conservative centered rectangle, not a bounding box
// which can include invisible corners. Refuse non-convex outlines.
fn inscribed_rectangle(polygon: &[Point]) -> Result<Vec<Point>, i32> {
    if polygon.len() < 3 {
        return Err(UNSUPPORTED);
    }
    let area: f64 = (0..polygon.len())
        .map(|i| {
            let a = polygon[i];
            let b = polygon[(i + 1) % polygon.len()];
            a[0] * b[1] - b[0] * a[1]
        })
        .sum();
    if area.abs() < 1e-12 {
        return Err(UNSUPPORTED);
    }
    let winding = area.signum();
    // Every point must be on the interior side of every edge. This also rejects
    // self-crossing and non-convex data rather than constructing a risky mask.
    for i in 0..polygon.len() {
        if polygon
            .iter()
            .any(|&p| winding * cross(polygon[i], polygon[(i + 1) % polygon.len()], p) < -1e-12)
        {
            return Err(UNSUPPORTED);
        }
    }
    let center = [
        polygon.iter().map(|p| p[0]).sum::<f64>() / polygon.len() as f64,
        polygon.iter().map(|p| p[1]).sum::<f64>() / polygon.len() as f64,
    ];
    let mut low = [f64::INFINITY; 2];
    let mut high = [f64::NEG_INFINITY; 2];
    for p in polygon {
        for axis in 0..2 {
            low[axis] = low[axis].min(p[axis]);
            high[axis] = high[axis].max(p[axis]);
        }
    }
    let half = [
        (center[0] - low[0]).min(high[0] - center[0]),
        (center[1] - low[1]).min(high[1] - center[1]),
    ];
    let mut scale = 1.0f64;
    for i in 0..polygon.len() {
        let a = polygon[i];
        let b = polygon[(i + 1) % polygon.len()];
        let extent = (b[1] - a[1]).abs() * half[0] + (b[0] - a[0]).abs() * half[1];
        if extent > 0.0 {
            scale = scale.min((winding * cross(a, b, center)) / extent);
        }
    }
    // A small inward margin keeps f64->f32 rounding from placing corners outside.
    scale *= 1.0 - 1e-6;
    let [x, y] = [half[0] * scale, half[1] * scale];
    if x <= 1e-8 || y <= 1e-8 {
        return Err(UNSUPPORTED);
    }
    Ok(vec![
        [center[0] - x, center[1] - y],
        [center[0] + x, center[1] - y],
        [center[0] + x, center[1] + y],
        [center[0] - x, center[1] + y],
    ])
}

pub(crate) fn convert(
    desc: &OvrFovStencilDesc,
    mask: &openxr::VisibilityMask,
) -> Result<Mesh, i32> {
    validate_desc(desc)?;
    if mask.vertices.is_empty()
        || mask.indices.is_empty()
        || mask.vertices.len() > MAX_VERTICES
        || mask.indices.len() > MAX_INDICES
        || mask
            .indices
            .iter()
            .any(|&i| i as usize >= mask.vertices.len())
    {
        return Err(UNSUPPORTED);
    }
    let f = desc.fov;
    let mut uv = Vec::with_capacity(mask.vertices.len());
    for v in &mask.vertices {
        if !v.x.is_finite() || !v.y.is_finite() {
            return Err(UNSUPPORTED);
        }
        // OpenXR mask: eye-space z=-1, right/up positive. CAPI: UV right/down.
        let p = [
            (f64::from(v.x) + f64::from(f.left_tan))
                / (f64::from(f.left_tan) + f64::from(f.right_tan)),
            (f64::from(f.up_tan) - f64::from(v.y)) / (f64::from(f.up_tan) + f64::from(f.down_tan)),
        ];
        uv.push(p);
    }
    let mut output = Mesh {
        vertices: Vec::new(),
        indices: Vec::new(),
    };
    match desc.stencil_type {
        0 | 1 => {
            if mask.indices.len() % 3 != 0 {
                return Err(UNSUPPORTED);
            }
            for triangle in mask.indices.chunks_exact(3) {
                let polygon = clip(triangle.iter().map(|&i| uv[i as usize]).collect());
                for i in 1..polygon.len().saturating_sub(1) {
                    let tri = [polygon[0], polygon[i], polygon[i + 1]];
                    if cross(tri[0], tri[1], tri[2]).abs() < 1e-15 {
                        continue;
                    }
                    if output.vertices.len() + 3 > MAX_VERTICES {
                        return Err(UNSUPPORTED);
                    }
                    let start = output.vertices.len() as u16;
                    output.vertices.extend(tri.into_iter().map(vertex));
                    output.indices.extend([start, start + 1, start + 2]);
                }
            }
        }
        2 | 3 => {
            if mask.indices.len() != mask.vertices.len() || mask.indices.len() < 3 {
                return Err(UNSUPPORTED);
            }
            let mut seen = vec![false; mask.vertices.len()];
            for &i in &mask.indices {
                if std::mem::replace(&mut seen[i as usize], true) {
                    return Err(UNSUPPORTED);
                }
            }
            let polygon: Vec<_> = mask.indices.iter().map(|&i| uv[i as usize]).collect();
            if desc.stencil_type == 2 {
                // Do not fabricate a boundary for an outline outside this FOV.
                if polygon.iter().flatten().any(|v| !(0.0..=1.0).contains(v)) {
                    return Err(UNSUPPORTED);
                }
                output.vertices = polygon.into_iter().map(vertex).collect();
                // OpenXR provides a closed loop; CAPI requests a line LIST.
                for i in 0..output.vertices.len() {
                    output
                        .indices
                        .extend([i as u16, ((i + 1) % output.vertices.len()) as u16]);
                }
            } else {
                // Limit the quadratic convexity check on runtime-supplied data.
                if polygon.len() > 4096 {
                    return Err(UNSUPPORTED);
                }
                // Validate before clipping: clipping alone cannot make an
                // arbitrary non-convex runtime outline safe for this routine.
                inscribed_rectangle(&polygon)?;
                output.vertices = inscribed_rectangle(&clip(polygon))?
                    .into_iter()
                    .map(vertex)
                    .collect();
                output.indices = vec![0, 1, 2, 0, 2, 3];
            }
        }
        _ => unreachable!(),
    }
    if output.vertices.is_empty() {
        return Err(UNSUPPORTED);
    }
    if output.vertices.iter().any(|p| {
        !p.x.is_finite()
            || !p.y.is_finite()
            || !(0.0..=1.0).contains(&p.x)
            || !(0.0..=1.0).contains(&p.y)
    }) {
        return Err(UNSUPPORTED);
    }
    if desc.stencil_flags & 1 != 0 {
        for v in &mut output.vertices {
            v.y = 1.0 - v.y;
        }
    }
    Ok(output)
}

/// # Safety
/// `buffer` must address the CAPI structure. Non-null arrays must have the
/// declared capacities, and the arrays/structure must not overlap each other.
pub(crate) unsafe fn write_mesh(mesh: &Mesh, buffer: *mut OvrFovStencilMeshBuffer) -> i32 {
    if buffer.is_null() {
        return INVALID;
    }
    // Used counts are outputs and may not have been initialized by the caller.
    let vertices = unsafe { core::ptr::addr_of!((*buffer).alloc_vertex_count).read_unaligned() };
    let indices = unsafe { core::ptr::addr_of!((*buffer).alloc_index_count).read_unaligned() };
    if vertices < 0 || indices < 0 {
        return INVALID;
    }
    unsafe {
        core::ptr::addr_of_mut!((*buffer).used_vertex_count)
            .write_unaligned(mesh.vertices.len() as i32);
        core::ptr::addr_of_mut!((*buffer).used_index_count)
            .write_unaligned(mesh.indices.len() as i32);
    }
    if vertices == 0 && indices == 0 {
        return 0;
    }
    if (vertices as usize) < mesh.vertices.len() || (indices as usize) < mesh.indices.len() {
        return INVALID;
    }
    let vertex_buffer = unsafe { core::ptr::addr_of!((*buffer).vertex_buffer).read_unaligned() };
    let index_buffer = unsafe { core::ptr::addr_of!((*buffer).index_buffer).read_unaligned() };
    if vertex_buffer.is_null() || index_buffer.is_null() {
        return INVALID;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(mesh.vertices.as_ptr(), vertex_buffer, mesh.vertices.len());
        core::ptr::copy_nonoverlapping(mesh.indices.as_ptr(), index_buffer, mesh.indices.len());
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::{OvrFovPort, OvrQuatf};

    fn desc(kind: i32) -> OvrFovStencilDesc {
        OvrFovStencilDesc {
            stencil_type: kind,
            stencil_flags: 0,
            eye: 0,
            fov: OvrFovPort {
                left_tan: 1.0,
                right_tan: 1.0,
                up_tan: 1.0,
                down_tan: 1.0,
            },
            hmd_to_eye_rotation: OvrQuatf {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                w: 1.0,
            },
        }
    }
    fn mask(points: &[[f32; 2]], indices: &[u32]) -> openxr::VisibilityMask {
        openxr::VisibilityMask {
            vertices: points
                .iter()
                .map(|p| openxr::Vector2f { x: p[0], y: p[1] })
                .collect(),
            indices: indices.to_vec(),
        }
    }
    fn triangle() -> openxr::VisibilityMask {
        mask(&[[-1.0, -1.0], [1.0, -1.0], [-1.0, 1.0]], &[0, 1, 2])
    }
    fn buffer(vertices: &mut [OvrVector2f], indices: &mut [u16]) -> OvrFovStencilMeshBuffer {
        OvrFovStencilMeshBuffer {
            alloc_vertex_count: vertices.len() as i32,
            used_vertex_count: -7,
            vertex_buffer: vertices.as_mut_ptr(),
            alloc_index_count: indices.len() as i32,
            used_index_count: -9,
            index_buffer: indices.as_mut_ptr(),
        }
    }

    #[test]
    fn native_query_retries_changed_counts_and_uses_filled_counts() {
        let mut calls = 0;
        let mask = fetch(|info| {
            calls += 1;
            if info.vertex_capacity_input == 0 {
                assert!(info.vertices.is_null() && info.indices.is_null());
                info.vertex_count_output = if calls == 1 { 3 } else { 4 };
                info.index_count_output = if calls == 1 { 3 } else { 6 };
                return openxr::sys::Result::SUCCESS;
            }
            if calls == 2 {
                return openxr::sys::Result::ERROR_SIZE_INSUFFICIENT;
            }
            assert_eq!(
                (info.vertex_capacity_input, info.index_capacity_input),
                (4, 6)
            );
            let m = triangle();
            unsafe {
                core::ptr::copy_nonoverlapping(m.vertices.as_ptr(), info.vertices, 3);
                core::ptr::copy_nonoverlapping(m.indices.as_ptr(), info.indices, 3);
            }
            info.vertex_count_output = 3;
            info.index_count_output = 3;
            openxr::sys::Result::SUCCESS
        })
        .unwrap();
        assert_eq!(calls, 4);
        assert_eq!(mask.vertices.len(), 3);
        assert_eq!(mask.indices, vec![0, 1, 2]);
        assert_eq!(convert(&desc(0), &mask).unwrap().vertices.len(), 3);
    }

    #[test]
    fn unavailable_huge_unstable_and_inconsistent_native_masks_fall_back() {
        for count in [0, MAX_VERTICES as u32 + 1, u32::MAX] {
            let mut calls = 0;
            assert_eq!(
                fetch(|info| {
                    calls += 1;
                    info.vertex_count_output = count;
                    info.index_count_output = 3;
                    openxr::sys::Result::SUCCESS
                })
                .err(),
                Some(UNSUPPORTED)
            );
            assert_eq!(calls, 1);
        }
        let mut calls = 0;
        assert_eq!(
            fetch(|info| {
                calls += 1;
                info.vertex_count_output = 3;
                info.index_count_output = 3;
                if info.vertex_capacity_input == 0 {
                    openxr::sys::Result::SUCCESS
                } else {
                    openxr::sys::Result::ERROR_SIZE_INSUFFICIENT
                }
            })
            .err(),
            Some(UNSUPPORTED)
        );
        assert_eq!(calls, 6);
        assert_eq!(
            fetch(|_| openxr::sys::Result::ERROR_FUNCTION_UNSUPPORTED).err(),
            Some(UNSUPPORTED)
        );
        assert_eq!(
            fetch(|info| {
                info.vertex_count_output = if info.vertex_capacity_input == 0 {
                    3
                } else {
                    4
                };
                info.index_count_output = 3;
                openxr::sys::Result::SUCCESS
            })
            .err(),
            Some(UNSUPPORTED)
        );
    }

    #[test]
    fn x64_capi_stencil_layout_matches_sdk() {
        use core::mem::{align_of, offset_of, size_of};
        assert_eq!(size_of::<OvrFovStencilDesc>(), 48);
        assert_eq!(align_of::<OvrFovStencilDesc>(), 8);
        assert_eq!(offset_of!(OvrFovStencilDesc, stencil_flags), 4);
        assert_eq!(offset_of!(OvrFovStencilDesc, eye), 8);
        assert_eq!(offset_of!(OvrFovStencilDesc, fov), 12);
        assert_eq!(offset_of!(OvrFovStencilDesc, hmd_to_eye_rotation), 28);
        assert_eq!(size_of::<OvrFovStencilMeshBuffer>(), 32);
        assert_eq!(align_of::<OvrFovStencilMeshBuffer>(), 8);
        assert_eq!(offset_of!(OvrFovStencilMeshBuffer, used_vertex_count), 4);
        assert_eq!(offset_of!(OvrFovStencilMeshBuffer, vertex_buffer), 8);
        assert_eq!(offset_of!(OvrFovStencilMeshBuffer, alloc_index_count), 16);
        assert_eq!(offset_of!(OvrFovStencilMeshBuffer, used_index_count), 20);
        assert_eq!(offset_of!(OvrFovStencilMeshBuffer, index_buffer), 24);
    }

    #[test]
    fn asymmetric_fov_projects_correctly_and_origin_flag_only_flips_y() {
        let mut d = desc(0);
        d.fov = OvrFovPort {
            left_tan: 2.0,
            right_tan: 1.0,
            up_tan: 3.0,
            down_tan: 1.0,
        };
        let m = mask(&[[-2.0, -1.0], [1.0, -1.0], [-2.0, 3.0]], &[0, 1, 2]);
        let top = convert(&d, &m).unwrap();
        assert_eq!((top.vertices[0].x, top.vertices[0].y), (0.0, 1.0));
        assert_eq!((top.vertices[1].x, top.vertices[1].y), (1.0, 1.0));
        assert_eq!((top.vertices[2].x, top.vertices[2].y), (0.0, 0.0));
        d.stencil_flags = 1;
        d.eye = 1;
        let bottom = convert(&d, &m).unwrap();
        for (a, b) in top.vertices.iter().zip(bottom.vertices.iter()) {
            assert_eq!(a.x, b.x);
            assert_eq!(a.y, 1.0 - b.y);
        }
        assert_eq!(top.indices, bottom.indices);
    }

    #[test]
    fn triangles_are_clipped_without_filling_visible_corners() {
        let m = mask(&[[-2.0, -2.0], [2.0, -2.0], [-2.0, 2.0]], &[0, 1, 2]);
        for kind in [0, 1] {
            let out = convert(&desc(kind), &m).unwrap();
            assert!(
                out.vertices
                    .iter()
                    .all(|v| (0.0..=1.0).contains(&v.x) && (0.0..=1.0).contains(&v.y))
            );
            // Source triangle occupies x<=y in UV space; never expand it.
            assert!(out.vertices.iter().all(|v| v.x <= v.y + 1e-6));
            let area: f64 = out
                .indices
                .chunks_exact(3)
                .map(|t| {
                    let p: Vec<Point> = t
                        .iter()
                        .map(|&i| {
                            let v = out.vertices[i as usize];
                            [v.x as f64, v.y as f64]
                        })
                        .collect();
                    cross(p[0], p[1], p[2]).abs() * 0.5
                })
                .sum();
            assert!((area - 0.5).abs() < 1e-6);
        }
    }

    #[test]
    fn empty_degenerate_bad_indices_and_nonfinite_masks_fall_back() {
        for m in [
            mask(&[], &[]),
            mask(&[[0.0, 0.0]], &[0, 0, 0]),
            mask(&[[0.0, 0.0]], &[0, 1, 2]),
            mask(&[[f32::NAN, 0.0]; 3], &[0, 1, 2]),
            mask(&[[f32::INFINITY, 0.0]; 3], &[0, 1, 2]),
            mask(&[[0.0, 0.0]; 3], &[0, 1]),
            mask(&[[4.0, 4.0], [5.0, 4.0], [4.0, 5.0]], &[0, 1, 2]),
        ] {
            assert_eq!(convert(&desc(0), &m).err(), Some(UNSUPPORTED));
        }
    }

    #[test]
    fn invalid_request_does_not_become_a_visibility_mask() {
        for mutate in [
            |d: &mut OvrFovStencilDesc| d.eye = 2,
            |d: &mut OvrFovStencilDesc| d.stencil_type = 4,
            |d: &mut OvrFovStencilDesc| d.stencil_flags = 2,
            |d: &mut OvrFovStencilDesc| d.fov.up_tan = 0.0,
            |d: &mut OvrFovStencilDesc| d.fov.left_tan = f32::NAN,
            |d: &mut OvrFovStencilDesc| d.fov.right_tan = -1.0,
        ] {
            let mut d = desc(0);
            mutate(&mut d);
            assert_eq!(convert(&d, &triangle()).err(), Some(INVALID));
        }
    }

    #[test]
    fn border_respects_runtime_index_order_and_closes_a_line_list() {
        let m = mask(
            &[[-1.0, -1.0], [1.0, 1.0], [1.0, -1.0], [-1.0, 1.0]],
            &[0, 2, 1, 3],
        );
        let out = convert(&desc(2), &m).unwrap();
        assert_eq!(out.indices, vec![0, 1, 1, 2, 2, 3, 3, 0]);
        assert_eq!((out.vertices[1].x, out.vertices[1].y), (1.0, 1.0));
        let mut bad = m;
        bad.indices = vec![0, 0, 1, 3];
        assert_eq!(convert(&desc(2), &bad).err(), Some(UNSUPPORTED));
    }

    #[test]
    fn rectangle_corners_stay_inside_diamond_and_keep_capi_order() {
        let m = mask(
            &[[0.0, -1.0], [1.0, 0.0], [0.0, 1.0], [-1.0, 0.0]],
            &[0, 1, 2, 3],
        );
        let out = convert(&desc(3), &m).unwrap();
        assert_eq!(out.indices, vec![0, 1, 2, 0, 2, 3]);
        assert_eq!(out.vertices.len(), 4);
        for p in &out.vertices {
            assert!((p.x - 0.5).abs() + (p.y - 0.5).abs() <= 0.5);
        }
        assert!(out.vertices[0].x < out.vertices[1].x && out.vertices[0].y < out.vertices[3].y);
        assert!((out.vertices[0].x - 0.25).abs() < 1e-5);
        let concave = mask(
            &[
                [-1.0, -1.0],
                [1.0, -1.0],
                [0.0, 0.0],
                [1.0, 1.0],
                [-1.0, 1.0],
            ],
            &[0, 1, 2, 3, 4],
        );
        assert_eq!(convert(&desc(3), &concave).err(), Some(UNSUPPORTED));
    }

    #[test]
    fn excessive_geometry_cannot_overflow_u16_indices() {
        let mut m = triangle();
        m.indices = vec![0, 1, 2].repeat(MAX_VERTICES / 3 + 1);
        assert_eq!(convert(&desc(0), &m).err(), Some(UNSUPPORTED));
    }

    #[test]
    fn rectangle_stays_inside_offset_clipped_ellipses_for_both_origins() {
        for n in 0..24 {
            let mut d = desc(3);
            d.stencil_flags = n % 2;
            d.fov.left_tan = 0.7 + n as f32 * 0.025;
            d.fov.up_tan = 0.9;
            let points: Vec<_> = (0..32)
                .map(|i| {
                    let angle = i as f32 * std::f32::consts::TAU / 32.0;
                    [angle.cos() * 1.4 + 0.1, angle.sin() * 1.2 - 0.15]
                })
                .collect();
            let m = mask(&points, &(0..32).collect::<Vec<_>>());
            let out = convert(&d, &m).unwrap();
            for v in out.vertices {
                assert!((0.0..=1.0).contains(&v.x) && (0.0..=1.0).contains(&v.y));
                let y = if d.stencil_flags == 0 { v.y } else { 1.0 - v.y };
                let p = [
                    (v.x * (d.fov.left_tan + d.fov.right_tan) - d.fov.left_tan) as f64,
                    (d.fov.up_tan - y * (d.fov.up_tan + d.fov.down_tan)) as f64,
                ];
                for i in 0..points.len() {
                    let a = points[i].map(f64::from);
                    let b = points[(i + 1) % points.len()].map(f64::from);
                    assert!(
                        cross(a, b, p) >= -1e-7,
                        "rectangle corner outside visible outline"
                    );
                }
            }
        }
    }

    #[test]
    fn sizing_short_buffers_and_success_preserve_canaries() {
        let mesh = convert(&desc(0), &triangle()).unwrap();
        let mut b = OvrFovStencilMeshBuffer {
            alloc_vertex_count: 0,
            used_vertex_count: -1,
            vertex_buffer: core::ptr::null_mut(),
            alloc_index_count: 0,
            used_index_count: -1,
            index_buffer: core::ptr::null_mut(),
        };
        assert_eq!(unsafe { write_mesh(&mesh, &mut b) }, 0);
        assert_eq!((b.used_vertex_count, b.used_index_count), (3, 3));
        let mut v = [OvrVector2f { x: 99.0, y: 99.0 }; 5];
        let mut i = [99u16; 5];
        let mut b = buffer(&mut v[1..4], &mut i[1..4]);
        b.alloc_index_count = 2;
        assert_eq!(unsafe { write_mesh(&mesh, &mut b) }, INVALID);
        assert!(v.iter().all(|p| p.x == 99.0 && p.y == 99.0));
        assert_eq!(i, [99; 5]);
        b.alloc_index_count = 3;
        assert_eq!(unsafe { write_mesh(&mesh, &mut b) }, 0);
        assert_eq!((v[0].x, v[4].x), (99.0, 99.0));
        assert_eq!((i[0], i[4]), (99, 99));
        assert_eq!(&i[1..4], &[0, 1, 2]);
        assert_eq!((b.alloc_vertex_count, b.alloc_index_count), (3, 3));
    }

    #[test]
    fn negative_counts_or_null_arrays_do_not_write_meshes() {
        let mesh = convert(&desc(0), &triangle()).unwrap();
        assert_eq!(unsafe { write_mesh(&mesh, core::ptr::null_mut()) }, INVALID);
        let mut v = [OvrVector2f { x: 99.0, y: 99.0 }; 3];
        let mut i = [99u16; 3];
        let mut b = buffer(&mut v, &mut i);
        b.alloc_vertex_count = -1;
        assert_eq!(unsafe { write_mesh(&mesh, &mut b) }, INVALID);
        assert_eq!((b.used_vertex_count, b.used_index_count), (-7, -9));
        b.alloc_vertex_count = 3;
        b.index_buffer = core::ptr::null_mut();
        assert_eq!(unsafe { write_mesh(&mesh, &mut b) }, INVALID);
        assert!(v.iter().all(|p| p.x == 99.0));
        assert_eq!(i, [99; 3]);
    }
}
