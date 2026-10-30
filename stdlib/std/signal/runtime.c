/* Dodo BSD-2-Clause signal ABI. Handlers only touch lock-free atomics and the
 * write end of a nonblocking wake channel, so nothing they do can block or
 * allocate. The channel's read end is pollable by std/net PollSet: a pipe on
 * POSIX, a loopback socket pair on Windows (WSAPoll accepts only sockets).
 *
 * Each delivery queues a one-byte tag, which preserves arrival order and keeps
 * the channel readable exactly while signals are pending. A tag that does not
 * fit in a full channel is counted instead and queued once space frees up.
 *
 * The channel is created once and kept for the process lifetime: a handler
 * already running on another thread can never write to a descriptor that
 * close() released and the system reused. */
#if !defined(_WIN32)
#if defined(__APPLE__)
#define _DARWIN_C_SOURCE
#else
/* SA_RESTART is an XSI extension hidden by strict POSIX mode. */
#define _GNU_SOURCE
#define _POSIX_C_SOURCE 200809L
#endif
#endif
#include <stdint.h>
#include <stdatomic.h>
#include <limits.h>
#include <string.h>

/* Bits and tags match std/signal Signal ordinals. */
enum { INTERRUPT = 0, TERMINATE = 1, HANGUP = 2, BREAK = 3, KINDS = 4 };

static atomic_uint active;
static atomic_uint selected;
static atomic_uint shutdown_requested;
static atomic_uint overflow[KINDS];

static int take_overflow(uint32_t *signal) {
    for (unsigned i = 0; i < KINDS; ++i) {
        unsigned count = atomic_load(&overflow[i]);
        while (count != 0) {
            if (atomic_compare_exchange_weak(&overflow[i], &count, count - 1)) {
                *signal = i; return 1;
            }
        }
    }
    return 0;
}
static void reset_pending(void) {
    for (unsigned i = 0; i < KINDS; ++i) atomic_store(&overflow[i], 0);
    atomic_store(&shutdown_requested, 0);
}

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#include <winsock2.h>
#include <ws2tcpip.h>
#include <windows.h>
static INIT_ONCE startup_once = INIT_ONCE_STATIC_INIT;
static int startup_error;
static SOCKET wake_read = INVALID_SOCKET, wake_write = INVALID_SOCKET;
static BOOL CALLBACK start_winsock(PINIT_ONCE once, PVOID arg, PVOID *context) {
    WSADATA data; (void)once; (void)arg; (void)context;
    startup_error = WSAStartup(MAKEWORD(2,2), &data); return TRUE;
}
static int same_endpoint(const struct sockaddr_in *left, const struct sockaddr_in *right) {
    return left->sin_family == right->sin_family && left->sin_port == right->sin_port
        && left->sin_addr.s_addr == right->sin_addr.s_addr;
}
/* A connected loopback pair. Another local process could connect to the
 * listener first, so the accepted peer must be our own connecting socket. */
static int make_channel(void) {
    InitOnceExecuteOnce(&startup_once, start_winsock, NULL, NULL);
    if (startup_error) return startup_error;
    SOCKET listener = socket(AF_INET, SOCK_STREAM, IPPROTO_TCP), client = INVALID_SOCKET, server = INVALID_SOCKET;
    struct sockaddr_in address, local, peer; int length = sizeof(address);
    u_long nonblocking = 1; BOOL nodelay = TRUE; int code = 0;
    if (listener == INVALID_SOCKET) return WSAGetLastError();
    memset(&address, 0, sizeof(address));
    address.sin_family = AF_INET; address.sin_addr.s_addr = htonl(INADDR_LOOPBACK); address.sin_port = 0;
    if (bind(listener, (struct sockaddr *)&address, sizeof(address)) != 0
        || getsockname(listener, (struct sockaddr *)&address, &length) != 0
        || listen(listener, 1) != 0) goto failed;
    client = socket(AF_INET, SOCK_STREAM, IPPROTO_TCP);
    if (client == INVALID_SOCKET || connect(client, (struct sockaddr *)&address, sizeof(address)) != 0) goto failed;
    length = sizeof(local);
    if (getsockname(client, (struct sockaddr *)&local, &length) != 0) goto failed;
    length = sizeof(peer);
    server = accept(listener, (struct sockaddr *)&peer, &length);
    if (server == INVALID_SOCKET) goto failed;
    if (!same_endpoint(&local, &peer)) { code = WSAEACCES; goto failed; }
    if (ioctlsocket(server, FIONBIO, &nonblocking) != 0 || ioctlsocket(client, FIONBIO, &nonblocking) != 0
        || setsockopt(client, IPPROTO_TCP, TCP_NODELAY, (const char *)&nodelay, sizeof(nodelay)) != 0) goto failed;
    (void)SetHandleInformation((HANDLE)server, HANDLE_FLAG_INHERIT, 0);
    (void)SetHandleInformation((HANDLE)client, HANDLE_FLAG_INHERIT, 0);
    closesocket(listener);
    wake_read = server; wake_write = client;
    return 0;
failed:
    if (!code) code = WSAGetLastError();
    if (server != INVALID_SOCKET) closesocket(server);
    if (client != INVALID_SOCKET) closesocket(client);
    closesocket(listener);
    return code ? code : WSAEINVAL;
}
static void notify(unsigned kind) {
    char tag = (char)kind;
    if (send(wake_write, &tag, 1, 0) != 1) atomic_fetch_add(&overflow[kind], 1);
}
static int read_tag(uint32_t *signal) {
    char tag;
    for (;;) {
        int count = recv(wake_read, &tag, 1, 0);
        if (count != 1) return 0;
        if ((unsigned char)tag < KINDS) { *signal = (unsigned char)tag; return 1; }
    }
}
/* Runs on a thread the system creates for each event. Returning FALSE passes
 * an unselected event to the next handler, ultimately the default exit. */
static BOOL WINAPI on_control(DWORD type) {
    unsigned kind;
    switch (type) {
    case CTRL_C_EVENT: kind = INTERRUPT; break;
    case CTRL_BREAK_EVENT: kind = BREAK; break;
    case CTRL_CLOSE_EVENT: case CTRL_LOGOFF_EVENT: case CTRL_SHUTDOWN_EVENT: kind = TERMINATE; break;
    default: return FALSE;
    }
    if (!atomic_load(&active) || !(atomic_load(&selected) & (1u << kind))) return FALSE;
    if (kind != BREAK) atomic_store(&shutdown_requested, 1);
    notify(kind);
    /* The system ends the process as soon as a close, logoff or shutdown
     * handler returns. Waiting lets the program shut down and exit itself
     * until the system's own deadline. */
    if (kind == TERMINATE) Sleep(INFINITE);
    return TRUE;
}
int32_t dodo_signal_listen(uint32_t mask, intptr_t *handle) {
    unsigned expected = 0;
    if (!atomic_compare_exchange_strong(&active, &expected, 1)) return -1;
    if (wake_read == INVALID_SOCKET) {
        int code = make_channel();
        if (code) { atomic_store(&active, 0); return code; }
    }
    uint32_t ignored;
    while (read_tag(&ignored)) {}
    reset_pending();
    atomic_store(&selected, mask);
    if (!SetConsoleCtrlHandler(on_control, TRUE)) {
        DWORD code = GetLastError();
        atomic_store(&selected, 0); atomic_store(&active, 0);
        return (int32_t)(code ? code : ERROR_GEN_FAILURE);
    }
    *handle = (intptr_t)wake_read;
    return 0;
}
int32_t dodo_signal_close(void) {
    atomic_store(&selected, 0);
    int32_t code = 0;
    if (!SetConsoleCtrlHandler(on_control, FALSE)) code = (int32_t)GetLastError();
    atomic_store(&active, 0);
    return code;
}
int32_t dodo_signal_wait(uint32_t milliseconds) {
    WSAPOLLFD item; item.fd = wake_read; item.events = POLLRDNORM; item.revents = 0;
    int result = WSAPoll(&item, 1, milliseconds > INT_MAX ? INT_MAX : (int)milliseconds);
    if (result < 0) return -WSAGetLastError();
    return result ? 1 : 0;
}
#else
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <signal.h>
#include <unistd.h>
static int wake[2] = {-1, -1};
static struct sigaction previous[3];
static const int numbers[3] = {SIGINT, SIGTERM, SIGHUP};
static int configure(int fd) {
    int flags = fcntl(fd, F_GETFL);
    if (flags < 0 || fcntl(fd, F_SETFL, flags | O_NONBLOCK) != 0) return -1;
    return fcntl(fd, F_SETFD, FD_CLOEXEC);
}
/* pipe2 is not available on Darwin. Dodo's process spawning closes
 * undeclared descriptors in children, so the window before FD_CLOEXEC cannot
 * leak the channel into Dodo children. */
static int make_channel(void) {
    int fds[2];
    if (pipe(fds) != 0) return errno;
    if (configure(fds[0]) != 0 || configure(fds[1]) != 0) {
        int code = errno; close(fds[0]); close(fds[1]); return code;
    }
    wake[0] = fds[0]; wake[1] = fds[1];
    return 0;
}
static void notify(unsigned kind) {
    unsigned char tag = (unsigned char)kind;
    if (write(wake[1], &tag, 1) != 1) atomic_fetch_add(&overflow[kind], 1);
}
static int read_tag(uint32_t *signal) {
    unsigned char tag;
    for (;;) {
        ssize_t count = read(wake[0], &tag, 1);
        if (count == 1) {
            if (tag < KINDS) { *signal = tag; return 1; }
            continue;
        }
        if (count < 0 && errno == EINTR) continue;
        return 0;
    }
}
static void on_signal(int number) {
    int saved = errno;
    unsigned kind = number == SIGINT ? INTERRUPT : number == SIGTERM ? TERMINATE : HANGUP;
    if (kind != HANGUP) atomic_store(&shutdown_requested, 1);
    notify(kind);
    errno = saved;
}
static void restore(unsigned installed) {
    for (unsigned i = 0; i < 3; ++i) {
        if (installed & (1u << i)) (void)sigaction(numbers[i], &previous[i], NULL);
    }
}
int32_t dodo_signal_listen(uint32_t mask, intptr_t *handle) {
    unsigned expected = 0;
    if (!atomic_compare_exchange_strong(&active, &expected, 1)) return -1;
    if (wake[0] == -1) {
        int code = make_channel();
        if (code) { atomic_store(&active, 0); return code; }
    }
    uint32_t ignored;
    while (read_tag(&ignored)) {}
    reset_pending();
    atomic_store(&selected, mask);
    struct sigaction action;
    memset(&action, 0, sizeof(action));
    action.sa_handler = on_signal;
    /* Restarting keeps unrelated blocking reads and writes uninterrupted;
     * poll() still returns EINTR, which PollSet reports as Interrupted. */
    action.sa_flags = SA_RESTART;
    sigemptyset(&action.sa_mask);
    for (unsigned i = 0; i < 3; ++i) sigaddset(&action.sa_mask, numbers[i]);
    unsigned installed = 0;
    for (unsigned i = 0; i < 3; ++i) {
        if (!(mask & (1u << i))) continue;
        if (sigaction(numbers[i], &action, &previous[i]) != 0) {
            int code = errno;
            restore(installed);
            atomic_store(&selected, 0); atomic_store(&active, 0);
            return code;
        }
        installed |= 1u << i;
    }
    *handle = (intptr_t)wake[0];
    return 0;
}
int32_t dodo_signal_close(void) {
    restore(atomic_load(&selected) & 7u);
    atomic_store(&selected, 0);
    atomic_store(&active, 0);
    return 0;
}
int32_t dodo_signal_wait(uint32_t milliseconds) {
    struct pollfd item; item.fd = wake[0]; item.events = POLLIN; item.revents = 0;
    int result = poll(&item, 1, milliseconds > INT_MAX ? INT_MAX : (int)milliseconds);
    if (result < 0) return errno == EINTR ? 0 : -errno;
    return result ? 1 : 0;
}
#endif

/* Move overflowed tags back into the channel as it drains, so the channel
 * stays readable while any signal is pending. They arrived after every queued
 * tag, so appending them keeps arrival order. */
static void refill(void) {
    for (unsigned i = 0; i < KINDS; ++i) {
        unsigned count = atomic_load(&overflow[i]);
        while (count != 0) {
            if (!atomic_compare_exchange_weak(&overflow[i], &count, count - 1)) continue;
            unsigned char tag = (unsigned char)i;
#ifdef _WIN32
            int written = send(wake_write, (const char *)&tag, 1, 0) == 1;
#else
            int written = write(wake[1], &tag, 1) == 1;
#endif
            if (!written) { atomic_fetch_add(&overflow[i], 1); return; }
            count = atomic_load(&overflow[i]);
        }
    }
}
/* Tags are only queued while listening, so every one reports a selection. */
int32_t dodo_signal_next(uint32_t *signal) {
    int found = read_tag(signal) || take_overflow(signal);
    refill();
    return found;
}
uint32_t dodo_signal_shutdown_requested(void) {
    return atomic_load(&shutdown_requested);
}
