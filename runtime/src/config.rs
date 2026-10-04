//! Game-directory configuration for the compatibility shim.

use serde::Deserialize;
use std::ffi::CString;
use std::path::PathBuf;
use std::sync::OnceLock;

#[derive(Debug, Deserialize)]
struct FileConfig {
    #[serde(default)]
    user: UserConfig,
    #[serde(default)]
    audio: AudioConfig,
}

#[derive(Debug, Default, Deserialize)]
struct AudioConfig {
    output_guid: Option<String>,
    input_guid: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct UserConfig {
    id: Option<u64>,
    org_id: Option<u64>,
    oculus_id: Option<String>,
}

#[derive(Debug)]
pub struct UserIdentity {
    pub id: u64,
    pub org_id: u64,
    pub oculus_id: CString,
    pub audio_output_guid: Option<Vec<u16>>,
    pub audio_input_guid: Option<Vec<u16>>,
}

fn config_path() -> Option<PathBuf> {
    std::env::current_exe().ok().and_then(|path| {
        path.parent()
            .map(|parent| parent.join("libovr-openxr.toml"))
    })
}

fn load_identity() -> UserIdentity {
    let path = config_path();
    let contents = path
        .as_ref()
        .and_then(|path| std::fs::read_to_string(path).ok());
    let config = contents
        .as_deref()
        .and_then(|contents| toml::from_str::<FileConfig>(contents).ok())
        .unwrap_or(FileConfig {
            user: UserConfig::default(),
            audio: AudioConfig::default(),
        });
    let name = config
        .user
        .oculus_id
        .unwrap_or_else(|| "OpenXRLocalUser".to_owned());
    let oculus_id = CString::new(name)
        .unwrap_or_else(|_| CString::new("OpenXRLocalUser").expect("literal has no NUL"));
    let id = config.user.id.unwrap_or(1);
    let identity = UserIdentity {
        id,
        org_id: config.user.org_id.unwrap_or(id),
        oculus_id,
        audio_output_guid: config
            .audio
            .output_guid
            .map(|value| value.encode_utf16().collect()),
        audio_input_guid: config
            .audio
            .input_guid
            .map(|value| value.encode_utf16().collect()),
    };
    crate::capi::log_call(&format!(
        "libovr config path={} read={} user_id={} org_id={}",
        path.as_ref()
            .map_or_else(|| "<none>".to_owned(), |path| path.display().to_string()),
        contents.is_some(),
        identity.id,
        identity.org_id,
    ));
    identity
}

pub fn user_identity() -> &'static UserIdentity {
    static IDENTITY: OnceLock<UserIdentity> = OnceLock::new();
    IDENTITY.get_or_init(load_identity)
}

/// Return the Windows WASAPI endpoint ID for the system default device.
///
/// This is intentionally resolved inside the Wine/Windows environment rather
/// than by inspecting Linux/PipeWire state. Proton exposes the active host
/// audio device through WASAPI, which is the API LibOVR clients understand.
#[cfg(windows)]
pub fn default_audio_device_id(render: bool) -> Option<Vec<u16>> {
    use windows::Win32::Media::Audio::{
        IMMDeviceEnumerator, MMDeviceEnumerator, eCapture, eConsole, eRender,
    };
    use windows::Win32::System::Com::{
        CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
    };

    static COM_INITIALIZED: OnceLock<()> = OnceLock::new();
    COM_INITIALIZED.get_or_init(|| {
        // The calling thread may already use a different COM apartment. WASAPI
        // still works when COM was initialized by the game, so only record the
        // successful/common case and ignore an already-initialized apartment.
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    });

    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()? };
    let flow = if render { eRender } else { eCapture };
    let device = unsafe { enumerator.GetDefaultAudioEndpoint(flow, eConsole).ok()? };
    let id = unsafe { device.GetId().ok()? };
    let result = unsafe { id.to_string().ok()? }.encode_utf16().collect();
    unsafe { CoTaskMemFree(Some(id.0.cast())) };
    Some(result)
}
