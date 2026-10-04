/* Enumerate loader extensions without creating an instance, session or window.
 * Linux: cc tools/probe_openxr_extensions.c -ldl -o probe_openxr_extensions
 * Windows: compile against OpenXR headers; no loader import library is needed.
 * Optional argument: explicit path to the OpenXR loader library.
 */
#define XR_NO_PROTOTYPES
#include <openxr/openxr.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#ifdef _WIN32
#include <windows.h>
#else
#include <dlfcn.h>
#endif

int main(int argc, char **argv)
{
    if (argc > 2) return 2;
#ifdef _WIN32
    const char *path = argc == 2 ? argv[1] : "openxr_loader.dll";
    HMODULE loader = LoadLibraryA(path);
    if (!loader) { fprintf(stderr, "LoadLibrary failed: %lu\n", GetLastError()); return 1; }
    PFN_xrEnumerateInstanceExtensionProperties enumerate =
        (PFN_xrEnumerateInstanceExtensionProperties)GetProcAddress(loader, "xrEnumerateInstanceExtensionProperties");
#else
    const char *path = argc == 2 ? argv[1] : "libopenxr_loader.so.1";
    void *loader = dlopen(path, RTLD_NOW | RTLD_LOCAL);
    if (!loader) { fprintf(stderr, "dlopen failed: %s\n", dlerror()); return 1; }
    PFN_xrEnumerateInstanceExtensionProperties enumerate =
        (PFN_xrEnumerateInstanceExtensionProperties)dlsym(loader, "xrEnumerateInstanceExtensionProperties");
#endif
    if (!enumerate) { fprintf(stderr, "Missing enumeration function\n"); return 1; }
    for (unsigned attempt = 0; attempt < 3; ++attempt) {
        uint32_t count = 0;
        XrResult result = enumerate(NULL, 0, &count, NULL);
        printf("count query result=%d count=%u\n", result, count);
        if (result != XR_SUCCESS || count > 4096) return 1;
        if (!count) { puts("XR_KHR_visibility_mask: unavailable"); return 0; }
        XrExtensionProperties *properties = calloc(count, sizeof(*properties));
        if (!properties) return 1;
        for (uint32_t i = 0; i < count; ++i) properties[i].type = XR_TYPE_EXTENSION_PROPERTIES;
        uint32_t capacity = count;
        result = enumerate(NULL, capacity, &count, properties);
        if (result == XR_ERROR_SIZE_INSUFFICIENT) { free(properties); continue; }
        printf("fill query result=%d count=%u\n", result, count);
        if (result != XR_SUCCESS || count > capacity) { free(properties); return 1; }
        int available = 0;
        for (uint32_t i = 0; i < count; ++i) {
            properties[i].extensionName[XR_MAX_EXTENSION_NAME_SIZE - 1] = '\0';
            printf("%s version=%u\n", properties[i].extensionName, properties[i].extensionVersion);
            if (!strcmp(properties[i].extensionName, "XR_KHR_visibility_mask")) available = 1;
        }
        printf("XR_KHR_visibility_mask: %s\n", available ? "available" : "unavailable");
        free(properties);
        return 0;
    }
    fputs("Extension counts kept changing\n", stderr);
    return 1;
}
