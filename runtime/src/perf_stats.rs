//! Versioned CAPI performance output. Core OpenXR has no compositor timing
//! statistics, so report zero samples and a neutral adaptive GPU scale.
// SDK layout: OculusMonitor/dev/sdk/include/oculus/OVR_CAPI.h.
// Pre-1.11 layout: Revive/windows-openxr/REV_CAPI.cpp, ovrPerfStats1.

#[repr(C)]
struct LegacyStats {
    frames: [[u32; 14]; 5],
    count: i32,
    dropped: u8,
    padding: [u8; 3],
    gpu_scale: f32,
}

#[repr(C)]
struct Stats {
    // Each frame adds the padded ASW flag and three ASW counters in 1.11.
    frames: [[u32; 18]; 5],
    count: i32,
    dropped: u8,
    padding: [u8; 3],
    gpu_scale: f32,
    asw_available: u8,
    padding2: [u8; 3],
    visible_process_id: u32,
}

/// `out` must hold the CAPI perf-stats structure for the negotiated version.
pub(crate) unsafe fn write_empty(out: *mut u8, minor: u32) {
    if minor < 11 {
        let stats = LegacyStats {
            gpu_scale: 1.0,
            ..unsafe { core::mem::zeroed() }
        };
        unsafe {
            out.cast::<LegacyStats>().write_unaligned(stats);
        }
    } else {
        let stats = Stats {
            gpu_scale: 1.0,
            ..unsafe { core::mem::zeroed() }
        };
        unsafe {
            out.cast::<Stats>().write_unaligned(stats);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sdk_layouts_match_and_output_stays_inside_caller_buffer() {
        assert_eq!(core::mem::size_of::<LegacyStats>(), 292);
        assert_eq!(core::mem::size_of::<Stats>(), 380);
        assert_eq!(core::mem::offset_of!(Stats, count), 360);
        assert_eq!(core::mem::offset_of!(Stats, gpu_scale), 368);
        for (minor, size, scale_offset) in [
            (10, 292, 288),
            (12, 380, 368),
            (55, 380, 368),
            (94, 380, 368),
        ] {
            let mut buffer = [0xa5u8; 400];
            unsafe {
                write_empty(buffer.as_mut_ptr().add(1), minor);
            }
            assert_eq!(buffer[0], 0xa5);
            assert!(buffer[size + 1..].iter().all(|v| *v == 0xa5));
            let output = &buffer[1..size + 1];
            assert_eq!(
                &output[scale_offset..scale_offset + 4],
                &1.0f32.to_ne_bytes()
            );
            assert!(output[..scale_offset].iter().all(|v| *v == 0));
            assert!(output[scale_offset + 4..].iter().all(|v| *v == 0));
        }
    }
}
