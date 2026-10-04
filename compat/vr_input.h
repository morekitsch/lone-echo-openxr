/* Layout shared with libovr-openxr 0.5.0 (CAPI 1.94). */
#include <stdint.h>
#include <stddef.h>

typedef struct { float x, y; } InputVector2;
typedef struct {
    double time;
    uint32_t buttons, touches;
    float trigger[2], grip[2];
    InputVector2 stick[2];
    uint32_t controller_type;
    float trigger_no_deadzone[2], grip_no_deadzone[2];
    InputVector2 stick_no_deadzone[2];
    float trigger_raw[2], grip_raw[2];
    InputVector2 stick_raw[2];
} InputState;

_Static_assert(sizeof(InputState) == 120, "input ABI size");
_Static_assert(offsetof(InputState, controller_type) == 48, "controller ABI offset");
_Static_assert(offsetof(InputState, trigger_raw) == 84, "raw trigger ABI offset");

static uint32_t touch_request(uint32_t request)
{
    /* Older CAPI used 0xff; current CAPI uses 0xffffffff for Active. */
    return request == 0xff || request == UINT32_MAX ? 3 : request & 3;
}

static void correct_input(InputState *state, uint32_t request)
{
    uint32_t selected = touch_request(request);
    state->controller_type = selected;
    /* Upstream puts Touch menu in LShoulder; CAPI specifies Enter. */
    if (state->buttons & 0x800) {
        state->buttons &= ~0x800u;
        state->buttons |= 0x100000;
    }
    for (unsigned hand = 0; hand < 2; ++hand) {
        if (selected & (1u << hand))
            continue;
        state->buttons &= ~(hand == 0 ? 0x100f00u : 0x0fu);
        state->touches &= ~(hand == 0 ? 0x7f00u : 0x7fu);
        state->trigger[hand] = state->grip[hand] = 0;
        state->trigger_no_deadzone[hand] = state->grip_no_deadzone[hand] = 0;
        state->trigger_raw[hand] = state->grip_raw[hand] = 0;
        state->stick[hand] = (InputVector2){0, 0};
        state->stick_no_deadzone[hand] = state->stick_raw[hand] = state->stick[hand];
    }
}
