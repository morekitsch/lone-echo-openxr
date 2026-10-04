#include <assert.h>
#include "../compat/vr_input.h"
#include "../compat/vr_hmd.h"

int main(void)
{
    InputState state = {0};
    state.buttons = 0x901; /* menu + X + A */
    state.touches = 0x2020; /* both index fingers pointing */
    state.trigger[0] = .25f; state.trigger[1] = .75f;
    correct_input(&state, UINT32_MAX);
    assert(state.controller_type == 3 && state.buttons == 0x100101);
    assert(state.touches == 0x2020 && state.trigger[0] == .25f && state.trigger[1] == .75f);
    correct_input(&state, 2);
    assert(state.controller_type == 2 && state.buttons == 1 && state.touches == 0x20);
    assert(state.trigger[0] == 0 && state.trigger[1] == .75f);
    correct_input(&state, 0x10); /* Xbox request must not receive Touch input */
    assert(state.controller_type == 0 && state.buttons == 0 && state.touches == 0);
    assert(state.trigger[1] == 0);
    assert(touch_request(0xff) == 3 && touch_request(1) == 1);
    UpstreamHmd hmd = {0};
    hmd.default_fov[0] = (Fov){.9f, 1.4f, 1.3f, .8f};
    hmd.default_fov[1] = (Fov){.9f, 1.4f, .8f, 1.3f};
    hmd.resolution = (Size){4128, 2162}; hmd.refresh_rate = 90;
    PublicHmd converted = correct_hmd(hmd);
    assert(converted.default_fov[0].left == 1.3f && converted.default_fov[1].right == 1.3f);
    assert(converted.resolution.w == 4128 && converted.resolution.h == 2162);
    assert(converted.refresh_rate == 90.0f);
    return 0;
}
