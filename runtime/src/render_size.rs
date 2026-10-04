//! Convert full OpenXR recommended eye images to LibOVR tangent density.
use crate::abi::{OvrFovPort, OvrSizei, OvrVector2f};

fn spans(fov: OvrFovPort) -> (f32, f32) {
    (
        (fov.left_tan + fov.right_tan).max(0.1),
        (fov.up_tan + fov.down_tan).max(0.1),
    )
}

pub(crate) fn pixels_per_tan(size: OvrSizei, default_fov: OvrFovPort) -> OvrVector2f {
    let (w, h) = spans(default_fov);
    OvrVector2f {
        x: size.w as f32 / w,
        y: size.h as f32 / h,
    }
}

pub(crate) fn texture_size(
    size: OvrSizei,
    default_fov: OvrFovPort,
    requested_fov: OvrFovPort,
    density: f32,
) -> OvrSizei {
    let (base_w, base_h) = spans(default_fov);
    let (w, h) = spans(requested_fov);
    let scale = if density.is_finite() {
        density.clamp(0.25, 4.0)
    } else {
        1.0
    };
    // Divide first so the default FOV at density 1 returns exactly the
    // recommended size, without an extra pixel from rounding a reciprocal.
    OvrSizei {
        w: (size.w as f32 * (w / base_w) * scale).ceil() as i32,
        h: (size.h as f32 * (h / base_h) * scale).ceil() as i32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const SIZE: OvrSizei = OvrSizei { w: 2688, h: 2880 };
    const FOV: OvrFovPort = OvrFovPort {
        left_tan: 1.376,
        right_tan: 0.839,
        up_tan: 0.966,
        down_tan: 1.428,
    };

    #[test]
    fn default_fov_matches_recommended_image_for_either_eye() {
        for fov in [
            FOV,
            OvrFovPort {
                left_tan: 0.839,
                right_tan: 1.376,
                ..FOV
            },
        ] {
            let result = texture_size(SIZE, fov, fov, 1.0);
            assert_eq!((result.w, result.h), (2688, 2880));
            let ppt = pixels_per_tan(SIZE, fov);
            assert!((ppt.x * (fov.left_tan + fov.right_tan) - 2688.0).abs() < 0.001);
            assert!((ppt.y * (fov.up_tan + fov.down_tan) - 2880.0).abs() < 0.001);
        }
    }

    #[test]
    fn requested_fov_and_density_scale_the_recommended_size() {
        let cropped = OvrFovPort {
            left_tan: FOV.left_tan / 2.0,
            right_tan: FOV.right_tan / 2.0,
            ..FOV
        };
        let result = texture_size(SIZE, FOV, cropped, 1.0);
        assert_eq!((result.w, result.h), (1344, 2880));
        let wider = OvrFovPort {
            up_tan: FOV.up_tan * 2.0,
            down_tan: FOV.down_tan * 2.0,
            ..FOV
        };
        let result = texture_size(SIZE, FOV, wider, 0.5);
        assert_eq!((result.w, result.h), (1344, 2880));
    }
}
