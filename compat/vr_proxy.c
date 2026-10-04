/* Forward the pinned runtime; correct CAPI input/HMD data and hand velocities.
 * Controller poses and gestures are preserved; no extra smoothing is applied.
 */
#include "vr_input.h"
#include "vr_hmd.h"
#include "vr_motion.h"
#define IMPORT __declspec(dllimport)
IMPORT void *GetModuleHandleA(const char *);
IMPORT void *LoadLibraryA(const char *);
IMPORT void *GetProcAddress(void *, const char *);
IMPORT void OutputDebugStringA(const char *);
IMPORT void AcquireSRWLockExclusive(void **);
IMPORT void ReleaseSRWLockExclusive(void **);
IMPORT uint32_t GetEnvironmentVariableA(const char *, char *, uint32_t);
int _fltused = 0;

/* Compiler-generated initialization of the returned HMD structure. */
void *memset(void *destination, int value, size_t count)
{
    volatile unsigned char *bytes = destination;
    for (size_t i = 0; i < count; ++i) bytes[i] = (unsigned char)value;
    return destination;
}

static void *log_lock;
static uint32_t last_request, last_buttons, last_touches, calls;
static int last_motion_match[2] = {-1, -1};

static void correct_velocity(unsigned hand, MotionState *state)
{
    char mode[16] = {0};
    GetEnvironmentVariableA("LE2_VELOCITY", mode, sizeof(mode));
    if (mode[0] == 'l' && mode[1] == 'e' && mode[2] == 'g' && mode[3] == 'a' &&
        mode[4] == 'c' && mode[5] == 'y' && mode[6] == 0) return;
    void *module = GetModuleHandleA("openxr_loader.dll");
    GetHandVelocity get = module ? (GetHandVelocity)GetProcAddress(module, "le2_GetHandVelocity") : 0;
    int matched = get ? get(hand, &state->pose, &state->linear_velocity, &state->angular_velocity) : 0;
    AcquireSRWLockExclusive(&log_lock);
    if (last_motion_match[hand] != matched) {
        OutputDebugStringA(hand == 0 ? "LE2 velocity: left CAPI source changed\n" :
                                       "LE2 velocity: right CAPI source changed\n");
        OutputDebugStringA(matched ? "LE2 velocity: CAPI uses matching native sample\n" :
                                    "LE2 velocity: CAPI has no matching native sample; upstream fallback\n");
        last_motion_match[hand] = matched;
    }
    ReleaseSRWLockExclusive(&log_lock);
}

__declspec(dllexport) MotionTracking ovr_GetTrackingState(void *session, double time, unsigned char marker)
{
    void *module = GetModuleHandleA("LibOVRRT64_upstream.dll");
    if (!module) module = LoadLibraryA("LibOVRRT64_upstream.dll");
    typedef MotionTracking (*GetTracking)(void *, double, unsigned char);
    GetTracking get = module ? (GetTracking)GetProcAddress(module, "ovr_GetTrackingState") : 0;
    MotionTracking state = {0};
    if (get) {
        state = get(session, time, marker);
        correct_velocity(0, &state.hands[0]);
        correct_velocity(1, &state.hands[1]);
    }
    return state;
}

__declspec(dllexport) int32_t ovr_GetDevicePoses(void *session, const int32_t *types,
    int32_t count, double time, MotionState *poses)
{
    if (count < 0 || (count && (!types || !poses))) return -1005;
    void *module = GetModuleHandleA("LibOVRRT64_upstream.dll");
    if (!module) module = LoadLibraryA("LibOVRRT64_upstream.dll");
    typedef int32_t (*GetPoses)(void *, const int32_t *, int32_t, double, MotionState *);
    GetPoses get = module ? (GetPoses)GetProcAddress(module, "ovr_GetDevicePoses") : 0;
    if (!get) return -1004;
    int32_t result = get(session, types, count, time, poses);
    if (result >= 0) for (int32_t i = 0; i < count; ++i) {
        if (types[i] == 2) correct_velocity(0, &poses[i]);
        if (types[i] == 4) correct_velocity(1, &poses[i]);
    }
    return result;
}

__declspec(dllexport) PublicHmd ovr_GetHmdDesc(void *session)
{
    void *module = GetModuleHandleA("LibOVRRT64_upstream.dll");
    if (!module) module = LoadLibraryA("LibOVRRT64_upstream.dll");
    typedef UpstreamHmd (*GetHmd)(void *);
    GetHmd get_hmd = module ? (GetHmd)GetProcAddress(module, "ovr_GetHmdDesc") : 0;
    if (!get_hmd) {
        PublicHmd empty = {0};
        return empty;
    }
    OutputDebugStringA("LE2 HMD: converting pinned shim layout to CAPI x64\n");
    return correct_hmd(get_hmd(session));
}

static char *append(char *out, const char *text)
{
    while (*text) *out++ = *text++;
    return out;
}

static char *hex(char *out, uint32_t value)
{
    out = append(out, "0x");
    for (int shift = 28; shift >= 0; shift -= 4)
        *out++ = "0123456789abcdef"[(value >> shift) & 15];
    return out;
}

static uint32_t percent(float value)
{
    return value > 0 ? (value < 1 ? (uint32_t)(value * 100) : 100) : 0;
}

__declspec(dllexport) int32_t ovr_GetInputState(void *session, uint32_t request,
                                              InputState *state)
{
    if (!state) return -1005;
    void *module = GetModuleHandleA("LibOVRRT64_upstream.dll");
    if (!module) module = LoadLibraryA("LibOVRRT64_upstream.dll");
    typedef int32_t (*GetInput)(void *, uint32_t, InputState *);
    GetInput get_input = module ? (GetInput)GetProcAddress(module, "ovr_GetInputState") : 0;
    if (!get_input) return -1004;
    int32_t result = get_input(session, touch_request(request), state);
    if (result < 0) return result;
    correct_input(state, request);
    AcquireSRWLockExclusive(&log_lock);
    if (!(calls++ % 90) || request != last_request || state->buttons != last_buttons ||
        state->touches != last_touches) {
        char line[384], *out = line;
        out = append(out, "LE2 input: requested="); out = hex(out, request);
        out = append(out, " returned="); out = hex(out, state->controller_type);
        out = append(out, " buttons="); out = hex(out, state->buttons);
        out = append(out, " touches="); out = hex(out, state->touches);
        out = append(out, " trigger_pct(L,R)="); out = hex(out, percent(state->trigger[0]));
        out = append(out, ","); out = hex(out, percent(state->trigger[1]));
        out = append(out, " grip_pct(L,R)="); out = hex(out, percent(state->grip[0]));
        out = append(out, ","); out = hex(out, percent(state->grip[1]));
        out = append(out, "\n"); *out = 0;
        OutputDebugStringA(line);
        last_request = request; last_buttons = state->buttons; last_touches = state->touches;
    }
    ReleaseSRWLockExclusive(&log_lock);
    return result;
}
