//! Resolver stubs for pnsovr.dll's platform dependency.

use core::ffi::{c_char, c_void};
use std::{
    collections::BTreeMap,
    ffi::CStr,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

const MESSAGE_PLATFORM_INITIALIZED: u32 = 0x6da7_ba8f;
const MESSAGE_USER_GET_LOGGED_IN_USER: u32 = 0x436f_345d;
const MESSAGE_USER_GET_LOGGED_IN_USER_FRIENDS: u32 = 0x587c_2a8d;
const MESSAGE_USER_GET_ORG_SCOPED_ID: u32 = 0x18f0_b01b;
const MESSAGE_USER_GET_ACCESS_TOKEN: u32 = 0x06a8_5abe;
const MESSAGE_IAP_GET_VIEWER_PURCHASES: u32 = 0x3a0f_8419;
const MESSAGE_RICH_PRESENCE_GET_DESTINATIONS: u32 = 0x586f_2d14;
const MESSAGE_RICH_PRESENCE_SET: u32 = 0x3c14_7509;
const MESSAGE_IAP_GET_PRODUCTS_BY_SKU: u32 = 0x7e9a_caf5;
const MESSAGE_USER_GET_USER_PROOF: u32 = 0x2281_0483;
const MESSAGE_NOTIFICATION_GET_ROOM_INVITES: u32 = 0x6f91_6b92;
const MESSAGE_ROOM_CREATE_AND_JOIN_PRIVATE2: u32 = 0x5a3a_6243;
const MESSAGE_ROOM_GET: u32 = 0x659a_8fb8;
const MESSAGE_ROOM_GET_INVITABLE_USERS2: u32 = 0x4f53_e8b0;
const MESSAGE_ROOM_UPDATE_DATA_STORE: u32 = 0x026e_4028;
static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

#[repr(C)]
struct PlatformUser {
    id: u64,
}

// The SDK defines these as opaque handles. Keep distinct backing objects for
// each handle kind; a room must never be passed to user accessors (or vice
// versa), even in this local-only Platform implementation.
#[repr(C)]
struct PlatformRoom {
    id: u64,
    join_policy: i32,
    max_users: u32,
}

#[repr(C)]
struct PlatformDataStore;

#[repr(C)]
struct PlatformUserArray;

#[repr(C)]
struct PlatformRoomOptions {
    _opaque: u8,
}

#[repr(C)]
struct PlatformRichPresenceOptions {
    _opaque: u8,
}

#[repr(C)]
#[derive(Debug)]
pub struct KeyValuePair {
    key: *const c_char,
    value_type: i32,
    string_value: *const c_char,
    int_value: i32,
    double_value: f64,
}
#[repr(C)]
struct PlatformMessage {
    message_type: u32,
    request_id: u64,
    payload: usize,
}

static EMPTY_ARRAY: u8 = 0;
static DATA_STORE_HANDLE: PlatformDataStore = PlatformDataStore;
static USER_ARRAY_HANDLE: PlatformUserArray = PlatformUserArray;
// These SDK handles are opaque but must be non-null and correctly typed. A
// `void` Rust export here is ABI-invalid: Windows x64 returns pointer/request
// results in RAX, and callers will otherwise consume stale register contents.
static ROOM_OPTIONS_HANDLE: PlatformRoomOptions = PlatformRoomOptions { _opaque: 0 };
static RICH_PRESENCE_OPTIONS_HANDLE: PlatformRichPresenceOptions =
    PlatformRichPresenceOptions { _opaque: 0 };
static ROOM_DATA: Mutex<BTreeMap<String, String>> = Mutex::new(BTreeMap::new());
static MESSAGE_QUEUE: Mutex<Vec<Box<PlatformMessage>>> = Mutex::new(Vec::new());
static BOOTSTRAP_MESSAGE_SENT: AtomicBool = AtomicBool::new(false);

/// Platform SDK microphone object backed by the Windows default capture
/// endpoint. Oculus Platform microphone PCM is signed 16-bit mono at 48 kHz;
/// WASAPI's shared-mode mix format is converted to that representation.
#[cfg(windows)]
struct PlatformMicrophone {
    client: windows::Win32::Media::Audio::IAudioClient,
    capture: windows::Win32::Media::Audio::IAudioCaptureClient,
    pending: Vec<i16>,
    started: bool,
}

#[cfg(windows)]
impl PlatformMicrophone {
    fn create() -> Result<Self, windows::core::Error> {
        use windows::Win32::Media::Audio::{
            AUDCLNT_SHAREMODE_SHARED, IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator,
            MMDeviceEnumerator, WAVE_FORMAT_PCM, WAVEFORMATEX, eCapture, eConsole,
        };
        use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};

        let enumerator: IMMDeviceEnumerator =
            unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? };
        let device = unsafe { enumerator.GetDefaultAudioEndpoint(eCapture, eConsole)? };
        let client: IAudioClient = unsafe { device.Activate(CLSCTX_ALL, None)? };
        // The Platform microphone contract is 48 kHz, mono, signed PCM16.
        // Shared-mode WASAPI performs any endpoint-specific format conversion,
        // preventing a 44.1 kHz capture device from sounding sped up when Echo
        // consumes it as the documented 48 kHz stream.
        let format = WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_PCM as u16,
            nChannels: 1,
            nSamplesPerSec: 48_000,
            nAvgBytesPerSec: 96_000,
            nBlockAlign: 2,
            wBitsPerSample: 16,
            cbSize: 0,
        };
        unsafe {
            client.Initialize(AUDCLNT_SHAREMODE_SHARED, 0, 0, 0, &format, None)?;
        }
        let capture: IAudioCaptureClient = unsafe { client.GetService()? };
        Ok(Self {
            client,
            capture,
            pending: Vec::new(),
            started: false,
        })
    }

    fn start(&mut self) -> Result<(), windows::core::Error> {
        if !self.started {
            unsafe { self.client.Start()? };
            self.started = true;
        }
        Ok(())
    }

    fn stop(&mut self) {
        if self.started {
            let _ = unsafe { self.client.Stop() };
            self.started = false;
            self.pending.clear();
        }
    }

    fn read_pcm(&mut self, output: &mut [i16]) -> usize {
        // Echo polls this from its game loop. Replaying samples that accumulated
        // during a long stalled frame makes voice catch up faster than real
        // time at the receiver. Keep only the newest data that fits this read.
        self.pending.clear();
        while self.pending.len() < output.len() {
            let packet_size = match unsafe { self.capture.GetNextPacketSize() } {
                Ok(size) if size != 0 => size,
                _ => break,
            };
            let mut data = core::ptr::null_mut();
            let mut frames = 0;
            let mut flags = 0;
            if unsafe {
                self.capture
                    .GetBuffer(&mut data, &mut frames, &mut flags, None, None)
            }
            .is_err()
            {
                break;
            }
            if flags & 2 == 0 {
                let samples = unsafe {
                    core::slice::from_raw_parts(
                        data.cast::<i16>(),
                        packet_size.min(frames) as usize,
                    )
                };
                self.pending.extend_from_slice(samples);
                if self.pending.len() > output.len() {
                    let excess = self.pending.len() - output.len();
                    self.pending.drain(..excess);
                }
            } else {
                self.pending.resize(self.pending.len() + frames as usize, 0);
                if self.pending.len() > output.len() {
                    let excess = self.pending.len() - output.len();
                    self.pending.drain(..excess);
                }
            }
            let _ = unsafe { self.capture.ReleaseBuffer(frames) };
        }
        let count = output.len().min(self.pending.len());
        output[..count].copy_from_slice(&self.pending[..count]);
        self.pending.drain(..count);
        count
    }
}

fn local_user() -> &'static PlatformUser {
    static USER: std::sync::OnceLock<PlatformUser> = std::sync::OnceLock::new();
    USER.get_or_init(|| PlatformUser {
        id: crate::config::user_identity().id,
    })
}

fn local_org() -> &'static PlatformUser {
    static ORG: std::sync::OnceLock<PlatformUser> = std::sync::OnceLock::new();
    ORG.get_or_init(|| PlatformUser {
        id: crate::config::user_identity().org_id,
    })
}

fn local_room() -> &'static PlatformRoom {
    static ROOM: std::sync::OnceLock<PlatformRoom> = std::sync::OnceLock::new();
    ROOM.get_or_init(|| PlatformRoom {
        // A local private room needs a stable non-zero ID, but is not a User.
        id: crate::config::user_identity().id,
        join_policy: 1, // ovrRoom_JoinPolicyEveryone
        max_users: 10,
    })
}

fn queue_message(message_type: u32, payload: usize) -> u64 {
    let request_id = NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut queue) = MESSAGE_QUEUE.lock() {
        queue.push(Box::new(PlatformMessage {
            message_type,
            request_id,
            payload,
        }));
    }
    request_id
}

fn queue_logged_in_user() -> u64 {
    queue_message(
        MESSAGE_USER_GET_LOGGED_IN_USER,
        (local_user() as *const PlatformUser) as usize,
    )
}

fn queue_room(message_type: u32) -> u64 {
    queue_message(message_type, (local_room() as *const PlatformRoom) as usize)
}

fn queue_platform_initialized() -> u64 {
    queue_message(MESSAGE_PLATFORM_INITIALIZED, 0)
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_RichPresence_GetNextDestinationArrayPage(_handle: *const c_void) -> u64 {
    crate::capi::log_call("ovr_RichPresence_GetNextDestinationArrayPage");
    0
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_RichPresence_GetDestinations() -> u64 {
    crate::capi::log_call("ovr_RichPresence_GetDestinations");
    queue_message(MESSAGE_RICH_PRESENCE_GET_DESTINATIONS, 0)
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_RichPresence_Clear() -> u64 {
    crate::capi::log_call("ovr_RichPresence_Clear");
    0
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_RichPresenceOptions_SetStartTime(_handle: *mut c_void, _value: u64) {
    crate::capi::log_call("ovr_RichPresenceOptions_SetStartTime");
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_RichPresenceOptions_SetMaxCapacity(_handle: *mut c_void, _value: u32) {
    crate::capi::log_call("ovr_RichPresenceOptions_SetMaxCapacity");
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_RichPresenceOptions_SetIsJoinable(_handle: *mut c_void, _value: bool) {
    crate::capi::log_call("ovr_RichPresenceOptions_SetIsJoinable");
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_RichPresenceOptions_SetInstanceId(
    _handle: *mut c_void,
    _value: *const c_char,
) {
    crate::capi::log_call("ovr_RichPresenceOptions_SetInstanceId");
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_RichPresenceOptions_SetExtraContext(_handle: *mut c_void, _value: i32) {
    crate::capi::log_call("ovr_RichPresenceOptions_SetExtraContext");
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_RichPresenceOptions_SetEndTime(_handle: *mut c_void, _value: u64) {
    crate::capi::log_call("ovr_RichPresenceOptions_SetEndTime");
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_RichPresenceOptions_SetDeeplinkMessageOverride(
    _handle: *mut c_void,
    _value: *const c_char,
) {
    crate::capi::log_call("ovr_RichPresenceOptions_SetDeeplinkMessageOverride");
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_RichPresenceOptions_SetCurrentCapacity(
    _handle: *mut c_void,
    _value: u32,
) {
    crate::capi::log_call("ovr_RichPresenceOptions_SetCurrentCapacity");
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_RoomOptions_Destroy(_handle: *mut c_void) {
    crate::capi::log_call("ovr_RoomOptions_Destroy");
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Net_SendPacket(
    _user_id: u64,
    _length: usize,
    _bytes: *const c_void,
    _policy: i32,
) -> bool {
    crate::capi::log_call("ovr_Net_SendPacket");
    false
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_RoomOptions_SetOrdering(_handle: *mut c_void, _value: i32) {
    crate::capi::log_call("ovr_RoomOptions_SetOrdering");
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Room_CreateAndJoinPrivate2(
    _join_policy: i32,
    _max_users: u32,
    _room_options: *const c_void,
) -> u64 {
    crate::capi::log_call("ovr_Room_CreateAndJoinPrivate2");
    queue_room(MESSAGE_ROOM_CREATE_AND_JOIN_PRIVATE2)
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Room_Get(_room_id: u64) -> u64 {
    crate::capi::log_call("ovr_Room_Get");
    queue_room(MESSAGE_ROOM_GET)
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Room_GetInvitableUsers2(_options: *const c_void) -> u64 {
    crate::capi::log_call("ovr_Room_GetInvitableUsers2");
    // OVR_Requests_Room.h: this is asynchronous and must complete with
    // ovrMessage_Room_GetInvitableUsers2. Previously it returned stale RAX
    // and emitted no completion, so Echo consumed an unrelated request.
    queue_message(MESSAGE_ROOM_GET_INVITABLE_USERS2, 0)
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Room_InviteUser(_room_id: u64, _invite_token: *const c_char) -> u64 {
    crate::capi::log_call("ovr_Room_InviteUser");
    0
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_RichPresence_Set(_options: *const c_void) -> u64 {
    crate::capi::log_call("ovr_RichPresence_Set");
    queue_message(MESSAGE_RICH_PRESENCE_SET, 0)
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Room_KickUser(_room_id: u64, _user_id: u64, _duration: i32) -> u64 {
    crate::capi::log_call("ovr_Room_KickUser");
    0
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Room_LaunchInvitableUserFlow(_room_id: u64) -> u64 {
    crate::capi::log_call("ovr_Room_LaunchInvitableUserFlow");
    0
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Room_Leave(_room_id: u64) -> u64 {
    crate::capi::log_call("ovr_Room_Leave");
    0
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Room_UpdateDataStore(
    _room_id: u64,
    _data: *mut KeyValuePair,
    _num_items: u32,
) -> u64 {
    crate::capi::log_call("ovr_Room_UpdateDataStore");
    if !_data.is_null() {
        let pairs = unsafe { core::slice::from_raw_parts(_data, _num_items as usize) };
        if let Ok(mut store) = ROOM_DATA.lock() {
            for pair in pairs {
                if pair.value_type != 0 || pair.key.is_null() || pair.string_value.is_null() {
                    continue;
                }
                let key = unsafe { CStr::from_ptr(pair.key) }
                    .to_string_lossy()
                    .into_owned();
                let value = unsafe { CStr::from_ptr(pair.string_value) }
                    .to_string_lossy()
                    .into_owned();
                store.insert(key, value);
            }
        }
    }
    queue_room(MESSAGE_ROOM_UPDATE_DATA_STORE)
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Room_UpdateMembershipLockStatus(_room_id: u64, _status: i32) -> u64 {
    crate::capi::log_call("ovr_Room_UpdateMembershipLockStatus");
    0
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Room_UpdateOwner(_room_id: u64, _user_id: u64) -> u64 {
    crate::capi::log_call("ovr_Room_UpdateOwner");
    0
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Room_UpdatePrivateRoomJoinPolicy(_room_id: u64, _policy: i32) -> u64 {
    crate::capi::log_call("ovr_Room_UpdatePrivateRoomJoinPolicy");
    0
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_User_GetAccessToken() -> u64 {
    crate::capi::log_call("ovr_User_GetAccessToken");
    queue_message(MESSAGE_USER_GET_ACCESS_TOKEN, 0)
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_User_GetLoggedInUser() -> u64 {
    crate::capi::log_call("ovr_User_GetLoggedInUser");
    queue_logged_in_user()
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_User_GetLoggedInUserFriends() -> u64 {
    crate::capi::log_call("ovr_User_GetLoggedInUserFriends");
    queue_message(MESSAGE_USER_GET_LOGGED_IN_USER_FRIENDS, 0)
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_User_GetLoggedInUserRecentlyMetUsersAndRooms(
    _options: *const c_void,
) -> u64 {
    crate::capi::log_call("ovr_User_GetLoggedInUserRecentlyMetUsersAndRooms");
    0
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_User_GetNextUserAndRoomArrayPage(_handle: *const c_void) -> u64 {
    crate::capi::log_call("ovr_User_GetNextUserAndRoomArrayPage");
    0
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_User_GetNextUserArrayPage(_handle: *const c_void) -> u64 {
    crate::capi::log_call("ovr_User_GetNextUserArrayPage");
    0
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_User_GetOrgScopedID(user_id: u64) -> u64 {
    let request_id = queue_message(
        MESSAGE_USER_GET_ORG_SCOPED_ID,
        (local_org() as *const PlatformUser) as usize,
    );
    crate::capi::log_call(&format!(
        "ovr_User_GetOrgScopedID user_id={user_id} -> request={request_id} org_handle={:p}",
        local_org()
    ));
    request_id
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_User_GetUserProof() -> u64 {
    crate::capi::log_call("ovr_User_GetUserProof");
    queue_message(MESSAGE_USER_GET_USER_PROOF, 0)
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_ApplicationLifecycle_GetLaunchDetails() -> *const c_void {
    crate::capi::log_call("ovr_ApplicationLifecycle_GetLaunchDetails");
    // No launch/deep-link was supplied by the local launcher.
    core::ptr::null()
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Net_AcceptForCurrentRoom() -> bool {
    crate::capi::log_call("ovr_Net_AcceptForCurrentRoom");
    false
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Net_CloseForCurrentRoom() {
    crate::capi::log_call("ovr_Net_CloseForCurrentRoom");
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Net_ReadPacket() -> *mut c_void {
    crate::capi::log_call("ovr_Net_ReadPacket");
    core::ptr::null_mut()
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Room_Join2(_room_id: u64, _options: *const c_void) -> u64 {
    crate::capi::log_call("ovr_Room_Join2");
    0
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_RoomOptions_Create() -> *mut c_void {
    crate::capi::log_call("ovr_RoomOptions_Create");
    (&raw const ROOM_OPTIONS_HANDLE).cast_mut().cast()
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_RichPresenceOptions_SetApiName(
    _handle: *mut c_void,
    _value: *const c_char,
) {
    crate::capi::log_call("ovr_RichPresenceOptions_SetApiName");
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_RichPresenceOptions_Destroy(_handle: *mut c_void) {
    crate::capi::log_call("ovr_RichPresenceOptions_Destroy");
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_RichPresenceOptions_Create() -> *mut c_void {
    crate::capi::log_call("ovr_RichPresenceOptions_Create");
    (&raw const RICH_PRESENCE_OPTIONS_HANDLE).cast_mut().cast()
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn ovr_FreeMessage(message: *mut c_void) {
    crate::capi::log_call("ovr_FreeMessage");
    if !message.is_null() {
        unsafe { drop(Box::from_raw(message.cast::<PlatformMessage>())) };
    }
}

/// Pop one completion from the local offline message queue.
#[unsafe(no_mangle)]
pub extern "system" fn ovr_PopMessage() -> *mut c_void {
    crate::capi::log_call("ovr_PopMessage");
    // pnsovr may initialize internally before resolving the public Ex export.
    // Seed the documented init completion followed by the local-user result.
    if !BOOTSTRAP_MESSAGE_SENT.swap(true, Ordering::AcqRel) {
        queue_platform_initialized();
        queue_logged_in_user();
    }
    let message = MESSAGE_QUEUE
        .lock()
        .ok()
        .and_then(|mut queue| (!queue.is_empty()).then(|| queue.remove(0)));
    match message {
        Some(message) => {
            let message_type = message.message_type;
            let raw = Box::into_raw(message).cast::<c_void>();
            crate::capi::log_call(&format!(
                "ovr_PopMessage delivered type={message_type:#x} handle={raw:p}"
            ));
            raw
        }
        None => core::ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_GetLoggedInUserID() -> u64 {
    crate::capi::log_call("ovr_GetLoggedInUserID");
    // Echo's offline path still requires a non-zero local principal. Zero
    // produces its "???-0" player records and later a null indirect call.
    local_user().id
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_IsPlatformInitialized() -> bool {
    crate::capi::log_call("ovr_IsPlatformInitialized");
    true
}

// pnsovr resolves these initialization functions dynamically rather than
// importing them. Report success for the local/offline shim.
#[unsafe(no_mangle)]
pub extern "system" fn ovr_PlatformInitializeWindows(_app_id: *const c_char) -> i32 {
    crate::capi::log_call("ovr_PlatformInitializeWindows");
    0 // ovrPlatformInitialize_Success
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_PlatformInitializeWindowsEx(
    _app_id: *const c_char,
    _product_version: i32,
    _major_version: i32,
) -> i32 {
    crate::capi::log_call("ovr_PlatformInitializeWindowsEx");
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn ovr_PlatformInitializeWindowsAsynchronousEx(
    _app_id: *const c_char,
    out_result: *mut i32,
    _product_version: i32,
    _major_version: i32,
) -> u64 {
    crate::capi::log_call("ovr_PlatformInitializeWindowsAsynchronousEx");
    if !out_result.is_null() {
        unsafe { *out_result = 0 };
    }
    queue_platform_initialized()
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_PlatformInitializeWindowsAsynchronous(_app_id: *const c_char) -> u64 {
    crate::capi::log_call("ovr_PlatformInitializeWindowsAsynchronous");
    queue_platform_initialized()
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_PlatformInitializeStandaloneAccessToken(_access_token: *const c_char) {
    crate::capi::log_call("ovr_PlatformInitializeStandaloneAccessToken");
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Platform_InitializeStandaloneOculus(_params: *const c_void) -> u64 {
    crate::capi::log_call("ovr_Platform_InitializeStandaloneOculus");
    queue_platform_initialized()
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn ovr_Platform_InitializeStandaloneOculusEx(
    params: *const c_void,
    out_result: *mut i32,
    _product_version: i32,
    _major_version: i32,
) -> u64 {
    crate::capi::log_call("ovr_Platform_InitializeStandaloneOculusEx");
    if !out_result.is_null() {
        unsafe {
            *out_result = 0;
        }
    }
    ovr_Platform_InitializeStandaloneOculus(params)
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_PlatformInitializeWithAccessToken(
    _app_id: u64,
    _token: *const c_char,
) -> u64 {
    crate::capi::log_call("ovr_PlatformInitializeWithAccessToken");
    queue_platform_initialized()
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn ovr_PlatformInitializeWithAccessTokenEx(
    app_id: u64,
    token: *const c_char,
    out_result: *mut i32,
    _product_version: i32,
    _major_version: i32,
) -> u64 {
    crate::capi::log_call("ovr_PlatformInitializeWithAccessTokenEx");
    if !out_result.is_null() {
        unsafe {
            *out_result = 0;
        }
    }
    ovr_PlatformInitializeWithAccessToken(app_id, token)
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_PlatformInitializeWithAccessTokenAndOptions(
    _app_id: u64,
    _token: *const c_char,
    _options: *mut KeyValuePair,
    _count: usize,
) -> u64 {
    crate::capi::log_call("ovr_PlatformInitializeWithAccessTokenAndOptions");
    queue_platform_initialized()
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Voip_Stop(_user_id: u64) {
    crate::capi::log_call("ovr_Voip_Stop");
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Voip_Start(_user_id: u64) {
    crate::capi::log_call("ovr_Voip_Start");
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Voip_SetMicrophoneMuted(_state: i32) {
    crate::capi::log_call("ovr_Voip_SetMicrophoneMuted");
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Voip_GetPCMSize(_sender_id: u64) -> usize {
    crate::capi::log_call("ovr_Voip_GetPCMSize");
    0
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Voip_GetPCM(_sender_id: u64, _buffer: *mut i16, _count: usize) -> usize {
    crate::capi::log_call("ovr_Voip_GetPCM");
    0
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Voip_GetOutputBufferMaxSize() -> usize {
    crate::capi::log_call("ovr_Voip_GetOutputBufferMaxSize");
    // This uses `size_t`. A void stub leaves RAX undefined,
    // which makes pnsovr treat an arbitrary value as an audio-buffer size.
    0
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Voip_Accept(_user_id: u64) {
    crate::capi::log_call("ovr_Voip_Accept");
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Net_SendPacketToCurrentRoom(
    _length: usize,
    _bytes: *const c_void,
    _policy: i32,
) -> bool {
    crate::capi::log_call("ovr_Net_SendPacketToCurrentRoom");
    false
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Notification_MarkAsRead(_id: u64) -> u64 {
    crate::capi::log_call("ovr_Notification_MarkAsRead");
    0
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Notification_GetRoomInvites() -> u64 {
    crate::capi::log_call("ovr_Notification_GetRoomInvites");
    queue_message(MESSAGE_NOTIFICATION_GET_ROOM_INVITES, 0)
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Notification_GetNextRoomInviteNotificationArrayPage(
    _handle: *const c_void,
) -> u64 {
    crate::capi::log_call("ovr_Notification_GetNextRoomInviteNotificationArrayPage");
    0
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_IAP_LaunchCheckoutFlow(_sku: *const c_char) -> u64 {
    crate::capi::log_call("ovr_IAP_LaunchCheckoutFlow");
    0
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_IAP_GetViewerPurchasesDurableCache() -> u64 {
    crate::capi::log_call("ovr_IAP_GetViewerPurchasesDurableCache");
    0
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_IAP_GetViewerPurchases() -> u64 {
    crate::capi::log_call("ovr_IAP_GetViewerPurchases");
    queue_message(MESSAGE_IAP_GET_VIEWER_PURCHASES, 0)
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_IAP_GetProductsBySKU(_skus: *const *const c_char, _count: i32) -> u64 {
    crate::capi::log_call("ovr_IAP_GetProductsBySKU");
    queue_message(MESSAGE_IAP_GET_PRODUCTS_BY_SKU, 0)
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_IAP_GetNextPurchaseArrayPage(_handle: *const c_void) -> u64 {
    crate::capi::log_call("ovr_IAP_GetNextPurchaseArrayPage");
    0
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_IAP_GetNextProductArrayPage(_handle: *const c_void) -> u64 {
    crate::capi::log_call("ovr_IAP_GetNextProductArrayPage");
    0
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Entitlement_GetIsViewerEntitled() -> u64 {
    crate::capi::log_call("ovr_Entitlement_GetIsViewerEntitled");
    queue_message(0x186b58b1, 0)
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Packet_GetSize(_obj: *const c_void) -> usize {
    crate::capi::log_call("ovr_Packet_GetSize");
    0
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Packet_GetSenderID(_obj: *const c_void) -> u64 {
    crate::capi::log_call("ovr_Packet_GetSenderID");
    0
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Packet_GetBytes(_obj: *const c_void) -> *const c_void {
    crate::capi::log_call("ovr_Packet_GetBytes");
    core::ptr::null()
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Packet_Free(_obj: *const c_void) {
    crate::capi::log_call("ovr_Packet_Free");
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Message_IsError(_message: *const c_void) -> bool {
    crate::capi::log_call("ovr_Message_IsError");
    false
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Message_GetUserProof(_obj: *const c_void) -> *const c_void {
    crate::capi::log_call("ovr_Message_GetUserProof");
    (&EMPTY_ARRAY as *const u8).cast()
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Message_GetUserArray(_obj: *const c_void) -> *const c_void {
    crate::capi::log_call("ovr_Message_GetUserArray");
    (&USER_ARRAY_HANDLE as *const PlatformUserArray).cast()
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_VoipEncoder_AddPCM(_obj: *const c_void, _data: *const f32, _size: u32) {
    crate::capi::log_call("ovr_VoipEncoder_AddPCM");
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_VoipEncoder_GetCompressedData(
    _obj: *const c_void,
    _buffer: *mut u8,
    _size: usize,
) -> usize {
    crate::capi::log_call("ovr_VoipEncoder_GetCompressedData");
    0
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_VoipDecoder_Decode(_obj: *const c_void, _data: *const u8, _size: usize) {
    crate::capi::log_call("ovr_VoipDecoder_Decode");
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_VoipDecoder_GetDecodedPCM(
    _obj: *const c_void,
    _buffer: *mut f32,
    _size: usize,
) -> usize {
    crate::capi::log_call("ovr_VoipDecoder_GetDecodedPCM");
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn ovr_Microphone_GetPCM(
    obj: *const c_void,
    buffer: *mut i16,
    size: usize,
) -> usize {
    crate::capi::log_call("ovr_Microphone_GetPCM");
    if obj.is_null() || buffer.is_null() || size == 0 {
        return 0;
    }
    #[cfg(windows)]
    {
        let microphone = unsafe { &mut *obj.cast_mut().cast::<PlatformMicrophone>() };
        let output = unsafe { core::slice::from_raw_parts_mut(buffer, size) };
        return microphone.read_pcm(output);
    }
    #[cfg(not(windows))]
    0
}

/// Windows Platform SDK always reports zero here; callers should read until
/// `ovr_Microphone_GetPCM` returns zero instead of sizing from this value.
#[unsafe(no_mangle)]
pub extern "system" fn ovr_Microphone_GetNumSamplesAvailable(_obj: *const c_void) -> usize {
    crate::capi::log_call("ovr_Microphone_GetNumSamplesAvailable");
    0
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Microphone_GetOutputBufferMaxSize(_obj: *const c_void) -> usize {
    crate::capi::log_call("ovr_Microphone_GetOutputBufferMaxSize");
    // One second of the documented 48 kHz mono output is a practical bounded
    // ring-buffer capacity for this pull-based WASAPI implementation.
    48_000
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn ovr_Microphone_GetPCMFloat(
    obj: *const c_void,
    buffer: *mut f32,
    size: usize,
) -> usize {
    crate::capi::log_call("ovr_Microphone_GetPCMFloat");
    if obj.is_null() || buffer.is_null() || size == 0 {
        return 0;
    }
    #[cfg(windows)]
    {
        let microphone = unsafe { &mut *obj.cast_mut().cast::<PlatformMicrophone>() };
        let mut pcm = vec![0_i16; size];
        let count = microphone.read_pcm(&mut pcm);
        let output = unsafe { core::slice::from_raw_parts_mut(buffer, count) };
        for (output, sample) in output.iter_mut().zip(pcm) {
            *output = sample as f32 / 32768.0;
        }
        return count;
    }
    #[cfg(not(windows))]
    0
}

#[deprecated(note = "use ovr_Microphone_GetPCMFloat")]
#[unsafe(no_mangle)]
pub unsafe extern "system" fn ovr_Microphone_ReadData(
    obj: *const c_void,
    buffer: *mut f32,
    size: usize,
) -> usize {
    unsafe { ovr_Microphone_GetPCMFloat(obj, buffer, size) }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn ovr_Microphone_SetAcceptableRecordingDelayHint(
    _obj: *const c_void,
    _delay_ms: usize,
) {
    crate::capi::log_call("ovr_Microphone_SetAcceptableRecordingDelayHint");
    // Shared-mode WASAPI chooses buffering; this is an advisory SDK setting.
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn ovr_Microphone_SetAudioDataAvailableCallback(
    obj: *const c_void,
    callback: Option<unsafe extern "system" fn(*mut c_void)>,
    user_data: *mut c_void,
) {
    crate::capi::log_call("ovr_Microphone_SetAudioDataAvailableCallback");
    // This pull-based implementation has no dedicated WASAPI event thread,
    // so callbacks cannot be delivered asynchronously without risking calls
    // into the game from an unknown thread. Callers can safely poll GetPCM.
    let _ = (obj, callback, user_data);
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn ovr_Microphone_Start(obj: *const c_void) {
    crate::capi::log_call("ovr_Microphone_Start");
    #[cfg(windows)]
    if !obj.is_null() {
        let microphone = unsafe { &mut *obj.cast_mut().cast::<PlatformMicrophone>() };
        if let Err(error) = microphone.start() {
            crate::capi::log_call(&format!("ovr_Microphone_Start failed: {error}"));
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn ovr_Microphone_Stop(obj: *const c_void) {
    crate::capi::log_call("ovr_Microphone_Stop");
    #[cfg(windows)]
    if !obj.is_null() {
        unsafe { &mut *obj.cast_mut().cast::<PlatformMicrophone>() }.stop();
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Voip_CreateEncoder() -> *mut c_void {
    crate::capi::log_call("ovr_Voip_CreateEncoder");
    core::ptr::null_mut()
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Voip_DestroyEncoder(_encoder: *mut c_void) {
    crate::capi::log_call("ovr_Voip_DestroyEncoder");
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Voip_CreateDecoder() -> *mut c_void {
    crate::capi::log_call("ovr_Voip_CreateDecoder");
    core::ptr::null_mut()
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Voip_DestroyDecoder(_decoder: *mut c_void) {
    crate::capi::log_call("ovr_Voip_DestroyDecoder");
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Microphone_Create() -> *mut c_void {
    crate::capi::log_call("ovr_Microphone_Create");
    #[cfg(windows)]
    {
        use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
        // COM apartments are thread-local. The game normally calls Create and
        // uses the handle from this thread; an existing apartment is harmless.
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        return PlatformMicrophone::create()
            .map(Box::new)
            .map(Box::into_raw)
            .map(|microphone| microphone.cast())
            .unwrap_or_else(|error| {
                crate::capi::log_call(&format!("ovr_Microphone_Create failed: {error}"));
                core::ptr::null_mut()
            });
    }
    #[cfg(not(windows))]
    core::ptr::null_mut()
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn ovr_Microphone_Destroy(obj: *mut c_void) {
    crate::capi::log_call("ovr_Microphone_Destroy");
    #[cfg(windows)]
    if !obj.is_null() {
        let mut microphone = unsafe { Box::from_raw(obj.cast::<PlatformMicrophone>()) };
        microphone.stop();
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Destination_GetApiName(_obj: *const c_void) -> *const c_char {
    crate::capi::log_call("ovr_Destination_GetApiName");
    c"".as_ptr()
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Destination_GetDisplayName(_obj: *const c_void) -> *const c_char {
    crate::capi::log_call("ovr_Destination_GetDisplayName");
    c"".as_ptr()
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_User_GetInviteToken(_obj: *const c_void) -> *const c_char {
    crate::capi::log_call("ovr_User_GetInviteToken");
    c"".as_ptr()
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_User_GetPresence(_obj: *const c_void) -> *const c_char {
    crate::capi::log_call("ovr_User_GetPresence");
    c"".as_ptr()
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_User_GetPresenceDeeplinkMessage(_obj: *const c_void) -> *const c_char {
    crate::capi::log_call("ovr_User_GetPresenceDeeplinkMessage");
    c"".as_ptr()
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_User_GetPresenceStatus(_obj: *const c_void) -> i32 {
    crate::capi::log_call("ovr_User_GetPresenceStatus");
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn ovr_User_GetID(user: *const c_void) -> u64 {
    let id = unsafe { user.cast::<PlatformUser>().as_ref() }.map_or(0, |user| user.id);
    crate::capi::log_call(&format!("ovr_User_GetID handle={user:p} -> {id}"));
    id
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_User_GetOculusID(_user: *const c_void) -> *const c_char {
    crate::capi::log_call("ovr_User_GetOculusID");
    crate::config::user_identity().oculus_id.as_ptr()
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_UserArray_GetElement(
    _obj: *const c_void,
    _index: usize,
) -> *const c_void {
    crate::capi::log_call("ovr_UserArray_GetElement");
    core::ptr::null()
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_UserArray_GetSize(_obj: *const c_void) -> usize {
    crate::capi::log_call("ovr_UserArray_GetSize");
    0
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_UserArray_HasNextPage(_obj: *const c_void) -> bool {
    crate::capi::log_call("ovr_UserArray_HasNextPage");
    false
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn ovr_DataStore_GetValue(
    _store: *const c_void,
    key: *const c_char,
) -> *const c_char {
    crate::capi::log_call("ovr_DataStore_GetValue");
    if key.is_null() {
        return core::ptr::null();
    }
    let key = unsafe { CStr::from_ptr(key) }.to_string_lossy();
    // The SDK owns returned strings for as long as the handle is valid. Leak
    // replacement values deliberately: this process-local store lives for the
    // game process and must not hand C callers dangling pointers.
    ROOM_DATA
        .lock()
        .ok()
        .and_then(|store| store.get(key.as_ref()).cloned())
        .map(|value| {
            std::ffi::CString::new(value)
                .unwrap()
                .into_raw()
                .cast_const()
        })
        .unwrap_or(core::ptr::null())
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn ovr_DataStore_Contains(
    _store: *const c_void,
    key: *const c_char,
) -> u32 {
    crate::capi::log_call("ovr_DataStore_Contains");
    if key.is_null() {
        return 0;
    }
    let key = unsafe { CStr::from_ptr(key) }.to_string_lossy();
    ROOM_DATA
        .lock()
        .is_ok_and(|store| store.contains_key(key.as_ref())) as u32
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_DataStore_GetNumKeys(_store: *const c_void) -> usize {
    crate::capi::log_call("ovr_DataStore_GetNumKeys");
    ROOM_DATA.lock().map_or(0, |store| store.len())
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_DataStore_GetKey(_store: *const c_void, index: i32) -> *const c_char {
    crate::capi::log_call("ovr_DataStore_GetKey");
    if index < 0 {
        return core::ptr::null();
    }
    ROOM_DATA
        .lock()
        .ok()
        .and_then(|store| store.keys().nth(index as usize).cloned())
        .map(|key| std::ffi::CString::new(key).unwrap().into_raw().cast_const())
        .unwrap_or(core::ptr::null())
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_DestinationArray_GetElement(
    _obj: *const c_void,
    _index: usize,
) -> *const c_void {
    crate::capi::log_call("ovr_DestinationArray_GetElement");
    core::ptr::null()
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_DestinationArray_GetSize(_obj: *const c_void) -> usize {
    crate::capi::log_call("ovr_DestinationArray_GetSize");
    0
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_DestinationArray_HasNextPage(_obj: *const c_void) -> bool {
    crate::capi::log_call("ovr_DestinationArray_HasNextPage");
    false
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Error_GetMessage(_obj: *const c_void) -> *const c_char {
    crate::capi::log_call("ovr_Error_GetMessage");
    c"".as_ptr()
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Error_GetCode(_obj: *const c_void) -> i32 {
    crate::capi::log_call("ovr_Error_GetCode");
    0
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Error_GetHttpCode(_obj: *const c_void) -> i32 {
    crate::capi::log_call("ovr_Error_GetHttpCode");
    0
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_LaunchDetails_GetDeeplinkMessage(_obj: *const c_void) -> *const c_char {
    crate::capi::log_call("ovr_LaunchDetails_GetDeeplinkMessage");
    c"".as_ptr()
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_LaunchDetails_GetDestinationApiName(
    _obj: *const c_void,
) -> *const c_char {
    crate::capi::log_call("ovr_LaunchDetails_GetDestinationApiName");
    c"".as_ptr()
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_LaunchDetails_GetLaunchSource(_obj: *const c_void) -> *const c_char {
    crate::capi::log_call("ovr_LaunchDetails_GetLaunchSource");
    c"".as_ptr()
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_LaunchDetails_GetRoomID(_obj: *const c_void) -> u64 {
    crate::capi::log_call("ovr_LaunchDetails_GetRoomID");
    0
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_LaunchDetails_GetLaunchType(_obj: *const c_void) -> i32 {
    crate::capi::log_call("ovr_LaunchDetails_GetLaunchType");
    0
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Room_GetDataStore(_room: *const c_void) -> *const c_void {
    crate::capi::log_call("ovr_Room_GetDataStore");
    (&DATA_STORE_HANDLE as *const PlatformDataStore).cast()
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn ovr_Room_GetID(room: *const c_void) -> u64 {
    crate::capi::log_call("ovr_Room_GetID");
    unsafe { room.cast::<PlatformRoom>().as_ref() }.map_or(0, |room| room.id)
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Room_GetIsMembershipLocked(_obj: *const c_void) -> bool {
    crate::capi::log_call("ovr_Room_GetIsMembershipLocked");
    false
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn ovr_Room_GetJoinPolicy(room: *const c_void) -> i32 {
    crate::capi::log_call("ovr_Room_GetJoinPolicy");
    unsafe { room.cast::<PlatformRoom>().as_ref() }.map_or(0, |room| room.join_policy)
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Room_GetOwner(_obj: *const c_void) -> *const c_void {
    crate::capi::log_call("ovr_Room_GetOwner");
    (local_user() as *const PlatformUser).cast()
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Room_GetUsers(_obj: *const c_void) -> *const c_void {
    crate::capi::log_call("ovr_Room_GetUsers");
    (&USER_ARRAY_HANDLE as *const PlatformUserArray).cast()
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn ovr_OrgScopedID_GetID(obj: *const c_void) -> u64 {
    let id = unsafe { obj.cast::<PlatformUser>().as_ref() }.map_or(0, |org| org.id);
    crate::capi::log_call(&format!("ovr_OrgScopedID_GetID handle={obj:p} -> {id}"));
    id
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Product_GetDescription(_obj: *const c_void) -> *const c_char {
    crate::capi::log_call("ovr_Product_GetDescription");
    c"".as_ptr()
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Product_GetFormattedPrice(_obj: *const c_void) -> *const c_char {
    crate::capi::log_call("ovr_Product_GetFormattedPrice");
    c"".as_ptr()
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Product_GetName(_obj: *const c_void) -> *const c_char {
    crate::capi::log_call("ovr_Product_GetName");
    c"".as_ptr()
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Product_GetSKU(_obj: *const c_void) -> *const c_char {
    crate::capi::log_call("ovr_Product_GetSKU");
    c"".as_ptr()
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_ProductArray_GetElement(
    _obj: *const c_void,
    _index: usize,
) -> *const c_void {
    crate::capi::log_call("ovr_ProductArray_GetElement");
    core::ptr::null()
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_ProductArray_GetSize(_obj: *const c_void) -> usize {
    crate::capi::log_call("ovr_ProductArray_GetSize");
    0
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_ProductArray_HasNextPage(_obj: *const c_void) -> bool {
    crate::capi::log_call("ovr_ProductArray_HasNextPage");
    false
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Purchase_GetSKU(_obj: *const c_void) -> *const c_char {
    crate::capi::log_call("ovr_Purchase_GetSKU");
    c"".as_ptr()
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_PurchaseArray_GetElement(
    _obj: *const c_void,
    _index: usize,
) -> *const c_void {
    crate::capi::log_call("ovr_PurchaseArray_GetElement");
    core::ptr::null()
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_PurchaseArray_GetSize(_obj: *const c_void) -> usize {
    crate::capi::log_call("ovr_PurchaseArray_GetSize");
    0
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_PurchaseArray_HasNextPage(_obj: *const c_void) -> bool {
    crate::capi::log_call("ovr_PurchaseArray_HasNextPage");
    false
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_RoomInviteNotification_GetID(_obj: *const c_void) -> u64 {
    crate::capi::log_call("ovr_RoomInviteNotification_GetID");
    0
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_RoomInviteNotification_GetRoomID(_obj: *const c_void) -> u64 {
    crate::capi::log_call("ovr_RoomInviteNotification_GetRoomID");
    0
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_RoomInviteNotification_GetSentTime(_obj: *const c_void) -> u64 {
    crate::capi::log_call("ovr_RoomInviteNotification_GetSentTime");
    0
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_RoomInviteNotificationArray_GetElement(
    _obj: *const c_void,
    _index: usize,
) -> *const c_void {
    crate::capi::log_call("ovr_RoomInviteNotificationArray_GetElement");
    core::ptr::null()
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_RoomInviteNotificationArray_GetSize(_obj: *const c_void) -> usize {
    crate::capi::log_call("ovr_RoomInviteNotificationArray_GetSize");
    0
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_RoomInviteNotificationArray_HasNextPage(_obj: *const c_void) -> bool {
    crate::capi::log_call("ovr_RoomInviteNotificationArray_HasNextPage");
    false
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_UserAndRoom_GetUser(_obj: *const c_void) -> *const c_void {
    crate::capi::log_call("ovr_UserAndRoom_GetUser");
    core::ptr::null()
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_UserAndRoomArray_GetElement(
    _obj: *const c_void,
    _index: usize,
) -> *const c_void {
    crate::capi::log_call("ovr_UserAndRoomArray_GetElement");
    core::ptr::null()
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_UserAndRoomArray_GetSize(_obj: *const c_void) -> usize {
    crate::capi::log_call("ovr_UserAndRoomArray_GetSize");
    0
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_UserAndRoomArray_HasNextPage(_obj: *const c_void) -> bool {
    crate::capi::log_call("ovr_UserAndRoomArray_HasNextPage");
    false
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_UserProof_GetNonce(_obj: *const c_void) -> *const c_char {
    crate::capi::log_call("ovr_UserProof_GetNonce");
    c"local-user-proof".as_ptr()
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn ovr_Message_GetRoom(message: *const c_void) -> *const c_void {
    crate::capi::log_call("ovr_Message_GetRoom");
    unsafe { message.cast::<PlatformMessage>().as_ref() }.map_or(core::ptr::null(), |message| {
        message.payload as *const c_void
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Message_GetRoomInviteNotification(_obj: *const c_void) -> *const c_void {
    crate::capi::log_call("ovr_Message_GetRoomInviteNotification");
    core::ptr::null()
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Message_GetRoomInviteNotificationArray(
    _obj: *const c_void,
) -> *const c_void {
    crate::capi::log_call("ovr_Message_GetRoomInviteNotificationArray");
    (&EMPTY_ARRAY as *const u8).cast()
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Message_GetUserAndRoomArray(_obj: *const c_void) -> *const c_void {
    crate::capi::log_call("ovr_Message_GetUserAndRoomArray");
    core::ptr::null()
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Message_GetDestinationArray(_obj: *const c_void) -> *const c_void {
    crate::capi::log_call("ovr_Message_GetDestinationArray");
    (&EMPTY_ARRAY as *const u8).cast()
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Message_GetError(_obj: *const c_void) -> *const c_void {
    crate::capi::log_call("ovr_Message_GetError");
    core::ptr::null()
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn ovr_Message_GetOrgScopedID(message: *const c_void) -> *const c_void {
    let handle = unsafe { message.cast::<PlatformMessage>().as_ref() }
        .and_then(|message| {
            (message.message_type == MESSAGE_USER_GET_ORG_SCOPED_ID)
                .then_some(message.payload as *const c_void)
        })
        .unwrap_or_else(|| (local_org() as *const PlatformUser).cast());
    crate::capi::log_call(&format!(
        "ovr_Message_GetOrgScopedID message={message:p} -> {handle:p}"
    ));
    handle
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Message_GetProductArray(_obj: *const c_void) -> *const c_void {
    crate::capi::log_call("ovr_Message_GetProductArray");
    (&EMPTY_ARRAY as *const u8).cast()
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Message_GetPurchase(_obj: *const c_void) -> *const c_void {
    crate::capi::log_call("ovr_Message_GetPurchase");
    core::ptr::null()
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Message_GetPurchaseArray(_obj: *const c_void) -> *const c_void {
    crate::capi::log_call("ovr_Message_GetPurchaseArray");
    (&EMPTY_ARRAY as *const u8).cast()
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn ovr_Message_GetRequestID(message: *const c_void) -> u64 {
    crate::capi::log_call("ovr_Message_GetRequestID");
    unsafe { message.cast::<PlatformMessage>().as_ref() }.map_or(0, |message| message.request_id)
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Message_GetString(_obj: *const c_void) -> *const c_char {
    crate::capi::log_call("ovr_Message_GetString");
    c"local-access-token".as_ptr()
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn ovr_Message_GetType(message: *const c_void) -> u32 {
    crate::capi::log_call("ovr_Message_GetType");
    unsafe { message.cast::<PlatformMessage>().as_ref() }.map_or(0, |message| message.message_type)
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn ovr_Message_GetUser(message: *const c_void) -> *mut c_void {
    let user = unsafe { message.cast::<PlatformMessage>().as_ref() }
        .map_or(core::ptr::null_mut(), |message| {
            message.payload as *mut c_void
        });
    crate::capi::log_call(&format!(
        "ovr_Message_GetUser message={message:p} -> {user:p}"
    ));
    user
}

#[unsafe(no_mangle)]
pub extern "system" fn ovrKeyValuePair_makeString(
    key: *const c_char,
    value: *const c_char,
) -> KeyValuePair {
    crate::capi::log_call("ovrKeyValuePair_makeString");
    KeyValuePair {
        key,
        value_type: 0, // ovrKeyValuePairType_String
        string_value: value,
        int_value: 0,
        double_value: 0.0,
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn ovrID_FromString(_out_id: *mut u64, _in_id: *const c_char) -> bool {
    crate::capi::log_call("ovrID_FromString");
    false
}

#[unsafe(no_mangle)]
pub extern "system" fn ovrLaunchType_ToString(_value: i32) -> *const c_char {
    crate::capi::log_call("ovrLaunchType_ToString");
    c"Unknown".as_ptr()
}

#[unsafe(no_mangle)]
pub extern "system" fn ovrRoomJoinPolicy_ToString(_value: i32) -> *const c_char {
    crate::capi::log_call("ovrRoomJoinPolicy_ToString");
    c"Unknown".as_ptr()
}

#[unsafe(no_mangle)]
pub extern "system" fn ovrPlatformInitializeResult_ToString(
    result: i32,
) -> *const core::ffi::c_char {
    crate::capi::log_call(&format!(
        "ovrPlatformInitializeResult_ToString result={result}"
    ));
    match result {
        0 => c"Success".as_ptr(),
        -1 => c"Uninitialized".as_ptr(),
        -2 => c"PreLoaded".as_ptr(),
        -3 => c"FileInvalid".as_ptr(),
        -4 => c"SignatureInvalid".as_ptr(),
        -5 => c"UnableToVerify".as_ptr(),
        -6 => c"VersionMismatch".as_ptr(),
        _ => c"Unknown".as_ptr(),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Voip_GetPCMFloat(_id: u64, _buffer: *mut f32, _size: usize) -> usize { 0 }
