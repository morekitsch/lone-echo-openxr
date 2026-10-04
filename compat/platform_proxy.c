/* Local extensions to libovr-openxr-rs 0.5.0 for Lone Echo II.
 *
 * The upstream entitlement function returns zero without queuing a response.
 * LE2's pnsovr.dll waits for message type 0x186b58b1 before continuing. Complete
 * that request locally, retaining ownership of our messages through FreeMessage.
 * All upstream messages and the remaining exports keep their upstream behavior.
 *
 * These Windows x64 declarations avoid a Windows SDK/CRT build dependency.
 */
#include <stdbool.h>
#include <stdint.h>

#define API __declspec(dllexport)
#define IMPORT __declspec(dllimport)
IMPORT void *GetModuleHandleA(const char *);
IMPORT void *GetProcAddress(void *, const char *);
IMPORT void *GetProcessHeap(void);
IMPORT void *HeapAlloc(void *, uint32_t, uint64_t);
IMPORT int HeapFree(void *, uint32_t, void *);
IMPORT void AcquireSRWLockExclusive(void **);
IMPORT void ReleaseSRWLockExclusive(void **);
IMPORT void OutputDebugStringA(const char *);

#define ENTITLEMENT_MESSAGE 0x186b58b1u
#define LOCAL_MAGIC UINT64_C(0x32454c4d53474c43)

typedef struct LocalMessage {
    uint64_t magic;
    uint64_t request_id;
    struct LocalMessage *next;
} LocalMessage;

static void *queue_lock;
static LocalMessage *queue_head;
static LocalMessage *queue_tail;
/* Separate our IDs from the upstream shim's counter, which starts at one. */
static uint64_t next_request = UINT64_C(0x8000000000000000);

static void *upstream(const char *name)
{
    void *module = GetModuleHandleA("LibOVRRT64_1.dll");
    void *function = module ? GetProcAddress(module, name) : 0;
    if (!function)
        OutputDebugStringA("LE2 platform: missing upstream function\n");
    return function;
}

static bool is_local(const void *message)
{
    return message && ((const LocalMessage *)message)->magic == LOCAL_MAGIC;
}

API uint64_t ovr_Entitlement_GetIsViewerEntitled(void)
{
    LocalMessage *message = HeapAlloc(GetProcessHeap(), 8, sizeof(*message));
    if (!message)
        return 0;
    message->magic = LOCAL_MAGIC;
    AcquireSRWLockExclusive(&queue_lock);
    uint64_t request = ++next_request;
    message->request_id = request;
    if (queue_tail)
        queue_tail->next = message;
    else
        queue_head = message;
    queue_tail = message;
    ReleaseSRWLockExclusive(&queue_lock);
    OutputDebugStringA("LE2 platform: entitlement completion queued locally\n");
    return request;
}

API void *ovr_PopMessage(void)
{
    AcquireSRWLockExclusive(&queue_lock);
    LocalMessage *message = queue_head;
    if (message) {
        queue_head = message->next;
        if (!queue_head)
            queue_tail = 0;
        message->next = 0;
    }
    ReleaseSRWLockExclusive(&queue_lock);
    if (message) {
        OutputDebugStringA("LE2 platform: entitlement completion delivered\n");
        return message;
    }
    typedef void *(*Function)(void);
    Function function = (Function)upstream("ovr_PopMessage");
    return function ? function() : 0;
}

API uint32_t ovr_Message_GetType(const void *message)
{
    if (is_local(message))
        return ENTITLEMENT_MESSAGE;
    typedef uint32_t (*Function)(const void *);
    Function function = (Function)upstream("ovr_Message_GetType");
    return function ? function(message) : 0;
}

API uint64_t ovr_Message_GetRequestID(const void *message)
{
    if (is_local(message))
        return ((const LocalMessage *)message)->request_id;
    typedef uint64_t (*Function)(const void *);
    Function function = (Function)upstream("ovr_Message_GetRequestID");
    return function ? function(message) : 0;
}

API bool ovr_Message_IsError(const void *message)
{
    if (is_local(message)) {
        OutputDebugStringA("LE2 platform: entitlement completion read as success\n");
        return false;
    }
    typedef bool (*Function)(const void *);
    Function function = (Function)upstream("ovr_Message_IsError");
    return function ? function(message) : true;
}

API void ovr_FreeMessage(void *message)
{
    if (is_local(message)) {
        HeapFree(GetProcessHeap(), 0, message);
        OutputDebugStringA("LE2 platform: entitlement completion released\n");
        return;
    }
    typedef void (*Function)(void *);
    Function function = (Function)upstream("ovr_FreeMessage");
    if (function)
        function(message);
}

/* Upstream supplies no remote voice samples; match that behavior for the
 * float-sample variant and leave the caller's output buffer untouched. */
__declspec(dllexport) unsigned long long ovr_Voip_GetPCMFloat(
    unsigned long long sender, float *buffer, unsigned long long count)
{
    (void)sender;
    (void)buffer;
    (void)count;
    return 0;
}
