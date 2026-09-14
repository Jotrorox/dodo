/* Dodo BSD-2-Clause socket ABI. Synchronous nonblocking syscalls never retain
 * caller pointers. Native address layouts are compiled against target headers. */
#ifndef _WIN32
#define _GNU_SOURCE
#endif
#define _POSIX_C_SOURCE 200809L
#include <stdint.h>
#include <stddef.h>
#include <string.h>
#include <limits.h>
#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#include <winsock2.h>
#include <ws2tcpip.h>
#include <windows.h>
typedef SOCKET dodo_socket;
typedef int dodo_socklen;
static INIT_ONCE startup_once = INIT_ONCE_STATIC_INIT;
static int startup_error;
static BOOL CALLBACK start_winsock(PINIT_ONCE once, PVOID arg, PVOID *context) {
    WSADATA data; (void)once; (void)arg; (void)context;
    startup_error = WSAStartup(MAKEWORD(2,2), &data); return TRUE;
}
static int initialize(void) { InitOnceExecuteOnce(&startup_once, start_winsock, NULL, NULL); return startup_error; }
static int last_error(void) { return WSAGetLastError(); }
static int socket_close(dodo_socket s) { return closesocket(s); }
static int set_nonblocking(dodo_socket s) { u_long value = 1; return ioctlsocket(s, FIONBIO, &value); }
#else
#include <sys/types.h>
#include <sys/socket.h>
#include <netinet/in.h>
#include <netdb.h>
#include <poll.h>
#include <fcntl.h>
#include <unistd.h>
#include <errno.h>
#include <time.h>
typedef int dodo_socket;
typedef socklen_t dodo_socklen;
static int initialize(void) { return 0; }
static int last_error(void) { return errno; }
static int socket_close(dodo_socket s) { return close(s); }
static int set_nonblocking(dodo_socket s) {
    int flags = fcntl(s, F_GETFL, 0);
    if (flags < 0 || fcntl(s, F_SETFL, flags | O_NONBLOCK) < 0) return -1;
    return fcntl(s, F_SETFD, FD_CLOEXEC);
}
#endif
/* Stable category tags independent of errno/WSA numeric values. */
uint32_t dodo_net_error_kind(int32_t error) {
    if (error == -1000001) return 12; /* name not found */
    if (error == -1000002) return 13; /* exhausted */
    if (error == -1000003) return 14; /* caller output exhausted */
#ifdef _WIN32
    switch(error) {
    case WSAEWOULDBLOCK: case WSAEINPROGRESS: case WSAEALREADY: return 1;
    case WSAEINTR: return 2;
    case WSAETIMEDOUT: return 3;
    case WSAENOTSOCK: case WSAESHUTDOWN: return 4;
    case WSAECONNRESET: case WSAECONNABORTED: return 5;
    case WSAECONNREFUSED: return 6;
    case WSAEADDRINUSE: return 7;
    case WSAENETUNREACH: case WSAEHOSTUNREACH: return 8;
    case WSAEINVAL: case WSAEAFNOSUPPORT: case WSAEMSGSIZE: return 9;
    case WSAENOBUFS: case WSAEMFILE: return 13;
    }
#else
    switch(error) {
    case EAGAIN: case EINPROGRESS: case EALREADY: return 1;
    case EINTR: return 2;
    case ETIMEDOUT: return 3;
    case EBADF: case ENOTSOCK: return 4;
    case ECONNRESET: case ECONNABORTED: case EPIPE: return 5;
    case ECONNREFUSED: return 6;
    case EADDRINUSE: return 7;
    case ENETUNREACH: case EHOSTUNREACH: return 8;
    case EINVAL: case EAFNOSUPPORT: case EMSGSIZE: return 9;
    case ENOMEM: case ENOBUFS: case EMFILE: case ENFILE: return 13;
    }
#endif
    return 0;
}
static dodo_socklen decode_address(const uint8_t *wire, struct sockaddr_storage *storage) {
    memset(storage, 0, sizeof(*storage));
    if (wire[0] == 4) {
        struct sockaddr_in *a = (struct sockaddr_in *)storage;
        a->sin_family = AF_INET; memcpy(&a->sin_port, wire+2, 2); memcpy(&a->sin_addr, wire+8, 4);
        return sizeof(*a);
    }
    struct sockaddr_in6 *a = (struct sockaddr_in6 *)storage;
    a->sin6_family = AF_INET6; memcpy(&a->sin6_port, wire+2, 2); memcpy(&a->sin6_addr, wire+8, 16);
    a->sin6_scope_id = (uint32_t)wire[4] | ((uint32_t)wire[5]<<8) | ((uint32_t)wire[6]<<16) | ((uint32_t)wire[7]<<24);
    return sizeof(*a);
}
static int encode_address(const struct sockaddr *a, uint8_t *wire) {
    memset(wire, 0, 24);
    if (a->sa_family == AF_INET) {
        const struct sockaddr_in *v4 = (const struct sockaddr_in *)a;
        wire[0] = 4; memcpy(wire+2, &v4->sin_port, 2); memcpy(wire+8, &v4->sin_addr, 4); return 1;
    }
    if (a->sa_family == AF_INET6) {
        const struct sockaddr_in6 *v6 = (const struct sockaddr_in6 *)a;
        wire[0] = 6; memcpy(wire+2, &v6->sin6_port, 2); memcpy(wire+8, &v6->sin6_addr, 16);
        uint32_t scope = v6->sin6_scope_id;
        for (unsigned i=0; i<4; i++) wire[4+i] = (uint8_t)(scope>>(i*8));
        return 1;
    }
    return 0;
}
intptr_t dodo_net_open(const uint8_t *wire, uint32_t mode, uint32_t backlog, int32_t *error) {
    *error = initialize(); if (*error) return -1;
    struct sockaddr_storage address; dodo_socklen length = decode_address(wire, &address);
    #ifdef _WIN32
    dodo_socket socket_value = WSASocketW(address.ss_family, mode == 2 ? SOCK_DGRAM : SOCK_STREAM, 0, NULL, 0, WSA_FLAG_NO_HANDLE_INHERIT);
#else
    dodo_socket socket_value = socket(address.ss_family, (mode == 2 ? SOCK_DGRAM : SOCK_STREAM) | SOCK_NONBLOCK | SOCK_CLOEXEC, 0);
#endif
    if ((intptr_t)socket_value == -1) { *error = last_error(); return -1; }
    if (set_nonblocking(socket_value) != 0) goto failed;
#ifdef _WIN32
    if (!SetHandleInformation((HANDLE)socket_value, HANDLE_FLAG_INHERIT, 0)) {
        *error = (int)GetLastError(); socket_close(socket_value); return -1;
    }
#endif
    /* IPv6 listeners are explicitly IPv6-only; IPv4 has an independent bind. */
    if (mode != 0 && wire[0] == 6) {
        int value = 1;
        if (setsockopt(socket_value, IPPROTO_IPV6, IPV6_V6ONLY, (const char *)&value, sizeof(value)) != 0) goto failed;
    }
    if (mode == 0) {
        if (connect(socket_value, (struct sockaddr *)&address, length) != 0) {
            int code = last_error(); if (dodo_net_error_kind(code) != 1) { *error = code; socket_close(socket_value); return -1; }
        }
    } else {
        if (bind(socket_value, (struct sockaddr *)&address, length) != 0) goto failed;
        if (mode == 1 && listen(socket_value, backlog > INT_MAX ? INT_MAX : (int)backlog) != 0) goto failed;
    }
    return (intptr_t)socket_value;
failed:
    *error = last_error(); socket_close(socket_value); return -1;
}
int32_t dodo_net_close(intptr_t raw) { if (socket_close((dodo_socket)raw) != 0) return last_error(); return 0; }
intptr_t dodo_net_accept(intptr_t raw, int32_t *error) {
#ifdef _WIN32
    dodo_socket accepted = accept((dodo_socket)raw, NULL, NULL);
#else
    dodo_socket accepted = accept4((dodo_socket)raw, NULL, NULL, SOCK_NONBLOCK | SOCK_CLOEXEC);
#endif
    *error = 0;
    if ((intptr_t)accepted == -1) { *error = last_error(); return -1; }
    if (set_nonblocking(accepted) != 0) { *error = last_error(); socket_close(accepted); return -1; }
#ifdef _WIN32
    if (!SetHandleInformation((HANDLE)accepted, HANDLE_FLAG_INHERIT, 0)) {
        *error = (int)GetLastError(); socket_close(accepted); return -1;
    }
#endif
    return (intptr_t)accepted;
}
int32_t dodo_net_address(intptr_t raw, uint8_t *wire, uint32_t peer) {
    struct sockaddr_storage address; dodo_socklen length = sizeof(address);
    int result = peer ? getpeername((dodo_socket)raw, (struct sockaddr *)&address, &length) : getsockname((dodo_socket)raw, (struct sockaddr *)&address, &length);
    if (result != 0) return last_error();
    return encode_address((struct sockaddr *)&address, wire) ? 0 : -1000001;
}
/* Readiness says retry the operation, including when an error/hangup is ready. */
int32_t dodo_net_wait(intptr_t raw, uint32_t writing, uint32_t milliseconds, int32_t *error) {
    *error = 0;
#ifdef _WIN32
    WSAPOLLFD item; item.fd = (SOCKET)raw; item.events = writing ? POLLWRNORM : POLLRDNORM; item.revents = 0;
    int result = WSAPoll(&item, 1, milliseconds > INT_MAX ? INT_MAX : (int)milliseconds);
#else
    struct pollfd item; item.fd = (int)raw; item.events = writing ? POLLOUT : POLLIN; item.revents = 0;
    int result = poll(&item, 1, milliseconds > INT_MAX ? INT_MAX : (int)milliseconds);
#endif
    if (result < 0) { *error = last_error(); return -1; }
    return result ? 1 : 0;
}
int32_t dodo_net_connected(intptr_t raw, int32_t *error) {
    int ready = dodo_net_wait(raw, 1, 0, error); if (ready <= 0) return ready;
    int code = 0; dodo_socklen length = sizeof(code);
    if (getsockopt((dodo_socket)raw, SOL_SOCKET, SO_ERROR, (char *)&code, &length) != 0) { *error = last_error(); return -1; }
    if (code) { *error = code; return -1; }
    return 1;
}
intptr_t dodo_net_read(intptr_t raw, uint8_t *buffer, size_t capacity, int32_t *error) {
    size_t count = capacity > INT_MAX ? INT_MAX : capacity;
    intptr_t result = recv((dodo_socket)raw, (char *)buffer, (int)count, 0);
    *error = result < 0 ? last_error() : 0; return result;
}
intptr_t dodo_net_write(intptr_t raw, const uint8_t *buffer, size_t length, int32_t *error) {
    size_t count = length > INT_MAX ? INT_MAX : length;
#ifdef _WIN32
    int flags = 0;
#else
    int flags = MSG_NOSIGNAL;
#endif
    intptr_t result = send((dodo_socket)raw, (const char *)buffer, (int)count, flags);
    *error = result < 0 ? last_error() : 0; return result;
}
int32_t dodo_net_shutdown(intptr_t raw, uint32_t direction) {
    if (shutdown((dodo_socket)raw, (int)direction) != 0) return last_error();
    return 0;
}
intptr_t dodo_net_sendto(intptr_t raw, const uint8_t *buffer, size_t length, const uint8_t *wire, int32_t *error) {
    struct sockaddr_storage address; dodo_socklen size = decode_address(wire, &address);
    if (length > INT_MAX) { *error = -1000003; return -1; }
    intptr_t result = sendto((dodo_socket)raw, (const char *)buffer, (int)length, 0, (struct sockaddr *)&address, size);
    *error = result < 0 ? last_error() : 0; return result;
}
intptr_t dodo_net_recvfrom(intptr_t raw, uint8_t *buffer, size_t capacity, uint8_t *wire, uint32_t *truncated, size_t *original, int32_t *error) {
    struct sockaddr_storage address; dodo_socklen size = sizeof(address);
    size_t count = capacity > INT_MAX ? INT_MAX : capacity;
    *truncated = 0; *original = 0; *error = 0;
#ifdef _WIN32
    WSABUF data; data.buf = (char *)buffer; data.len = (ULONG)count;
    DWORD bytes = 0, flags = 0;
    int result = WSARecvFrom((SOCKET)raw, &data, 1, &bytes, &flags, (struct sockaddr *)&address, &size, NULL, NULL);
    if (result == SOCKET_ERROR) {
        *error = last_error();
        if (*error != WSAEMSGSIZE) return -1;
        *error = 0; *truncated = 1; *original = SIZE_MAX;
        /* Winsock supplies the copied prefix before reporting message size. */
        bytes = (DWORD)count;
    } else *original = bytes;
    encode_address((struct sockaddr *)&address, wire); return bytes;
#else
    intptr_t result = recvfrom((int)raw, buffer, count, MSG_TRUNC, (struct sockaddr *)&address, &size);
    if (result < 0) { *error = last_error(); return -1; }
    *original = (size_t)result; *truncated = (size_t)result > count;
    encode_address((struct sockaddr *)&address, wire); return (size_t)result > count ? (intptr_t)count : result;
#endif
}
/* Resolver is explicitly synchronous; libc/Winsock owns its internal allocation
 * until freeaddrinfo. Host bytes are already validated and NUL terminated. */
intptr_t dodo_net_resolve(const uint8_t *hostname, uint16_t port, uint8_t *output, size_t capacity, uint32_t numeric_only, int32_t *error) {
    *error = initialize(); if (*error) return -1;
    struct addrinfo hints, *result = NULL; memset(&hints, 0, sizeof(hints)); hints.ai_family = AF_UNSPEC; hints.ai_socktype = SOCK_STREAM;
    if (numeric_only) hints.ai_flags = AI_NUMERICHOST;
    int code = getaddrinfo((const char *)hostname, NULL, &hints, &result);
    if (code != 0) {
        *error = code == EAI_NONAME ? -1000001 : code == EAI_MEMORY ? -1000002 : code;
        return -1;
    }
    size_t count = 0;
    for (struct addrinfo *item = result; item; item = item->ai_next) {
        uint8_t wire[24]; if (!encode_address(item->ai_addr, wire)) continue;
        wire[2] = (uint8_t)(port>>8); wire[3] = (uint8_t)port;
        int duplicate = 0;
        for (size_t i=0; i<count; i++) if (!memcmp(output+i*24, wire, 24)) duplicate = 1;
        if (duplicate) continue;
        if (count == capacity) { *error = -1000003; break; }
        memcpy(output+count*24, wire, 24); count++;
    }
    freeaddrinfo(result); return (intptr_t)count;
}
