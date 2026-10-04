//! Persistent startup headset data learned from a real OpenXR graphics session.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::abi::{OvrFovPort, OvrVector3f};

const CACHE_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct HmdCache {
    version: u32,
    fovs: [[f32; 4]; 2],
    eye_offsets: [[f32; 3]; 2],
    eye_width: i32,
    eye_height: i32,
    refresh_rate: f32,
}

impl HmdCache {
    pub fn new(
        fovs: [OvrFovPort; 2],
        eye_offsets: [OvrVector3f; 2],
        eye_width: i32,
        eye_height: i32,
        refresh_rate: f32,
    ) -> Option<Self> {
        let cache = Self {
            version: CACHE_VERSION,
            fovs: fovs.map(|fov| [fov.up_tan, fov.down_tan, fov.left_tan, fov.right_tan]),
            eye_offsets: eye_offsets.map(|offset| [offset.x, offset.y, offset.z]),
            eye_width,
            eye_height,
            refresh_rate,
        };
        cache.valid().then_some(cache)
    }

    pub fn fovs(self) -> [OvrFovPort; 2] {
        self.fovs.map(|fov| OvrFovPort {
            up_tan: fov[0],
            down_tan: fov[1],
            left_tan: fov[2],
            right_tan: fov[3],
        })
    }

    pub fn eye_offsets(self) -> [OvrVector3f; 2] {
        self.eye_offsets.map(|offset| OvrVector3f {
            x: offset[0],
            y: offset[1],
            z: offset[2],
        })
    }

    pub fn eye_size(self) -> (i32, i32) {
        (self.eye_width, self.eye_height)
    }

    pub fn refresh_rate(self) -> f32 {
        self.refresh_rate
    }

    fn valid(&self) -> bool {
        self.version == CACHE_VERSION
            && self.eye_width > 0
            && self.eye_height > 0
            && self.refresh_rate.is_finite()
            && self.refresh_rate > 1.0
            && self
                .fovs
                .iter()
                .flatten()
                .all(|value| value.is_finite() && *value > 0.0)
            && self
                .eye_offsets
                .iter()
                .flatten()
                .all(|value| value.is_finite())
    }
}

pub fn cache_path() -> Option<PathBuf> {
    std::env::current_exe().ok().and_then(|path| {
        path.parent()
            .map(|parent| parent.join("libovr-openxr-hmd-cache.toml"))
    })
}

pub fn load() -> Result<Option<HmdCache>, String> {
    let Some(path) = cache_path() else {
        return Ok(None);
    };
    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    let cache: HmdCache =
        toml::from_str(&contents).map_err(|error| format!("{}: {error}", path.display()))?;
    if !cache.valid() {
        return Err(format!("{}: invalid or unsupported cache", path.display()));
    }
    Ok(Some(cache))
}

pub fn save(cache: HmdCache) -> Result<(), String> {
    let Some(path) = cache_path() else {
        return Err("could not determine echovr.exe directory".into());
    };
    let contents = toml::to_string_pretty(&cache).map_err(|error| error.to_string())?;
    let temporary = path.with_extension("toml.tmp");
    std::fs::write(&temporary, contents)
        .map_err(|error| format!("{}: {error}", temporary.display()))?;
    if let Err(error) = std::fs::rename(&temporary, &path) {
        // Windows does not replace an existing destination with rename(). The
        // temporary file was completely written first, so preserve a usable
        // cache even on that platform.
        let _ = std::fs::remove_file(&path);
        std::fs::rename(&temporary, &path).map_err(|replace_error| {
            format!(
                "{}: {error}; replacement failed: {replace_error}",
                path.display()
            )
        })?;
    }
    Ok(())
}
