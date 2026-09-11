/* Dodo process boundary: no shell, caller-backed strings and output storage.
 * Only primitive/pointer values cross the Dodo/C ABI. */
#ifndef _GNU_SOURCE
#define _GNU_SOURCE
#endif
#include <stdint.h>
#include <stddef.h>
#include <stdlib.h>
#include <string.h>
#include <limits.h>

typedef struct {
    uintptr_t process;
    intptr_t input, output, error;
    int32_t done, kind, code;
} dodo_child;
_Static_assert(sizeof(void *) == 8 && sizeof(dodo_child) == 48, "Dodo process runtime requires x64 ABI");
_Static_assert(offsetof(dodo_child, done) == 32 && offsetof(dodo_child, code) == 40, "Dodo child ABI offsets");
/* Negative codes are portable runtime errors; positive codes are native. */
#define DODO_INVALID (-1)
#define DODO_TIMEOUT (-2)
#define DODO_LIMIT (-3)
#define DODO_CANCELLED (-4)
#define DODO_PENDING (-5)

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#include <windows.h>

static void close_stream(intptr_t *stream) {
    if (*stream != -1) { CloseHandle((HANDLE)*stream); *stream = -1; }
}
static int32_t winerr(void) { return (int32_t)GetLastError(); }
static int make_stream(int mode, int input, HANDLE *child, intptr_t *parent) {
    SECURITY_ATTRIBUTES security = {sizeof(security), NULL, TRUE};
    *parent = -1;
    if (mode == 0) {
        HANDLE source = GetStdHandle(input ? STD_INPUT_HANDLE : STD_OUTPUT_HANDLE);
        if (!source || source == INVALID_HANDLE_VALUE) {
            *child = CreateFileW(L"NUL", input ? GENERIC_READ : GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE, &security, OPEN_EXISTING, 0, NULL);
        } else if (!DuplicateHandle(GetCurrentProcess(), source, GetCurrentProcess(), child, 0, TRUE, DUPLICATE_SAME_ACCESS)) return winerr();
        return *child == INVALID_HANDLE_VALUE ? winerr() : 0;
    }
    if (mode == 2) {
        *child = CreateFileW(L"NUL", input ? GENERIC_READ : GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE, &security, OPEN_EXISTING, 0, NULL);
        return *child == INVALID_HANDLE_VALUE ? winerr() : 0;
    }
    HANDLE read_end, write_end;
    if (!CreatePipe(&read_end, &write_end, &security, 0)) return winerr();
    *child = input ? read_end : write_end;
    HANDLE local = input ? write_end : read_end;
    if (!SetHandleInformation(local, HANDLE_FLAG_INHERIT, 0)) {
        int code = winerr(); CloseHandle(read_end); CloseHandle(write_end); return code;
    }
    *parent = (intptr_t)local;
    return 0;
}
/* CRT argv quoting: quote every argument, double backslashes preceding quotes
 * and the closing quote. The executable path is passed separately. */
static int append_argument(wchar_t *line, size_t capacity, size_t *used, const wchar_t *arg) {
    size_t n = *used;
#define PUT(c) do { if (n + 1 >= capacity) return DODO_LIMIT; line[n++] = (c); } while (0)
    if (n) PUT(L' ');
    PUT(L'"');
    for (;;) {
        size_t slashes = 0;
        while (*arg == L'\\') { ++slashes; ++arg; }
        if (*arg == L'"' || !*arg) {
            for (size_t j = 0; j < slashes * 2; ++j) PUT(L'\\');
            if (!*arg) break;
            PUT(L'\\'); PUT(*arg++);
        } else {
            for (size_t j = 0; j < slashes; ++j) PUT(L'\\');
            PUT(*arg++);
        }
    }
    PUT(L'"'); line[n] = 0; *used = n;
#undef PUT
    return 0;
}
int32_t dodo_process_spawn(const void *exe_raw, const void *args_raw, size_t arg_units,
    const void *env_raw, size_t env_units, int32_t inherit_env, const void *cwd_raw,
    int32_t input, int32_t output, int32_t error, int32_t search_path, dodo_child *result) {
    (void)search_path; /* lpApplicationName deliberately selects the executable. */
    const wchar_t *exe = exe_raw, *args = args_raw, *env = env_raw;
    wchar_t command[32768]; size_t used = 0;
    int32_t code = append_argument(command, 32768, &used, exe);
    for (size_t i = 0; !code && i < arg_units;) {
        code = append_argument(command, 32768, &used, args + i);
        while (i < arg_units && args[i]) ++i;
        ++i;
    }
    if (code) return code;
    /* Windows environment blocks need a second terminator. */
    wchar_t environment[32768];
    if (!inherit_env) {
        if (env_units > 32766) return DODO_LIMIT;
        size_t total = 0;
        for (size_t start = 0; start < env_units;) {
            size_t length = 0;
            while (start + length < env_units && env[start + length]) ++length;
            ++length;
            size_t insertion = 0;
            while (insertion < total) {
                size_t existing = 0;
                while (environment[insertion + existing]) ++existing;
                if (CompareStringOrdinal(env + start, (int)(length - 1), environment + insertion,
                        (int)existing, TRUE) == CSTR_LESS_THAN) break;
                insertion += existing + 1;
            }
            memmove(environment + insertion + length, environment + insertion, (total - insertion) * sizeof(wchar_t));
            memcpy(environment + insertion, env + start, length * sizeof(wchar_t));
            total += length; start += length;
        }
        environment[env_units] = 0; environment[env_units + 1] = 0;
    }
    STARTUPINFOEXW start; PROCESS_INFORMATION info;
    memset(&start, 0, sizeof(start)); memset(&info, 0, sizeof(info));
    start.StartupInfo.cb = sizeof(start); start.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    HANDLE handles[3] = {INVALID_HANDLE_VALUE, INVALID_HANDLE_VALUE, INVALID_HANDLE_VALUE};
    intptr_t parents[3] = {-1, -1, -1};
    code = make_stream(input, 1, handles, parents);
    if (!code) code = make_stream(output, 0, handles + 1, parents + 1);
    if (!code && error == 0) {
        HANDLE source = GetStdHandle(STD_ERROR_HANDLE);
        if (!source || source == INVALID_HANDLE_VALUE) code = make_stream(2, 0, handles + 2, parents + 2);
        else if (!DuplicateHandle(GetCurrentProcess(), source, GetCurrentProcess(), handles + 2, 0, TRUE, DUPLICATE_SAME_ACCESS)) code = winerr();
    } else if (!code) code = make_stream(error, 0, handles + 2, parents + 2);
    /* Handle whitelist prevents inheritance of unrelated inheritable handles. */
    union { uintptr_t alignment; unsigned char data[1024]; } attributes;
    SIZE_T bytes = sizeof(attributes.data);
    start.lpAttributeList = (LPPROC_THREAD_ATTRIBUTE_LIST)(void *)attributes.data;
    int attributes_live = 0;
    if (!code) {
        if (!InitializeProcThreadAttributeList(start.lpAttributeList, 1, 0, &bytes)) code = winerr();
        else attributes_live = 1;
    }
    if (!code && !UpdateProcThreadAttribute(start.lpAttributeList, 0, PROC_THREAD_ATTRIBUTE_HANDLE_LIST, handles, sizeof(handles), NULL, NULL)) code = winerr();
    start.StartupInfo.hStdInput = handles[0]; start.StartupInfo.hStdOutput = handles[1]; start.StartupInfo.hStdError = handles[2];
    if (!code && !CreateProcessW(exe, command, NULL, NULL, TRUE,
        EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT, inherit_env ? NULL : environment,
        cwd_raw, &start.StartupInfo, &info)) code = winerr();
    if (attributes_live) DeleteProcThreadAttributeList(start.lpAttributeList);
    for (int i = 0; i < 3; ++i) if (handles[i] != INVALID_HANDLE_VALUE) CloseHandle(handles[i]);
    if (code) { for (int i = 0; i < 3; ++i) close_stream(parents + i); return code; }
    CloseHandle(info.hThread);
    *result = (dodo_child){(uintptr_t)info.hProcess, parents[0], parents[1], parents[2], 0, 0, 0};
    return 0;
}
static uint64_t now_ms(void) { return GetTickCount64(); }
static void pause_ms(void) { Sleep(1); }
static int32_t try_wait(dodo_child *child) {
    if (child->done) return 0;
    DWORD status = WaitForSingleObject((HANDLE)child->process, 0);
    if (status == WAIT_TIMEOUT) return DODO_PENDING;
    if (status != WAIT_OBJECT_0) return winerr();
    DWORD code;
    if (!GetExitCodeProcess((HANDLE)child->process, &code)) return winerr();
    child->done = 1; child->kind = 0; child->code = (int32_t)code;
    return 0;
}
static int32_t terminate_child(dodo_child *child, int32_t code) {
    if (child->done) return 0;
    return TerminateProcess((HANDLE)child->process, (UINT)code) ? 0 : winerr();
}
static int32_t read_ready(intptr_t stream, unsigned char *data, size_t capacity, size_t *count, int *eof) {
    DWORD available = 0, actual = 0;
    if (!PeekNamedPipe((HANDLE)stream, NULL, 0, NULL, &available, NULL)) {
        if (GetLastError() == ERROR_BROKEN_PIPE) { *eof = 1; return 0; }
        return winerr();
    }
    if (!available) return 0;
    if (capacity > available) capacity = available;
    if (capacity > 65536) capacity = 65536;
    if (!ReadFile((HANDLE)stream, data, (DWORD)capacity, &actual, NULL)) {
        if (GetLastError() == ERROR_BROKEN_PIPE) { *eof = 1; return 0; }
        return winerr();
    }
    *count = actual; return 0;
}
#else
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <pthread.h>
#include <signal.h>
#include <spawn.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>
extern char **environ;
static void close_stream(intptr_t *stream) {
    if (*stream != -1) { close((int)*stream); *stream = -1; }
}
static uint64_t now_ms(void) {
    struct timespec value; clock_gettime(CLOCK_MONOTONIC, &value);
    return (uint64_t)value.tv_sec * 1000 + (uint64_t)value.tv_nsec / 1000000;
}
static void pause_ms(void) { struct timespec delay = {0, 1000000}; nanosleep(&delay, NULL); }
int32_t dodo_process_spawn(const void *exe, const void *args_raw, size_t arg_units,
    const void *env_raw, size_t env_units, int32_t inherit_env, const void *cwd,
    int32_t input, int32_t output, int32_t error, int32_t search_path, dodo_child *result) {
    const char *args = args_raw, *env = env_raw;
    /* Explicit deterministic argument-count bound; no managed allocation. */
    char *argv[4097], *envp[4097]; size_t argc = 1, envc = 0;
    argv[0] = (char *)exe;
    for (size_t i = 0; i < arg_units;) {
        if (argc == 4096) return DODO_LIMIT;
        argv[argc++] = (char *)args + i;
        while (i < arg_units && args[i]) ++i;
        ++i;
    }
    argv[argc] = NULL;
    for (size_t i = 0; !inherit_env && i < env_units;) {
        if (envc == 4096) return DODO_LIMIT;
        envp[envc++] = (char *)env + i;
        while (i < env_units && env[i]) ++i;
        ++i;
    }
    envp[envc] = NULL;
    posix_spawn_file_actions_t actions;
    int code = posix_spawn_file_actions_init(&actions);
    if (code) return code;
    int modes[3] = {input, output, error};
    intptr_t parents[3] = {-1, -1, -1}, children[3] = {-1, -1, -1};
    for (int i = 0; !code && i < 3; ++i) {
        if (modes[i] == 0) continue;
        if (modes[i] == 2) {
            code = posix_spawn_file_actions_addopen(&actions, i, "/dev/null", i ? O_WRONLY : O_RDONLY, 0);
        } else {
            int pipefd[2];
            if (pipe2(pipefd, O_CLOEXEC)) { code = errno; break; }
            /* Move descriptors away from standard-stream numbers. */
            for (int j = 0; j < 2; ++j) if (pipefd[j] < 3) {
                int moved = fcntl(pipefd[j], F_DUPFD_CLOEXEC, 3);
                if (moved < 0) { code = errno; break; }
                close(pipefd[j]); pipefd[j] = moved;
            }
            if (code) { close(pipefd[0]); close(pipefd[1]); break; }
            parents[i] = pipefd[i ? 0 : 1]; children[i] = pipefd[i ? 1 : 0];
            if (i && fcntl((int)parents[i], F_SETFL, O_NONBLOCK)) { code = errno; break; }
            code = posix_spawn_file_actions_adddup2(&actions, (int)children[i], i);
        }
    }
    if (!code && cwd) code = posix_spawn_file_actions_addchdir_np(&actions, cwd);
    /* Linux/glibc closefrom action closes unrelated parent descriptors too. */
    if (!code) code = posix_spawn_file_actions_addclosefrom_np(&actions, 3);
    pid_t pid = 0;
    if (!code) {
        code = search_path ? posix_spawnp(&pid, exe, &actions, NULL, argv, inherit_env ? environ : envp)
                           : posix_spawn(&pid, exe, &actions, NULL, argv, inherit_env ? environ : envp);
    }
    posix_spawn_file_actions_destroy(&actions);
    for (int i = 0; i < 3; ++i) close_stream(children + i);
    if (code) { for (int i = 0; i < 3; ++i) close_stream(parents + i); return code; }
    *result = (dodo_child){(uintptr_t)pid, parents[0], parents[1], parents[2], 0, 0, 0};
    return 0;
}
static int32_t try_wait(dodo_child *child) {
    if (child->done) return 0;
    int status;
    pid_t result = waitpid((pid_t)child->process, &status, WNOHANG);
    if (!result) return DODO_PENDING;
    if (result < 0) return errno == EINTR ? DODO_PENDING : errno;
    child->done = 1;
    child->kind = WIFSIGNALED(status) ? 1 : 0;
    child->code = WIFSIGNALED(status) ? WTERMSIG(status) : WEXITSTATUS(status);
    return 0;
}
static int32_t terminate_child(dodo_child *child, int32_t code) {
    if (child->done) return 0;
    return kill((pid_t)child->process, code) ? errno : 0;
}
static int32_t read_ready(intptr_t stream, unsigned char *data, size_t capacity, size_t *count, int *eof) {
    if (capacity > 65536) capacity = 65536;
    ssize_t result = read((int)stream, data, capacity);
    if (result < 0) return errno == EAGAIN || errno == EWOULDBLOCK || errno == EINTR ? 0 : errno;
    *count = (size_t)result; *eof = !result; return 0;
}
#endif

int32_t dodo_process_wait(dodo_child *child, uint64_t timeout_ms) {
    uint64_t start = now_ms();
    for (;;) {
        int32_t code = try_wait(child);
        if (code != DODO_PENDING) return code;
        if (timeout_ms != UINT64_MAX && now_ms() - start >= timeout_ms) return DODO_TIMEOUT;
        pause_ms();
    }
}
int32_t dodo_process_terminate(dodo_child *child) {
#ifdef _WIN32
    return terminate_child(child, 1);
#else
    return terminate_child(child, SIGTERM);
#endif
}
static int32_t kill_and_wait(dodo_child *child) {
#ifdef _WIN32
    int32_t code = terminate_child(child, 1);
#else
    int32_t code = terminate_child(child, SIGKILL);
#endif
    if (code) { int32_t waited = try_wait(child); if (waited) return code; }
    return dodo_process_wait(child, UINT64_MAX);
}
int32_t dodo_process_cancel(dodo_child *child) {
    if (child->done) return 0;
    int32_t code = kill_and_wait(child);
    if (!code) { child->kind = 2; child->code = 0; }
    return code;
}
void dodo_process_drop(dodo_child *child) {
    close_stream(&child->input); close_stream(&child->output); close_stream(&child->error);
    if (child->process) {
        if (!child->done) (void)kill_and_wait(child);
#ifdef _WIN32
        CloseHandle((HANDLE)child->process);
#endif
        child->process = 0;
    }
}
int32_t dodo_process_collect(dodo_child *child, unsigned char *output, size_t out_capacity,
    unsigned char *error, size_t err_capacity, uint64_t timeout_ms, size_t *out_len, size_t *err_len) {
    *out_len = 0; *err_len = 0;
    /* Collection delivers EOF on stdin before draining the output streams. */
    close_stream(&child->input);
    uint64_t start = now_ms();
    int32_t code = 0;
    while (child->output != -1 || child->error != -1 || !child->done) {
        for (int i = 0; i < 2 && !code; ++i) {
            intptr_t *stream = i ? &child->error : &child->output;
            if (*stream == -1) continue;
            unsigned char *data = i ? error : output;
            size_t capacity = i ? err_capacity : out_capacity;
            size_t *used = i ? err_len : out_len;
            unsigned char overflow_byte; size_t count = 0; int eof = 0;
            size_t left = capacity - *used;
            code = read_ready(*stream, left ? data + *used : &overflow_byte, left ? left : 1, &count, &eof);
            if (!code && !left && count) code = DODO_LIMIT;
            else *used += count;
            if (eof) close_stream(stream);
        }
        if (code) break;
        code = try_wait(child);
        if (code == DODO_PENDING) code = 0;
        if (code) break;
        if (child->output == -1 && child->error == -1 && child->done) break;
        if (timeout_ms != UINT64_MAX && now_ms() - start >= timeout_ms) { code = DODO_TIMEOUT; break; }
        pause_ms();
    }
    if (code) { close_stream(&child->output); close_stream(&child->error); (void)kill_and_wait(child); }
    return code;
}

/* Block SIGPIPE only in the writing thread; consume only a newly generated
 * signal. An ordinary closed child pipe is a recoverable EPIPE error. */
int32_t dodo_process_pipe_write(intptr_t handle, const unsigned char *data, size_t count, size_t *written) {
    *written = 0;
#ifdef _WIN32
    if (count > UINT32_MAX) count = UINT32_MAX;
    DWORD actual = 0;
    if (!WriteFile((HANDLE)handle, data, (DWORD)count, &actual, NULL)) return winerr();
    *written = actual; return 0;
#else
    sigset_t set, old, pending;
    sigemptyset(&set); sigaddset(&set, SIGPIPE);
    int code = pthread_sigmask(SIG_BLOCK, &set, &old);
    if (code) return code;
    sigpending(&pending); int was_pending = sigismember(&pending, SIGPIPE);
    ssize_t actual = write((int)handle, data, count);
    int saved = actual < 0 ? errno : 0;
    if (saved == EPIPE && !was_pending) {
        struct timespec zero = {0, 0};
        while (sigtimedwait(&set, NULL, &zero) < 0 && errno == EINTR) {}
    }
    pthread_sigmask(SIG_SETMASK, &old, NULL);
    if (actual >= 0) *written = (size_t)actual;
    return saved;
#endif
}
