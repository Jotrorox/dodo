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
int __real_pthread_create(pthread_t *, const pthread_attr_t *, void *(*)(void *), void *);
int __wrap_pthread_create(pthread_t *thread, const pthread_attr_t *attributes,
                          void *(*entry)(void *), void *argument) {
    if (atomic_load(&mode) == 1) return EAGAIN;
    return __real_pthread_create(thread, attributes, entry, argument);
}
void *__real_mmap(void *, size_t, int, int, int, off_t);
void *__wrap_mmap(void *address, size_t size, int protection, int flags, int descriptor, off_t offset) {
    if (atomic_load(&mode) == 2) { errno = ENOMEM; return MAP_FAILED; }
    void *result = __real_mmap(address, size, protection, flags, descriptor, offset);
    if (result != MAP_FAILED) atomic_fetch_add(&maps, 1);
    return result;
}
int __real_munmap(void *, size_t);
int __wrap_munmap(void *address, size_t size) {
    int result = __real_munmap(address, size);
    if (!result) atomic_fetch_add(&frees, 1);
    return result;
}
