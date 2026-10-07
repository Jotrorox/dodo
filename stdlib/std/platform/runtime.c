/* A signal set is an OS ABI object. Keep its layout and pthread integration in
 * C instead of reproducing C-library internals in portable Dodo contracts. */
#if !defined(_WIN32)
#define _POSIX_C_SOURCE 200809L
#include <errno.h>
#include <pthread.h>
#include <signal.h>
#include <stddef.h>
#include <stdint.h>
#include <sys/types.h>
#include <time.h>
#include <unistd.h>

/* errno is a thread-local lvalue whose accessor differs between C libraries
 * (__errno_location on glibc, __error on Darwin). */
int32_t dodo_platform_errno(void) {
    return errno;
}

void dodo_platform_clear_errno(void) {
    errno = 0;
}

/* Stable tags for std/platform/error.Kind; see from_code in
 * std/platform/posix.dodo. errno values differ between Linux and Darwin, so
 * classify by name. Comparisons (not a switch) tolerate aliases such as
 * EWOULDBLOCK == EAGAIN and ENOTSUP == EOPNOTSUPP. */
uint32_t dodo_platform_error_tag(int32_t code) {
    if (code == ENOENT) return 1;
    if (code == EEXIST) return 2;
    if (code == EPERM || code == EACCES) return 3;
    if (code == EINTR) return 4;
    if (code == EAGAIN || code == EWOULDBLOCK) return 5;
    if (code == EXDEV) return 6;
    if (code == ENOTDIR) return 7;
    if (code == EISDIR) return 8;
    /* EILSEQ: APFS and HFS+ reject file names that are not UTF-8. */
    if (code == EINVAL || code == EILSEQ) return 9;
    if (code == ENOSYS || code == ENOTSUP || code == EOPNOTSUPP) return 10;
    if (code == ENOTEMPTY) return 11;
    if (code == ETIMEDOUT) return 12;
    if (code == ECANCELED) return 13;
    if (code == ERANGE || code == ENAMETOOLONG) return 14;
    if (code == EPIPE) return 15;
    if (code == EBADF) return 16;
    return 0;
}

/* Consume the SIGPIPE that a failed write generated for this thread while the
 * signal was blocked. Linux and Darwin both direct a write's SIGPIPE at the
 * writing thread. Darwin has no sigtimedwait, but the signal is already
 * pending, so sigwait returns without blocking. */
static void consume_sigpipe(const sigset_t *blocked) {
#if defined(__APPLE__)
    sigset_t pending;
    int signal_number;
    if (sigpending(&pending) == 0 && sigismember(&pending, SIGPIPE) == 1) {
        (void)sigwait(blocked, &signal_number);
    }
#else
    struct timespec zero = {0, 0};
    while (sigtimedwait(blocked, NULL, &zero) < 0 && errno == EINTR) {}
#endif
}

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
        consume_sigpipe(&blocked);
    }
    (void)pthread_sigmask(SIG_SETMASK, &previous, NULL);
    errno = saved;
    return result;
}
#endif
