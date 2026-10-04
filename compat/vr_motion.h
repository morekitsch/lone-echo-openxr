#ifndef LE2_VR_MOTION_H
#define LE2_VR_MOTION_H
#include <stdint.h>
#include <stddef.h>
typedef struct { float x, y, z; } MotionVector;
typedef struct { float x, y, z, w; } MotionQuat;
typedef struct { MotionQuat orientation; MotionVector position; } MotionPose;
typedef struct {
    MotionPose pose;
    MotionVector angular_velocity, linear_velocity;
    MotionVector angular_acceleration, linear_acceleration;
    double time;
} MotionState;
typedef struct {
    MotionState head;
    uint32_t flags;
    MotionState hands[2];
    uint32_t hand_flags[2];
    MotionPose origin;
} MotionTracking;
_Static_assert(sizeof(MotionState) == 88, "CAPI pose state size");
_Static_assert(offsetof(MotionState, linear_velocity) == 40, "linear velocity ABI");
_Static_assert(sizeof(MotionTracking) == 312, "CAPI tracking state size");
_Static_assert(offsetof(MotionTracking, hands) == 96, "CAPI hands offset");

typedef int (*GetHandVelocity)(unsigned, const MotionPose *, MotionVector *, MotionVector *);
#endif
