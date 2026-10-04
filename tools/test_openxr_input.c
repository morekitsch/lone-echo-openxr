/* Exercise loader dispatch and immutable binding rewrite with a fake runtime. */
#include <assert.h>
#include <stdlib.h>
#include <string.h>
#define __declspec(x)
#include "../compat/openxr_input_proxy.c"

static unsigned calls, retained;
static int grip_mode;
static XrActionSuggestedBinding observed[3];
static XrResult runtime_create(XrSession session, const XrActionSpaceCreateInfo *info, XrSpace *space)
{ (void)session; *space = (XrSpace)(uintptr_t)info->subactionPath; return XR_SUCCESS; }
static XrResult runtime_destroy(XrSpace space) { (void)space; return XR_SUCCESS; }
static int tracking_valid = 1, bad_velocity;
static XrResult runtime_locate(XrSpace space, XrSpace base, XrTime time, XrSpaceLocation *location)
{
    (void)base; (void)time;
    location->locationFlags = tracking_valid ? 15 : 0;
    location->pose = (XrPosef){ .orientation.w = 1, .position.x = (float)(uintptr_t)space };
    XrSpaceVelocity *v = location->next;
    assert(v && v->type == XR_TYPE_SPACE_VELOCITY); /* no duplicate chain entry */
    v->velocityFlags = tracking_valid ? 3 : 0;
    v->linearVelocity = (XrVector3f){0, 0, -5};
    v->angularVelocity = (XrVector3f){(float)(uintptr_t)space, 2, 3};
    if (bad_velocity) v->linearVelocity.z = __builtin_nanf("");
    return XR_SUCCESS;
}
static XrResult runtime_suggest(XrInstance instance, const XrInteractionProfileSuggestedBinding *info)
{
    assert(instance == (XrInstance)(uintptr_t)1);
    assert(info->type == XR_TYPE_INTERACTION_PROFILE_SUGGESTED_BINDING);
    assert(info->interactionProfile == 77 && info->countSuggestedBindings == 3);
    assert(info->next == (void *)(uintptr_t)123);
    for (unsigned i = 0; i < 3; ++i) observed[i] = info->suggestedBindings[i];
    ++calls;
    return XR_SUCCESS;
}
static XrResult runtime_path(XrInstance instance, const char *name, XrPath *path)
{
    (void)instance;
    if (equal(name, "/user/hand/left/input/grip/pose")) *path = 1;
    else if (equal(name, "/user/hand/right/input/grip/pose")) *path = 2;
    else if (equal(name, "/user/hand/left/input/aim/pose")) *path = 3;
    else if (equal(name, "/user/hand/right/input/aim/pose")) *path = 4;
    else if (equal(name, "/user/hand/left")) *path = 101;
    else if (equal(name, "/user/hand/right")) *path = 102;
    else return XR_ERROR_PATH_FORMAT_INVALID;
    return XR_SUCCESS;
}
static void unrelated(void) { ++retained; }
static XrResult runtime_proc(XrInstance instance, const char *name, PFN_xrVoidFunction *out)
{
    (void)instance;
    if (!out) return XR_ERROR_VALIDATION_FAILURE;
    if (equal(name, "xrSuggestInteractionProfileBindings"))
        *out = (PFN_xrVoidFunction)runtime_suggest;
    else if (equal(name, "unrelated")) *out = unrelated;
    else if (equal(name, "xrLocateSpace")) *out = (PFN_xrVoidFunction)runtime_locate;
    else if (equal(name, "xrCreateActionSpace")) *out = (PFN_xrVoidFunction)runtime_create;
    else if (equal(name, "xrDestroySpace")) *out = (PFN_xrVoidFunction)runtime_destroy;
    else { *out = 0; return XR_ERROR_FUNCTION_UNSUPPORTED; }
    return XR_SUCCESS;
}
void *GetModuleHandleA(const char *name) { (void)name; return (void *)(uintptr_t)1; }
void *LoadLibraryA(const char *name) { return GetModuleHandleA(name); }
void *GetProcAddress(void *module, const char *name)
{
    (void)module;
    if (equal(name, "xrSuggestInteractionProfileBindings")) return (void *)runtime_suggest;
    if (equal(name, "xrStringToPath")) return (void *)runtime_path;
    if (equal(name, "xrGetInstanceProcAddr")) return (void *)runtime_proc;
    if (equal(name, "xrLocateSpace")) return (void *)runtime_locate;
    if (equal(name, "xrCreateActionSpace")) return (void *)runtime_create;
    if (equal(name, "xrDestroySpace")) return (void *)runtime_destroy;
    return 0;
}
void *GetProcessHeap(void) { return 0; }
void *HeapAlloc(void *heap, uint32_t flags, uint64_t size)
{ (void)heap; (void)flags; return malloc(size); }
int HeapFree(void *heap, uint32_t flags, void *memory)
{ (void)heap; (void)flags; free(memory); return 1; }
void OutputDebugStringA(const char *message) { (void)message; }
void AcquireSRWLockExclusive(void **lock) { (void)lock; }
void ReleaseSRWLockExclusive(void **lock) { (void)lock; }
uint32_t GetEnvironmentVariableA(const char *name, char *out, uint32_t size)
{
    assert(equal(name, "LE2_CONTROLLER_POSE") && size >= 5);
    if (grip_mode) { memcpy(out, "grip", 5); return 4; }
    return 0;
}
int main(void)
{
    XrInstance instance = (XrInstance)(uintptr_t)1;
    XrActionSuggestedBinding bindings[] = {
        {(XrAction)(uintptr_t)11, 1}, {(XrAction)(uintptr_t)12, 2}, {(XrAction)(uintptr_t)13, 9},
    };
    XrInteractionProfileSuggestedBinding info = {
        XR_TYPE_INTERACTION_PROFILE_SUGGESTED_BINDING, (void *)(uintptr_t)123, 77, 3, bindings,
    };
    PFN_xrVoidFunction function;
    assert(xrGetInstanceProcAddr(instance, "xrSuggestInteractionProfileBindings", &function) == XR_SUCCESS);
    assert(function == (PFN_xrVoidFunction)xrSuggestInteractionProfileBindings);
    assert(((PFN_xrSuggestInteractionProfileBindings)function)(instance, &info) == XR_SUCCESS);
    assert(calls == 1 && observed[0].binding == 3 && observed[1].binding == 4 && observed[2].binding == 9);
    assert(observed[0].action == bindings[0].action && observed[1].action == bindings[1].action);
    assert(bindings[0].binding == 1 && bindings[1].binding == 2); /* caller remains unchanged */
    grip_mode = 1;
    assert(xrSuggestInteractionProfileBindings(instance, &info) == XR_SUCCESS);
    assert(calls == 2 && observed[0].binding == 1 && observed[1].binding == 2);
    assert(xrGetInstanceProcAddr(instance, "unrelated", &function) == XR_SUCCESS);
    function(); assert(retained == 1);
    assert(xrGetInstanceProcAddr(instance, "missing", &function) == XR_ERROR_FUNCTION_UNSUPPORTED);
    assert(function == 0);
    /* Identical forward translation, distinguishable per-hand angular data. */
    for (unsigned h = 0; h < 2; ++h) {
        XrActionSpaceCreateInfo create = { .type = XR_TYPE_ACTION_SPACE_CREATE_INFO,
            .subactionPath = 101 + h, .poseInActionSpace.orientation.w = 1 };
        XrSpace space;
        assert(xrCreateActionSpace(XR_NULL_HANDLE, &create, &space) == XR_SUCCESS);
        XrSpaceLocation location = { .type = XR_TYPE_SPACE_LOCATION };
        assert(xrGetInstanceProcAddr(instance, "xrLocateSpace", &function) == XR_SUCCESS);
        assert(function == (PFN_xrVoidFunction)xrLocateSpace);
        assert(((PFN_xrLocateSpace)function)(space, XR_NULL_HANDLE, 100, &location) == XR_SUCCESS);
        assert(!location.next); /* temporary velocity chain is removed */
        MotionPose pose = { .orientation.w = 1, .position.x = (float)(101 + h) };
        MotionVector linear, angular;
        assert(le2_GetHandVelocity(h, &pose, &linear, &angular));
        assert(linear.z == -5 && angular.x == (float)(101 + h));
        assert(!le2_GetHandVelocity(1 - h, &pose, &linear, &angular));
        /* Tracking loss must zero velocity, without poisoning the last pose. */
        tracking_valid = 0;
        assert(xrLocateSpace(space, XR_NULL_HANDLE, 200, &location) == XR_SUCCESS);
        assert(le2_GetHandVelocity(h, &pose, &linear, &angular));
        assert(linear.z == 0 && angular.x == 0);
        tracking_valid = 1;
        XrSpaceVelocity existing = { .type = XR_TYPE_SPACE_VELOCITY };
        location.next = &existing;
        assert(xrLocateSpace(space, XR_NULL_HANDLE, 300, &location) == XR_SUCCESS);
        assert(location.next == &existing && existing.linearVelocity.z == -5);
        assert(le2_GetHandVelocity(h, &pose, &linear, &angular) && linear.z == -5);
        bad_velocity = 1;
        assert(xrLocateSpace(space, XR_NULL_HANDLE, 400, &location) == XR_SUCCESS);
        assert(le2_GetHandVelocity(h, &pose, &linear, &angular) && linear.z == 0);
        bad_velocity = 0;
        assert(xrDestroySpace(space) == XR_SUCCESS);
        assert(!le2_GetHandVelocity(h, &pose, &linear, &angular));
    }
    return 0;
}
