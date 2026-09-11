/* Force the engine-owned staging allocation to fail, before any session exists.
 * OpenSSL's own smaller global allocations remain available and are cleaned up. */
#include <openssl/crypto.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>

void *__real_CRYPTO_zalloc(size_t, const char *, int);
void *__wrap_CRYPTO_zalloc(size_t size, const char *file, int line) {
    if (size >= 16384 && size < 17408 && strstr(file, "runtime.c")) return NULL;
    return __real_CRYPTO_zalloc(size, file, line);
}
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
