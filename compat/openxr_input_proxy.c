/* Local adapter for the pinned LibOVR shim: use OpenXR aim poses, as ReviveXR
 * does for Oculus controller poses. Keep path interning and all other API
 * behavior intact; replace only hand grip bindings in a copied suggestion.
 * LE2_CONTROLLER_POSE=grip bypasses the replacement for comparison.
 */
#define XR_NO_PROTOTYPES
#include <openxr.h>
#include "vr_motion.h"
#define IMPORT __declspec(dllimport)
IMPORT void *GetModuleHandleA(const char *);
IMPORT void *LoadLibraryA(const char *);
IMPORT void *GetProcAddress(void *, const char *);
IMPORT void *GetProcessHeap(void);
IMPORT void *HeapAlloc(void *, uint32_t, uint64_t);
IMPORT int HeapFree(void *, uint32_t, void *);
IMPORT uint32_t GetEnvironmentVariableA(const char *, char *, uint32_t);
IMPORT void OutputDebugStringA(const char *);
IMPORT void AcquireSRWLockExclusive(void **);
IMPORT void ReleaseSRWLockExclusive(void **);
int _fltused = 0;

static void *motion_lock;
static XrPath hand_paths[2];
static XrSpace hand_spaces[2];
typedef struct {
    MotionPose pose;
    MotionVector linear, angular;
    int present;
} VelocitySample;
static VelocitySample samples[2][4];
static unsigned sample_index[2];
static uint32_t logged_velocity_flags[2] = {UINT32_MAX, UINT32_MAX};

static int finite_vector(MotionVector value)
{
    const float limit = 3.402823466e+38f;
    return value.x >= -limit && value.x <= limit &&
           value.y >= -limit && value.y <= limit && value.z >= -limit && value.z <= limit;
}

static int same_pose(const MotionPose *a, const MotionPose *b)
{
    return a->position.x == b->position.x && a->position.y == b->position.y &&
        a->position.z == b->position.z && a->orientation.x == b->orientation.x &&
        a->orientation.y == b->orientation.y && a->orientation.z == b->orientation.z &&
        a->orientation.w == b->orientation.w;
}

/* Private bridge to the LibOVR wrapper. Match the exact pose returned by the
 * upstream shim, including a short history to tolerate its frame/input threads.
 */
__declspec(dllexport) int le2_GetHandVelocity(unsigned hand, const MotionPose *pose,
    MotionVector *linear, MotionVector *angular)
{
    if (hand > 1 || !pose || !linear || !angular) return 0;
    int found = 0;
    AcquireSRWLockExclusive(&motion_lock);
    for (unsigned i = 0; i < 4; ++i) {
        VelocitySample *sample = &samples[hand][(sample_index[hand] - i) & 3];
        if (sample->present && same_pose(&sample->pose, pose)) {
            *linear = sample->linear; *angular = sample->angular;
            found = 1; break;
        }
    }
    ReleaseSRWLockExclusive(&motion_lock);
    return found;
}

static int equal(const char *a, const char *b)
{
    if (!a || !b) return 0;
    while (*a && *a == *b) { ++a; ++b; }
    return *a == *b;
}

static void *upstream(const char *name)
{
    void *module = GetModuleHandleA("openxr_loader_upstream.dll");
    if (!module) module = LoadLibraryA("openxr_loader_upstream.dll");
    return module ? GetProcAddress(module, name) : 0;
}

__declspec(dllexport) XrResult XRAPI_CALL xrCreateActionSpace(
    XrSession session, const XrActionSpaceCreateInfo *info, XrSpace *space)
{
    PFN_xrCreateActionSpace create = (PFN_xrCreateActionSpace)upstream("xrCreateActionSpace");
    if (!create) return XR_ERROR_RUNTIME_UNAVAILABLE;
    XrResult result = create(session, info, space);
    if (XR_SUCCEEDED(result) && info && space) {
        AcquireSRWLockExclusive(&motion_lock);
        for (unsigned hand = 0; hand < 2; ++hand) {
            if (hand_paths[hand] && info->subactionPath == hand_paths[hand]) {
                hand_spaces[hand] = *space;
                for (unsigned i = 0; i < 4; ++i) {
                    samples[hand][i].present = 0;
                    samples[hand][i].pose = (MotionPose){ .orientation.w = 1 };
                }
                sample_index[hand] = 0;
                logged_velocity_flags[hand] = UINT32_MAX;
            }
        }
        ReleaseSRWLockExclusive(&motion_lock);
    }
    return result;
}

__declspec(dllexport) XrResult XRAPI_CALL xrDestroySpace(XrSpace space)
{
    PFN_xrDestroySpace destroy = (PFN_xrDestroySpace)upstream("xrDestroySpace");
    if (!destroy) return XR_ERROR_RUNTIME_UNAVAILABLE;
    XrResult result = destroy(space);
    if (XR_SUCCEEDED(result)) {
        AcquireSRWLockExclusive(&motion_lock);
        for (unsigned hand = 0; hand < 2; ++hand) if (space == hand_spaces[hand]) {
            hand_spaces[hand] = XR_NULL_HANDLE;
            for (unsigned i = 0; i < 4; ++i) samples[hand][i].present = 0;
        }
        ReleaseSRWLockExclusive(&motion_lock);
    }
    return result;
}

__declspec(dllexport) XrResult XRAPI_CALL xrLocateSpace(
    XrSpace space, XrSpace base, XrTime time, XrSpaceLocation *location)
{
    PFN_xrLocateSpace locate = (PFN_xrLocateSpace)upstream("xrLocateSpace");
    if (!locate) return XR_ERROR_RUNTIME_UNAVAILABLE;
    int hand = -1;
    AcquireSRWLockExclusive(&motion_lock);
    for (unsigned h = 0; h < 2; ++h) if (space && space == hand_spaces[h]) hand = (int)h;
    ReleaseSRWLockExclusive(&motion_lock);
    if (hand < 0 || !location) return locate(space, base, time, location);
    XrSpaceVelocity extra = { .type = XR_TYPE_SPACE_VELOCITY };
    XrSpaceVelocity *velocity = 0;
    for (XrBaseOutStructure *next = location->next; next; next = next->next)
        if (next->type == XR_TYPE_SPACE_VELOCITY) velocity = (XrSpaceVelocity *)next;
    void *original_next = location->next;
    if (!velocity) {
        extra.next = original_next; location->next = &extra; velocity = &extra;
    }
    XrResult result = locate(space, base, time, location);
    location->next = original_next;
    AcquireSRWLockExclusive(&motion_lock);
    if (hand_spaces[hand] == space) {
        VelocitySample sample = samples[hand][sample_index[hand] & 3];
        sample.linear = sample.angular = (MotionVector){0, 0, 0};
        uint32_t valid = 0;
        if (XR_SUCCEEDED(result)) {
            /* Match upstream's stable_pose behavior for invalid components. */
            MotionVector position = {location->pose.position.x, location->pose.position.y, location->pose.position.z};
            float norm = location->pose.orientation.x * location->pose.orientation.x +
                         location->pose.orientation.y * location->pose.orientation.y +
                         location->pose.orientation.z * location->pose.orientation.z +
                         location->pose.orientation.w * location->pose.orientation.w;
            int position_ok = (location->locationFlags & XR_SPACE_LOCATION_POSITION_VALID_BIT) && finite_vector(position);
            int orientation_ok = (location->locationFlags & XR_SPACE_LOCATION_ORIENTATION_VALID_BIT) && norm > .5f && norm < 1.5f;
            if (position_ok) sample.pose.position = position;
            if (orientation_ok)
                sample.pose.orientation = (MotionQuat){location->pose.orientation.x,
                    location->pose.orientation.y, location->pose.orientation.z, location->pose.orientation.w};
            MotionVector linear = {velocity->linearVelocity.x, velocity->linearVelocity.y, velocity->linearVelocity.z};
            MotionVector angular = {velocity->angularVelocity.x, velocity->angularVelocity.y, velocity->angularVelocity.z};
            if (position_ok &&
                (velocity->velocityFlags & XR_SPACE_VELOCITY_LINEAR_VALID_BIT) && finite_vector(linear)) {
                sample.linear = linear; valid |= 1;
            }
            if (orientation_ok &&
                (velocity->velocityFlags & XR_SPACE_VELOCITY_ANGULAR_VALID_BIT) && finite_vector(angular)) {
                sample.angular = angular; valid |= 2;
            }
        }
        sample.present = 1;
        samples[hand][++sample_index[hand] & 3] = sample;
        if (logged_velocity_flags[hand] != valid) {
            OutputDebugStringA(hand == 0 ? "LE2 velocity: left hand sample status changed\n" :
                                           "LE2 velocity: right hand sample status changed\n");
            OutputDebugStringA(valid == 3 ? "LE2 velocity: native linear and angular valid\n" :
                                           "LE2 velocity: unavailable components zeroed\n");
            logged_velocity_flags[hand] = valid;
        }
    }
    ReleaseSRWLockExclusive(&motion_lock);
    return result;
}

__declspec(dllexport) XrResult XRAPI_CALL xrSuggestInteractionProfileBindings(
    XrInstance instance, const XrInteractionProfileSuggestedBinding *suggestion)
{
    PFN_xrSuggestInteractionProfileBindings suggest =
        (PFN_xrSuggestInteractionProfileBindings)upstream("xrSuggestInteractionProfileBindings");
    PFN_xrStringToPath to_path = (PFN_xrStringToPath)upstream("xrStringToPath");
    if (!suggest || !to_path) return XR_ERROR_RUNTIME_UNAVAILABLE;
    XrPath paths[2];
    if (XR_SUCCEEDED(to_path(instance, "/user/hand/left", &paths[0])) &&
        XR_SUCCEEDED(to_path(instance, "/user/hand/right", &paths[1]))) {
        AcquireSRWLockExclusive(&motion_lock);
        hand_paths[0] = paths[0]; hand_paths[1] = paths[1];
        ReleaseSRWLockExclusive(&motion_lock);
    }
    char mode[16] = {0};
    GetEnvironmentVariableA("LE2_CONTROLLER_POSE", mode, sizeof(mode));
    if (equal(mode, "grip")) {
        OutputDebugStringA("LE2 pose: retaining upstream grip bindings (comparison mode)\n");
        return suggest(instance, suggestion);
    }
    if (!suggestion || !suggestion->suggestedBindings || !suggestion->countSuggestedBindings)
        return suggest(instance, suggestion);
    XrPath grip[2], aim[2];
    const char *grip_names[] = {"/user/hand/left/input/grip/pose", "/user/hand/right/input/grip/pose"};
    const char *aim_names[] = {"/user/hand/left/input/aim/pose", "/user/hand/right/input/aim/pose"};
    for (unsigned hand = 0; hand < 2; ++hand) {
        XrResult result = to_path(instance, grip_names[hand], &grip[hand]);
        if (XR_FAILED(result)) return result;
        result = to_path(instance, aim_names[hand], &aim[hand]);
        if (XR_FAILED(result)) return result;
    }
    XrActionSuggestedBinding *bindings = HeapAlloc(GetProcessHeap(), 0,
        (uint64_t)suggestion->countSuggestedBindings * sizeof(*bindings));
    if (!bindings) return XR_ERROR_OUT_OF_MEMORY;
    unsigned changed = 0;
    for (uint32_t i = 0; i < suggestion->countSuggestedBindings; ++i) {
        bindings[i] = suggestion->suggestedBindings[i];
        for (unsigned hand = 0; hand < 2; ++hand) {
            if (bindings[i].binding == grip[hand]) {
                bindings[i].binding = aim[hand];
                ++changed;
            }
        }
    }
    XrInteractionProfileSuggestedBinding copy = *suggestion;
    copy.suggestedBindings = bindings;
    XrResult result = suggest(instance, &copy);
    if (changed && XR_SUCCEEDED(result))
        OutputDebugStringA("LE2 pose: runtime accepted hand grip -> aim bindings\n");
    HeapFree(GetProcessHeap(), 0, bindings);
    return result;
}

__declspec(dllexport) XrResult XRAPI_CALL xrGetInstanceProcAddr(
    XrInstance instance, const char *name, PFN_xrVoidFunction *function)
{
    PFN_xrGetInstanceProcAddr get_proc = (PFN_xrGetInstanceProcAddr)upstream("xrGetInstanceProcAddr");
    if (!get_proc) return XR_ERROR_RUNTIME_UNAVAILABLE;
    XrResult result = get_proc(instance, name, function);
    if (XR_SUCCEEDED(result) && function) {
        if (equal(name, "xrSuggestInteractionProfileBindings"))
            *function = (PFN_xrVoidFunction)xrSuggestInteractionProfileBindings;
        else if (equal(name, "xrGetInstanceProcAddr"))
            *function = (PFN_xrVoidFunction)xrGetInstanceProcAddr;
        else if (equal(name, "xrCreateActionSpace"))
            *function = (PFN_xrVoidFunction)xrCreateActionSpace;
        else if (equal(name, "xrLocateSpace"))
            *function = (PFN_xrVoidFunction)xrLocateSpace;
        else if (equal(name, "xrDestroySpace"))
            *function = (PFN_xrVoidFunction)xrDestroySpace;
    }
    return result;
}
