/* ABI smoke test against the built Windows DLLs, run through Proton.
 * Exit 0 means local request ordering, message ownership, upstream forwarding,
 * and the added PCM export behaved as expected. Other codes identify a stage.
 */
#include <stdbool.h>
#include <stdint.h>
#include "../compat/vr_input.h"
#include "../compat/vr_hmd.h"
#include "../compat/vr_motion.h"

__declspec(dllimport) void *LoadLibraryA(const char *);
__declspec(dllimport) void *GetProcAddress(void *, const char *);
__declspec(dllimport) void ExitProcess(uint32_t);
int _fltused = 0; /* MSVC ABI marker; no CRT helpers are needed. */

#define RESOLVE(type, variable, name) \
    type variable = (type)GetProcAddress(module, name); \
    if (!variable) ExitProcess(2)

void mainCRTStartup(void)
{
    void *module = LoadLibraryA("LibOVRPlatform64_1.dll");
    if (!module)
        ExitProcess(1);
    typedef uint64_t (*Request)(void);
    typedef void *(*Pop)(void);
    typedef void (*Free)(void *);
    typedef uint32_t (*Type)(const void *);
    typedef uint64_t (*RequestID)(const void *);
    typedef bool (*IsError)(const void *);
    typedef void *(*GetUser)(const void *);
    typedef uint64_t (*UserID)(const void *);
    typedef uint64_t (*PCM)(uint64_t, float *, uint64_t);
    RESOLVE(Request, entitlement, "ovr_Entitlement_GetIsViewerEntitled");
    RESOLVE(Request, user_request, "ovr_User_GetLoggedInUser");
    RESOLVE(Pop, pop, "ovr_PopMessage");
    RESOLVE(Free, release, "ovr_FreeMessage");
    RESOLVE(Type, type, "ovr_Message_GetType");
    RESOLVE(RequestID, request_id, "ovr_Message_GetRequestID");
    RESOLVE(IsError, is_error, "ovr_Message_IsError");
    RESOLVE(GetUser, get_user, "ovr_Message_GetUser");
    RESOLVE(UserID, user_id, "ovr_User_GetID");
    RESOLVE(PCM, pcm, "ovr_Voip_GetPCMFloat");

    uint64_t first = entitlement(), second = entitlement();
    if (!first || !second || first == second)
        ExitProcess(3);
    uint64_t expected[] = {first, second};
    for (unsigned i = 0; i < 2; ++i) {
        void *message = pop();
        if (!message || type(message) != 0x186b58b1 ||
            request_id(message) != expected[i] || is_error(message))
            ExitProcess(4);
        release(message);
    }
    /* This request comes from the upstream DLL, whose objects must be passed
     * back to its accessors and allocator. Ignore its bootstrap notifications. */
    uint64_t requested = user_request();
    bool found = false;
    if (!requested)
        ExitProcess(5);
    for (unsigned i = 0; i < 16; ++i) {
        void *message = pop();
        if (!message)
            break;
        if (request_id(message) == requested) {
            if (type(message) != 0x436f345d || is_error(message) ||
                !get_user(message) || !user_id(get_user(message)))
                ExitProcess(6);
            found = true;
        }
        release(message);
    }
    if (!found)
        ExitProcess(7);
    float samples[] = {0.25f, -0.5f};
    if (pcm(1, samples, 2) != 0 || samples[0] != 0.25f || samples[1] != -0.5f)
        ExitProcess(8);
    release(0);
    module = LoadLibraryA("LibOVRRT64_1.dll");
    if (!module) ExitProcess(9);
    typedef int32_t (*GetInput)(void *, uint32_t, InputState *);
    typedef uint32_t (*Connected)(void *);
    RESOLVE(GetInput, get_input, "ovr_GetInputState");
    RESOLVE(Connected, connected, "ovr_GetConnectedControllerTypes");
    InputState input;
    if (get_input(0, UINT32_MAX, 0) != -1005 || connected(0) != 3)
        ExitProcess(10);
    if (get_input(0, UINT32_MAX, &input) != 0 || input.controller_type != 3)
        ExitProcess(11);
    if (get_input(0, 1, &input) != 0 || input.controller_type != 1)
        ExitProcess(12);
    if (get_input(0, 0x10, &input) != 0 || input.controller_type != 0)
        ExitProcess(13);
    typedef PublicHmd (*GetHmd)(void *);
    RESOLVE(GetHmd, get_hmd, "ovr_GetHmdDesc");
    PublicHmd hmd = get_hmd(0);
    if (hmd.type != 16 || hmd.product[0] != 'O' || hmd.refresh_rate != 90.0f ||
        hmd.default_fov[0].left != 1.0f || hmd.resolution.w != 3664)
        ExitProcess(14);
    typedef MotionTracking (*GetTracking)(void *, double, unsigned char);
    RESOLVE(GetTracking, get_tracking, "ovr_GetTrackingState");
    MotionTracking tracking = get_tracking(0, 12.5, 0);
    if (tracking.flags != 0xe3 || tracking.head.pose.orientation.w != 1 ||
        tracking.head.time != 12.5 || tracking.origin.orientation.w != 1)
        ExitProcess(15);
    typedef int32_t (*GetPoses)(void *, const int32_t *, int32_t, double, MotionState *);
    RESOLVE(GetPoses, get_poses, "ovr_GetDevicePoses");
    int32_t types[] = {4, 1, 2};
    MotionState poses[3];
    if (get_poses(0, types, 3, 20, poses) != 0 || poses[1].pose.orientation.w != 1 ||
        poses[1].time != 20 || get_poses(0, 0, 1, 20, poses) != -1005)
        ExitProcess(16);
    ExitProcess(0);
}
