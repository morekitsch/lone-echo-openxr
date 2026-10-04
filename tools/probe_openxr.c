/* Native runtime check; creates no graphics session or swapchains.
 * Build: cc tools/probe_openxr.c -lopenxr_loader -o working/probe_openxr
 * Run with XR_RUNTIME_JSON pointing at the WiVRn manifest.
 */
#include <openxr/openxr.h>
#include <stdio.h>

int main(void)
{
    uint32_t count = 0;
    XrResult result = xrEnumerateInstanceExtensionProperties(0, 0, &count, 0);
    printf("xrEnumerateInstanceExtensionProperties: %d (%u extensions)\n", result, count);
    if (XR_FAILED(result))
        return 1;
    const char *extensions[] = {"XR_KHR_vulkan_enable"};
    XrInstanceCreateInfo info = {
        .type = XR_TYPE_INSTANCE_CREATE_INFO,
        .applicationInfo = {
            .applicationName = "Lone Echo II runtime probe",
            .apiVersion = XR_MAKE_VERSION(1, 0, 0),
        },
        .enabledExtensionCount = 1,
        .enabledExtensionNames = extensions,
    };
    XrInstance instance = XR_NULL_HANDLE;
    result = xrCreateInstance(&info, &instance);
    printf("xrCreateInstance: %d\n", result);
    if (XR_FAILED(result))
        return 2;
    XrInstanceProperties properties = {.type = XR_TYPE_INSTANCE_PROPERTIES};
    result = xrGetInstanceProperties(instance, &properties);
    if (XR_SUCCEEDED(result))
        printf("Runtime: %s\n", properties.runtimeName);
    XrSystemGetInfo system_info = {
        .type = XR_TYPE_SYSTEM_GET_INFO,
        .formFactor = XR_FORM_FACTOR_HEAD_MOUNTED_DISPLAY,
    };
    XrSystemId system = XR_NULL_SYSTEM_ID;
    result = xrGetSystem(instance, &system_info, &system);
    printf("xrGetSystem: %d\n", result);
    if (XR_SUCCEEDED(result)) {
        XrSystemProperties system_properties = {.type = XR_TYPE_SYSTEM_PROPERTIES};
        result = xrGetSystemProperties(instance, system, &system_properties);
        if (XR_SUCCEEDED(result))
            printf("Headset: %s\n", system_properties.systemName);
    }
    xrDestroyInstance(instance);
    return XR_FAILED(result) ? 3 : 0;
}
