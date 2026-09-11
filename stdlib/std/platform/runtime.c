/* A signal set is an OS ABI object. Keep its layout and pthread integration in
 * C instead of reproducing glibc internals in portable Dodo contracts. */
#if !defined(_WIN32)
#define _POSIX_C_SOURCE 200809L
#include <errno.h>
#include <pthread.h>
#include <signal.h>
#include <stddef.h>
#include <sys/types.h>
#include <time.h>
#include <unistd.h>

/* Preserve the caller's signal mask and any preexisting pending SIGPIPE. A
 * write-generated SIGPIPE becomes EPIPE without changing process-global signal
 * dispositions or consuming a signal that preceded this operation. */
ssize_t dodo_platform_write(int fd, const unsigned char *data, size_t count) {
    sigset_t blocked, previous, pending;
    sigemptyset(&blocked);
    sigaddset(&blocked, SIGPIPE);
    int code = pthread_sigmask(SIG_BLOCK, &blocked, &previous);
    if (code != 0) {
        errno = code;
        return -1;
    }
    if (sigpending(&pending) != 0) {
        int saved = errno;
        (void)pthread_sigmask(SIG_SETMASK, &previous, NULL);
        errno = saved;
        return -1;
    }
    int was_pending = sigismember(&pending, SIGPIPE);
    ssize_t result = write(fd, data, count);
    int saved = errno;
    if (result < 0 && saved == EPIPE && !was_pending) {
        struct timespec zero = {0, 0};
        while (sigtimedwait(&blocked, NULL, &zero) < 0 && errno == EINTR) {}
    }
    (void)pthread_sigmask(SIG_SETMASK, &previous, NULL);
    errno = saved;
    return result;
}
#endif
