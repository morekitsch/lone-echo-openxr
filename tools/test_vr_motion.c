#include <assert.h>
#include <string.h>
#define __declspec(x)
#include "../compat/vr_proxy.c"

static int legacy;
static MotionTracking runtime_tracking(void *session, double time, unsigned char marker)
{
    assert(session == (void *)(uintptr_t)1 && marker == 1);
    MotionTracking tracking = {0};
    tracking.flags = 0xe3;
    tracking.head.linear_velocity.z = 7;
    for (unsigned h = 0; h < 2; ++h) {
        tracking.hands[h].pose.position.x = (float)h;
        tracking.hands[h].pose.orientation.w = 1;
        tracking.hands[h].time = time;
        tracking.hands[h].linear_velocity.z = 100 + (float)h;
    }
    return tracking;
}
static int native_velocity(unsigned hand, const MotionPose *pose, MotionVector *linear, MotionVector *angular)
{
    assert(hand < 2 && pose->position.x == (float)hand && pose->orientation.w == 1);
    *linear = (MotionVector){0, 0, -5};
    *angular = (MotionVector){1 + (float)hand, 2, 3};
    return 1;
}
static int32_t runtime_devices(void *session, const int32_t *types, int32_t count, double time, MotionState *poses)
{
    MotionTracking state = runtime_tracking(session, time, 1);
    for (int i = 0; i < count; ++i)
        poses[i] = types[i] == 1 ? state.head : state.hands[types[i] == 2 ? 0 : 1];
    return 0;
}
void *GetModuleHandleA(const char *name) { (void)name; return (void *)(uintptr_t)1; }
void *LoadLibraryA(const char *name) { return GetModuleHandleA(name); }
void *GetProcAddress(void *module, const char *name)
{
    (void)module;
    if (!strcmp(name, "ovr_GetTrackingState")) return (void *)runtime_tracking;
    if (!strcmp(name, "ovr_GetDevicePoses")) return (void *)runtime_devices;
    if (!strcmp(name, "le2_GetHandVelocity")) return (void *)native_velocity;
    return 0;
}
void OutputDebugStringA(const char *line) { (void)line; }
void AcquireSRWLockExclusive(void **lock) { (void)lock; }
void ReleaseSRWLockExclusive(void **lock) { (void)lock; }
uint32_t GetEnvironmentVariableA(const char *name, char *out, uint32_t size)
{
    assert(!strcmp(name, "LE2_VELOCITY") && size >= 7);
    if (legacy) { memcpy(out, "legacy", 7); return 6; }
    return 0;
}
int main(void)
{
    void *session = (void *)(uintptr_t)1;
    MotionTracking state = ovr_GetTrackingState(session, 12.5, 1);
    assert(state.flags == 0xe3 && state.head.linear_velocity.z == 7);
    for (unsigned h = 0; h < 2; ++h) {
        assert(state.hands[h].linear_velocity.z == -5);
        assert(state.hands[h].angular_velocity.x == 1 + (float)h);
        assert(state.hands[h].time == 12.5);
    }
    int32_t types[] = {4, 1, 2};
    MotionState poses[3];
    assert(ovr_GetDevicePoses(session, types, 3, 20, poses) == 0);
    assert(poses[0].angular_velocity.x == 2 && poses[0].linear_velocity.z == -5);
    assert(poses[1].linear_velocity.z == 7);
    assert(poses[2].angular_velocity.x == 1 && poses[2].linear_velocity.z == -5);
    legacy = 1;
    state = ovr_GetTrackingState(session, 12.5, 1);
    assert(state.hands[0].linear_velocity.z == 100 && state.hands[1].linear_velocity.z == 101);
    return 0;
}
