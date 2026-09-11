/* Native thread ABI boundary. No Dodo pointers escape the lifetime established
 * by Join, except detached owned contexts whose callback releases the storage.
 * Thread cancellation is never enabled or exposed by this runtime. */
#define _GNU_SOURCE
#define _POSIX_C_SOURCE 200809L
#include <stdint.h>
#include <stddef.h>
#include <stdlib.h>
#include <errno.h>

#ifdef _WIN32
#include <windows.h>
#include <process.h>
typedef HANDLE DodoThreadHandle;
#else
#include <pthread.h>
#include <sched.h>
#include <time.h>
#include <sys/mman.h>
#include <unistd.h>
typedef pthread_t DodoThreadHandle;
#endif

typedef void (*DodoCallback)(void *);
typedef struct {
    DodoThreadHandle handle;
    DodoCallback callback;
    void *argument;
} DodoThreadState;
_Static_assert(sizeof(DodoThreadState) <= 16 * sizeof(uintptr_t), "Dodo native thread storage too small");
_Static_assert(_Alignof(DodoThreadState) <= _Alignof(uintptr_t), "Dodo native thread storage alignment insufficient");
_Static_assert(sizeof(DodoCallback) == sizeof(void *), "Dodo callback representation unsupported");

#ifdef _WIN32
static unsigned __stdcall dodo_thread_entry(void *argument) {
#else
static void *dodo_thread_entry(void *argument) {
    /* The library has no cancellation cleanup/unwinding protocol. Foreign
     * pthread_cancel must not asynchronously destroy Dodo-owned values. */
    if (pthread_setcancelstate(PTHREAD_CANCEL_DISABLE, NULL) != 0) abort();
#endif
    DodoThreadState *state = argument;
    DodoCallback callback = state->callback;
    void *context = state->argument;
    callback(context);
#ifdef _WIN32
    return 0;
#else
    return NULL;
#endif
}

int32_t dodo_thread_start(void *storage, DodoCallback callback, void *argument) {
    DodoThreadState *state = storage;
    state->callback = callback;
    state->argument = argument;
#ifdef _WIN32
    uintptr_t handle = _beginthreadex(NULL, 0, dodo_thread_entry, state, 0, NULL);
    if (!handle) return errno ? errno : EAGAIN;
    state->handle = (HANDLE)handle;
    return 0;
#else
    return pthread_create(&state->handle, NULL, dodo_thread_entry, state);
#endif
}

void dodo_thread_join(void *storage) {
    DodoThreadState *state = storage;
#ifdef _WIN32
    if (WaitForSingleObject(state->handle, INFINITE) != WAIT_OBJECT_0) abort();
    if (!CloseHandle(state->handle)) abort();
#else
    if (pthread_join(state->handle, NULL) != 0) abort();
#endif
}

void dodo_thread_detach(void *storage) {
    DodoThreadState *state = storage;
#ifdef _WIN32
    if (!CloseHandle(state->handle)) abort();
#else
    if (pthread_detach(state->handle) != 0) abort();
#endif
}

void dodo_thread_yield(void) {
#ifdef _WIN32
    (void)SwitchToThread();
#else
    (void)sched_yield();
#endif
}
void dodo_thread_sleep(uint32_t milliseconds) {
#ifdef _WIN32
    /* UINT32_MAX is a finite API duration, while Win32 reserves it for
     * INFINITE. Split it rather than silently requesting an infinite wait. */
    if (milliseconds == INFINITE) {
        Sleep(INFINITE - 1);
        Sleep(1);
    } else {
        Sleep(milliseconds);
    }
#else
    struct timespec duration = { milliseconds / 1000, (milliseconds % 1000) * 1000000L };
    while (nanosleep(&duration, &duration) != 0) {
        if (errno != EINTR) abort();
    }
#endif
}

/* Allocation is requested explicitly by the owned-thread API. Mapping keeps
 * resource release independent of any process-global Dodo allocator. */
void *dodo_thread_allocate(uintptr_t size, uintptr_t align, int32_t *error) {
    *error = 0;
#ifdef _WIN32
    SYSTEM_INFO information;
    GetSystemInfo(&information);
    if (align > information.dwPageSize || !size) { *error = EINVAL; return NULL; }
    void *result = VirtualAlloc(NULL, size, MEM_RESERVE | MEM_COMMIT, PAGE_READWRITE);
    if (!result) *error = (int32_t)GetLastError();
    return result;
#else
    long page = sysconf(_SC_PAGESIZE);
    if (page <= 0 || align > (uintptr_t)page || !size) { *error = EINVAL; return NULL; }
    void *result = mmap(NULL, size, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (result == MAP_FAILED) { *error = errno; return NULL; }
    return result;
#endif
}
void dodo_thread_free(void *pointer, uintptr_t size) {
#ifdef _WIN32
    (void)size;
    if (!VirtualFree(pointer, 0, MEM_RELEASE)) abort();
#else
    if (munmap(pointer, size) != 0) abort();
#endif
}
