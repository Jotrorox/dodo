/* Explicit hosted blocking boundary. No scheduler, heap or process-global state.
 * Public Dodo owners keep these caller-backed objects at stable addresses.
 * Layouts come from target headers; no pthread or Win32 layout is guessed. */
#define _GNU_SOURCE
#include <stdint.h>
#include <stddef.h>
#include <string.h>
#include <limits.h>
#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
typedef SRWLOCK Gate;
typedef CONDITION_VARIABLE Condition;
#else
#include <pthread.h>
#include <time.h>
#include <errno.h>
#include <sys/mman.h>
typedef pthread_mutex_t Gate;
typedef pthread_cond_t Condition;
#endif

/* Return codes are independent of errno/GetLastError: 1 timeout, 2 cancelled,
 * 3 closed, 4 invalid input, 5 native failure, 6 allocation failure. */
typedef struct {
    Gate gate;
    Condition changed;
    Condition notified;
    size_t readers, writers_waiting;
    int writer, closed, cancelled, broken;
    size_t parties, arrived, generation;
    unsigned char *ring;
    size_t stride, capacity, head, count;
    size_t references, mapping_size;
} State;
_Static_assert(sizeof(State) <= 256, "Dodo sync storage is too small on this ABI");
_Static_assert(_Alignof(State) <= 8, "Dodo sync storage alignment unsupported");

static uint64_t ticks(void) {
#ifdef _WIN32
    return GetTickCount64();
#else
    struct timespec t;
    if (clock_gettime(CLOCK_MONOTONIC, &t)) return 0;
    return (uint64_t)t.tv_sec * 1000 + (uint64_t)t.tv_nsec / 1000000;
#endif
}
static uint64_t deadline(uint64_t timeout) {
    uint64_t now = ticks();
    return timeout > UINT64_MAX - now ? UINT64_MAX : now + timeout;
}
static void enter(State *s) {
#ifdef _WIN32
    AcquireSRWLockExclusive(&s->gate);
#else
    if (pthread_mutex_lock(&s->gate)) __builtin_trap();
#endif
}
static void leave(State *s) {
#ifdef _WIN32
    ReleaseSRWLockExclusive(&s->gate);
#else
    if (pthread_mutex_unlock(&s->gate)) __builtin_trap();
#endif
}
static void wake(State *s, int all) {
#ifdef _WIN32
    if (all) WakeAllConditionVariable(&s->changed);
    else WakeConditionVariable(&s->changed);
#else
    if (all) pthread_cond_broadcast(&s->changed);
    else pthread_cond_signal(&s->changed);
#endif
}
static int wait_condition(State *s, Condition *condition, uint64_t end) {
#ifdef _WIN32
    uint64_t now = ticks();
    DWORD ms = INFINITE;
    if (end != UINT64_MAX) {
        if (now >= end) return 1;
        uint64_t remaining = end - now;
        ms = remaining >= INFINITE ? INFINITE - 1 : (DWORD)remaining;
    }
    if (SleepConditionVariableSRW(condition, &s->gate, ms, 0)) return 0;
    return GetLastError() == ERROR_TIMEOUT ? 1 : 5;
#else
    int code;
    if (end == UINT64_MAX) code = pthread_cond_wait(condition, &s->gate);
    else {
        struct timespec t = {(time_t)(end / 1000), (long)(end % 1000) * 1000000};
        code = pthread_cond_timedwait(condition, &s->gate, &t);
    }
    return code == 0 ? 0 : code == ETIMEDOUT ? 1 : 5;
#endif
}
static int wait_until(State *s, uint64_t end) {
    return wait_condition(s, &s->changed, end);
}
static void notify(State *s, int all) {
#ifdef _WIN32
    if (all) WakeAllConditionVariable(&s->notified);
    else WakeConditionVariable(&s->notified);
#else
    if (all) pthread_cond_broadcast(&s->notified);
    else pthread_cond_signal(&s->notified);
#endif
}
int32_t dodo_sync_init(void *memory, size_t bytes) {
    if (!memory || bytes < sizeof(State) || (uintptr_t)memory % _Alignof(State)) return 4;
    State *s = memory;
    memset(s, 0, sizeof(*s));
#ifdef _WIN32
    InitializeSRWLock(&s->gate);
    InitializeConditionVariable(&s->changed);
    InitializeConditionVariable(&s->notified);
#else
    int code = pthread_mutex_init(&s->gate, NULL);
    if (code) return 5;
    pthread_condattr_t attr;
    if (pthread_condattr_init(&attr)) { pthread_mutex_destroy(&s->gate); return 5; }
    code = pthread_condattr_setclock(&attr, CLOCK_MONOTONIC);
    if (!code) code = pthread_cond_init(&s->changed, &attr);
    if (!code) {
        code = pthread_cond_init(&s->notified, &attr);
        if (code) pthread_cond_destroy(&s->changed);
    }
    pthread_condattr_destroy(&attr);
    if (code) { pthread_mutex_destroy(&s->gate); return 5; }
#endif
    s->references = 1;
    return 0;
}
void dodo_sync_destroy(void *memory) {
    State *s = memory;
#ifndef _WIN32
    /* A live guard/operation holds its owner. Failure means an unsafe caller
     * violated ownership; continuing would permit use after destruction. */
    if (pthread_cond_destroy(&s->changed) || pthread_cond_destroy(&s->notified) || pthread_mutex_destroy(&s->gate)) __builtin_trap();
#else
    (void)s;
#endif
}
/* Mutex/RW ownership is represented by counters under the native gate. This
 * gives both targets the same monotonic timed-wait and cancellation contract. */
int32_t dodo_sync_lock(void *memory, int32_t shared, uint64_t timeout) {
    State *s = memory;
    uint64_t end = deadline(timeout);
    enter(s);
    if (!shared) ++s->writers_waiting;
    int result = 0;
    while (s->writer || (!shared && s->readers) || (shared && s->writers_waiting)) {
        if (s->cancelled) { result = 2; break; }
        result = wait_until(s, end);
        if (result) break;
    }
    if (!shared) --s->writers_waiting;
    if (!result && s->cancelled) result = 2;
    if (!result) {
        if (shared) ++s->readers;
        else s->writer = 1;
    }
    if (result && !shared) wake(s, 1);
    leave(s);
    return result;
}
void dodo_sync_unlock(void *memory, int32_t shared) {
    State *s = memory;
    enter(s);
    if (shared) --s->readers;
    else s->writer = 0;
    wake(s, 1);
    leave(s);
}
/* Always reacquires logical exclusive ownership, including timeout/error.
 * Cancellation wakes the wait, never invalidates an existing guard. */
int32_t dodo_sync_wait(void *memory, uint64_t timeout) {
    State *s = memory;
    uint64_t end = deadline(timeout);
    enter(s);
    s->writer = 0;
    wake(s, 1);
    int result = s->cancelled ? 2 : wait_condition(s, &s->notified, end);
    ++s->writers_waiting;
    while (s->writer || s->readers) {
        if (wait_until(s, UINT64_MAX) == 5) __builtin_trap();
    }
    --s->writers_waiting;
    s->writer = 1;
    if (s->cancelled) result = 2;
    leave(s);
    return result;
}
void dodo_sync_notify(void *memory, int32_t all) {
    State *s = memory;
    enter(s); notify(s, all); leave(s);
}
void dodo_sync_cancel(void *memory) {
    State *s = memory;
    enter(s); s->cancelled = 1; wake(s, 1); notify(s, 1); leave(s);
}
int32_t dodo_sync_barrier_init(void *memory, size_t parties) {
    if (!parties) return 4;
    State *s = memory;
    s->parties = parties;
    return 0;
}
int32_t dodo_sync_barrier_wait(void *memory, uint64_t timeout) {
    State *s = memory;
    uint64_t end = deadline(timeout);
    enter(s);
    if (s->broken || s->cancelled) { leave(s); return 2; }
    size_t generation = s->generation;
    if (++s->arrived == s->parties) {
        s->arrived = 0; ++s->generation; wake(s, 1); leave(s); return 0;
    }
    int result = 0;
    while (generation == s->generation && !s->broken && !s->cancelled) {
        result = wait_until(s, end);
        if (result && generation == s->generation) {
            s->broken = 1; wake(s, 1); break;
        }
        result = 0;
    }
    if (!result && (s->broken || s->cancelled)) result = 2;
    leave(s);
    return result;
}
int32_t dodo_sync_channel_init(void *memory, void *ring, size_t capacity, size_t stride) {
    if (!capacity || !ring || (stride && capacity > SIZE_MAX / stride)) return 4;
    State *s = memory;
    s->ring = ring; s->capacity = capacity; s->stride = stride;
    return 0;
}
int32_t dodo_sync_send(void *memory, const void *value, uint64_t timeout) {
    State *s = memory;
    uint64_t end = deadline(timeout);
    enter(s);
    int result = 0;
    while (s->count == s->capacity && !s->closed && !s->cancelled) {
        result = wait_until(s, end);
        if (result) break;
    }
    if (s->closed) result = 3;
    else if (s->cancelled) result = 2;
    if (!result) {
        size_t index = (s->head + s->count) % s->capacity;
        memcpy(s->ring + index * s->stride, value, s->stride);
        ++s->count; wake(s, 1);
    }
    leave(s);
    return result;
}
int32_t dodo_sync_receive(void *memory, void *value, uint64_t timeout) {
    State *s = memory;
    uint64_t end = deadline(timeout);
    enter(s);
    int result = 0;
    while (!s->count && !s->closed && !s->cancelled) {
        result = wait_until(s, end);
        if (result) break;
    }
    /* Buffered messages remain drainable after close/cancellation. */
    if (s->count) {
        memcpy(value, s->ring + s->head * s->stride, s->stride);
        s->head = (s->head + 1) % s->capacity; --s->count;
        wake(s, 1); result = 0;
    } else if (s->closed) result = 3;
    else if (s->cancelled) result = 2;
    leave(s);
    return result;
}
void dodo_sync_close(void *memory) {
    State *s = memory;
    enter(s); s->closed = 1; wake(s, 1); leave(s);
}
/* Explicit page allocator. Each allocation is independent of libc malloc and
 * survives moving its Dodo owner. Payload begins after a 256-byte header. */
void *dodo_sync_allocate(size_t payload, size_t alignment) {
    if (!alignment || alignment > 256 || (alignment & (alignment - 1)) || payload > SIZE_MAX - 256) return NULL;
    size_t bytes = payload + 256;
#ifdef _WIN32
    void *p = VirtualAlloc(NULL, bytes, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
#else
    void *p = mmap(NULL, bytes, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (p == MAP_FAILED) p = NULL;
#endif
    if (!p) return NULL;
    if (dodo_sync_init(p, 256)) {
#ifdef _WIN32
        VirtualFree(p, 0, MEM_RELEASE);
#else
        munmap(p, bytes);
#endif
        return NULL;
    }
    ((State *)p)->mapping_size = bytes;
    return p;
}
void dodo_sync_retain(void *memory) {
    State *s = memory;
    enter(s);
    if (s->references == SIZE_MAX) __builtin_trap();
    ++s->references;
    leave(s);
}
int32_t dodo_sync_release(void *memory) {
    State *s = memory;
    enter(s); int last = --s->references == 0; leave(s);
    return last;
}
void dodo_sync_free(void *memory) {
    State *s = memory;
    size_t bytes = s->mapping_size;
    dodo_sync_destroy(memory);
#ifdef _WIN32
    (void)bytes;
    if (!VirtualFree(memory, 0, MEM_RELEASE)) __builtin_trap();
#else
    if (munmap(memory, bytes)) __builtin_trap();
#endif
}
