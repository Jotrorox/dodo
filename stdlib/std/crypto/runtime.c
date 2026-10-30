/* Operating-system CSPRNG only: no OpenSSL, user-space generator or fallback
 * source. A failure is reported, never replaced by weaker bytes. */
#if defined(__linux__) && !defined(_DEFAULT_SOURCE)
#define _DEFAULT_SOURCE
#endif
#include <stddef.h>
#include <stdint.h>
#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <bcrypt.h>
int32_t dodo_crypto_random(unsigned char *bytes, size_t count) {
    while (count) {
        ULONG chunk = count > 0x40000000u ? 0x40000000u : (ULONG)count;
        NTSTATUS status = BCryptGenRandom(NULL, bytes, chunk, BCRYPT_USE_SYSTEM_PREFERRED_RNG);
        if (status < 0) return (int32_t)status;
        bytes += chunk; count -= chunk;
    }
    return 0;
}
#elif defined(__APPLE__)
#include <Security/SecRandom.h>
int32_t dodo_crypto_random(unsigned char *bytes, size_t count) {
    while (count) {
        size_t chunk = count > 0x40000000u ? 0x40000000u : count;
        int status = SecRandomCopyBytes(kSecRandomDefault, chunk, bytes);
        if (status != errSecSuccess) return status ? (int32_t)status : -1;
        bytes += chunk; count -= chunk;
    }
    return 0;
}
#else
#include <errno.h>
#include <sys/random.h>
int32_t dodo_crypto_random(unsigned char *bytes, size_t count) {
    while (count) {
        /* Flags 0 block until the kernel pool is initialized, then never block. */
        size_t chunk = count > 33554431u ? 33554431u : count;
        ssize_t result = getrandom(bytes, chunk, 0);
        if (result < 0) {
            if (errno == EINTR) continue;
            return errno ? errno : -1;
        }
        bytes += result; count -= (size_t)result;
    }
    return 0;
}
#endif
