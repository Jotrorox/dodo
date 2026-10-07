/* Force the engine-owned staging allocation to fail, before any session exists.
 * OpenSSL's own smaller global allocations remain available and are cleaned up. */
#include <openssl/crypto.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>

static int fails(size_t size, const char *file) {
    return size >= 16384 && size < 17408 && strstr(file, "runtime.c");
}
#if defined(__APPLE__)
/* ld64 has no --wrap. A definition in the executable binds the TLS runtime's
 * calls; two-level namespaces keep libcrypto's internal calls on its own. */
#include <dlfcn.h>
void *CRYPTO_zalloc(size_t size, const char *file, int line) {
    typedef void *(*allocator)(size_t, const char *, int);
    static allocator real;
    if (fails(size, file)) return NULL;
    if (!real) *(void **)&real = dlsym(RTLD_NEXT, "CRYPTO_zalloc");
    return real(size, file, line);
}
#else
void *__real_CRYPTO_zalloc(size_t, const char *, int);
void *__wrap_CRYPTO_zalloc(size_t size, const char *file, int line) {
    if (fails(size, file)) return NULL;
    return __real_CRYPTO_zalloc(size, file, line);
}
#endif
extern void *dodo_tls_new(int, const unsigned char *, size_t, const unsigned char *, size_t,
                         const unsigned char *, size_t, const unsigned char *, size_t,
                         const unsigned char *, size_t, int, int, int64_t, int *);
extern void dodo_tls_free(void *);
int main(void) {
    int error = 0;
    void *engine = dodo_tls_new(0, (const unsigned char *)"localhost", 9,
                               NULL, 0, NULL, 0, NULL, 0, NULL, 0, 0, 0, -1, &error);
    if (engine || error != -2) { dodo_tls_free(engine); return 1; }
    dodo_tls_free(NULL);
    OPENSSL_cleanup();
    return 0;
}
