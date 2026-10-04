#include <stdint.h>
#include <stddef.h>

typedef struct { float up, down, left, right; } Fov;
typedef struct { int32_t w, h; } Size;

/* Actual pinned 0.5.0 layout, confirmed in its source and DLL disassembly. */
typedef struct {
    int32_t type;
    char product[64], manufacturer[64];
    int16_t vendor, product_id;
    char serial[24];
    int16_t firmware_major, firmware_minor;
    Size resolution;
    Fov default_fov[2], max_fov[2];
    int32_t default_eye, refresh_rate;
} UpstreamHmd;

/* Public Oculus CAPI x64 layout. Explicit padding is part of the ABI. */
typedef struct {
    int32_t type, pad0;
    char product[64], manufacturer[64];
    int16_t vendor, product_id;
    char serial[24];
    int16_t firmware_major, firmware_minor;
    uint32_t available_hmd_caps, default_hmd_caps;
    uint32_t available_tracking_caps, default_tracking_caps;
    Fov default_fov[2], max_fov[2];
    Size resolution;
    float refresh_rate;
    int32_t pad1;
} PublicHmd;

_Static_assert(sizeof(UpstreamHmd) == 244, "pinned shim HMD size");
_Static_assert(offsetof(UpstreamHmd, default_fov) == 172, "pinned shim FOV offset");
_Static_assert(sizeof(PublicHmd) == 264, "CAPI x64 HMD size");
_Static_assert(offsetof(PublicHmd, default_fov) == 184, "CAPI FOV offset");
_Static_assert(offsetof(PublicHmd, resolution) == 248, "CAPI resolution offset");
_Static_assert(offsetof(PublicHmd, refresh_rate) == 256, "CAPI refresh offset");

static PublicHmd correct_hmd(UpstreamHmd source)
{
    PublicHmd result = {0};
    result.type = source.type;
    for (unsigned i = 0; i < 64; ++i) {
        result.product[i] = source.product[i];
        result.manufacturer[i] = source.manufacturer[i];
    }
    for (unsigned i = 0; i < 24; ++i) result.serial[i] = source.serial[i];
    result.vendor = source.vendor; result.product_id = source.product_id;
    result.firmware_major = source.firmware_major;
    result.firmware_minor = source.firmware_minor;
    result.available_tracking_caps = result.default_tracking_caps = 0x50;
    result.default_fov[0] = source.default_fov[0];
    result.default_fov[1] = source.default_fov[1];
    result.max_fov[0] = source.max_fov[0]; result.max_fov[1] = source.max_fov[1];
    result.resolution = source.resolution;
    result.refresh_rate = (float)source.refresh_rate;
    return result;
}
