#define _GNU_SOURCE
#include <pthread.h>
#include <stdatomic.h>
#include <sys/mman.h>
#include <errno.h>
#include <stdint.h>

static _Atomic int mode;
static _Atomic int drops;
static _Atomic int maps;
static _Atomic int frees;
void dodo_thread_test_mode(int32_t value) { atomic_store(&mode, value); }
void dodo_thread_test_drop(void) { atomic_fetch_add(&drops, 1); }
int32_t dodo_thread_test_drops(void) { return atomic_load(&drops); }
int32_t dodo_thread_test_maps(void) { return atomic_load(&maps); }
int32_t dodo_thread_test_frees(void) { return atomic_load(&frees); }
#if defined(__APPLE__)
/* ld64 has no --wrap. Definitions in the executable bind the Dodo runtime's
 * calls; two-level namespaces keep libSystem's internal calls on its own. */
#include <dlfcn.h>
#define REAL(name) real_##name
#define LOOKUP(name) do { if (!real_##name) *(void **)&real_##name = dlsym(RTLD_NEXT, #name); } while (0)
static int (*real_pthread_create)(pthread_t *, const pthread_attr_t *, void *(*)(void *), void *);
static void *(*real_mmap)(void *, size_t, int, int, int, off_t);
static int (*real_munmap)(void *, size_t);
#define WRAP(name) name
#else
#define REAL(name) __real_##name
#define LOOKUP(name) do {} while (0)
int __real_pthread_create(pthread_t *, const pthread_attr_t *, void *(*)(void *), void *);
void *__real_mmap(void *, size_t, int, int, int, off_t);
int __real_munmap(void *, size_t);
#define WRAP(name) __wrap_##name
#endif
int WRAP(pthread_create)(pthread_t *thread, const pthread_attr_t *attributes,
                         void *(*entry)(void *), void *argument) {
    if (atomic_load(&mode) == 1) return EAGAIN;
    LOOKUP(pthread_create);
    return REAL(pthread_create)(thread, attributes, entry, argument);
}
void *WRAP(mmap)(void *address, size_t size, int protection, int flags, int descriptor, off_t offset) {
    if (atomic_load(&mode) == 2) { errno = ENOMEM; return MAP_FAILED; }
    LOOKUP(mmap);
    void *result = REAL(mmap)(address, size, protection, flags, descriptor, offset);
    if (result != MAP_FAILED) atomic_fetch_add(&maps, 1);
    return result;
}
int WRAP(munmap)(void *address, size_t size) {
    LOOKUP(munmap);
    int result = REAL(munmap)(address, size);
    if (!result) atomic_fetch_add(&frees, 1);
    return result;
}
