/* Optional rooted static files, independently selected from routing and HTTP.
 * No borrowed pointer survives a call. No heap allocation. */
#ifndef _WIN32
#define _GNU_SOURCE
#endif
#include <stdint.h>
#include <stddef.h>
#include <string.h>
#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <wchar.h>
#include <winternl.h>
static int wide_path(const unsigned char *path, size_t length, wchar_t *out, int capacity) {
    if (!length || length > 32766 || memchr(path, 0, length)) {
        SetLastError(ERROR_INVALID_NAME); return 0;
    }
    int count = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, (const char *)path,
                                  (int)length, out, capacity - 1);
    if (!count) return 0;
    out[count] = 0;
    return count;
}
static int regular(HANDLE handle, int directory) {
    BY_HANDLE_FILE_INFORMATION info;
    if (!GetFileInformationByHandle(handle, &info)) return 0;
    if ((info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT) ||
        !!(info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) != directory ||
        GetFileType(handle) != FILE_TYPE_DISK) {
        SetLastError(ERROR_ACCESS_DENIED); return 0;
    }
    return 1;
}
intptr_t dodo_web_root(const unsigned char *path, size_t length, int *code) {
    wchar_t name[32768];
    if (!wide_path(path, length, name, 32768)) { *code = (int)GetLastError(); return -1; }
    HANDLE handle = CreateFileW(name, GENERIC_READ, FILE_SHARE_READ, NULL,
        OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT, NULL);
    if (handle == INVALID_HANDLE_VALUE) { *code = (int)GetLastError(); return -1; }
    if (!regular(handle, 1)) { *code = (int)GetLastError(); CloseHandle(handle); return -1; }
    return (intptr_t)handle;
}
/* Lookup one component relative to a retained directory capability. Never
 * reconstruct an absolute name: an ancestor rename must not redirect lookup.
 * Denying write sharing for directories also prevents in-place junction changes
 * while the directory capability is retained. */
typedef NTSTATUS (NTAPI *dodo_nt_create_file)(PHANDLE, ACCESS_MASK,
    POBJECT_ATTRIBUTES, PIO_STATUS_BLOCK, PLARGE_INTEGER, ULONG, ULONG, ULONG,
    ULONG, PVOID, ULONG);
typedef ULONG (NTAPI *dodo_nt_error)(NTSTATUS);
intptr_t dodo_web_file(intptr_t root, const unsigned char *path, size_t length, int *code) {
    wchar_t relative[32768];
    int count = wide_path(path, length, relative, 32768);
    if (!count) { *code = (int)GetLastError(); return -1; }
    HMODULE ntdll = GetModuleHandleW(L"ntdll.dll");
    dodo_nt_create_file create = ntdll ? (dodo_nt_create_file)(void *)GetProcAddress(ntdll, "NtCreateFile") : NULL;
    dodo_nt_error map_error = ntdll ? (dodo_nt_error)(void *)GetProcAddress(ntdll, "RtlNtStatusToDosError") : NULL;
    if (!create || !map_error) { *code = ERROR_NOT_SUPPORTED; return -1; }
    HANDLE directory = (HANDLE)root;
    int owned_directory = 0;
    int start = 0;
    for (int i = 0; i <= count; ++i) {
        if (relative[i] != L'/' && relative[i] != 0) continue;
        int final = i == count;
        int units = i - start;
        if (!units) { *code = ERROR_INVALID_NAME; goto failed; }
        UNICODE_STRING name;
        name.Buffer = relative + start;
        name.Length = (USHORT)((unsigned)units * sizeof(wchar_t));
        name.MaximumLength = name.Length;
        OBJECT_ATTRIBUTES attributes;
        memset(&attributes, 0, sizeof(attributes));
        attributes.Length = sizeof(attributes);
        attributes.RootDirectory = directory;
        attributes.ObjectName = &name;
        attributes.Attributes = 0x40 | 0x1000; /* CASE_INSENSITIVE | DONT_REPARSE */
        IO_STATUS_BLOCK status;
        HANDLE next = INVALID_HANDLE_VALUE;
        NTSTATUS result = create(&next, GENERIC_READ | SYNCHRONIZE, &attributes,
            &status, NULL, FILE_ATTRIBUTE_NORMAL,
            final ? FILE_SHARE_READ | FILE_SHARE_WRITE : FILE_SHARE_READ,
            1, /* FILE_OPEN */
            0x00200000 | 0x20 | (final ? 0x40 : 1),
            /* OPEN_REPARSE_POINT | SYNCHRONOUS_IO_NONALERT | type */
            NULL, 0);
        if (result < 0) { *code = (int)map_error(result); goto failed; }
        if (!regular(next, !final)) { *code = (int)GetLastError(); CloseHandle(next); goto failed; }
        if (owned_directory) CloseHandle(directory);
        if (final) return (intptr_t)next;
        directory = next;
        owned_directory = 1;
        start = i + 1;
    }
    *code = ERROR_INVALID_NAME;
failed:
    if (owned_directory) CloseHandle(directory);
    return -1;
}
void dodo_web_close_root(intptr_t root) { CloseHandle((HANDLE)root); }
#else
#include <unistd.h>
#include <fcntl.h>
#include <sys/stat.h>
#include <errno.h>
static int local_path(const unsigned char *path, size_t length, char *out) {
    if (!length || length >= 4096 || memchr(path, 0, length)) { errno = EINVAL; return 0; }
    memcpy(out, path, length); out[length] = 0; return 1;
}
intptr_t dodo_web_root(const unsigned char *path, size_t length, int *code) {
    char name[4096];
    if (!local_path(path, length, name)) { *code = errno; return -1; }
    int fd = open(name, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC);
    if (fd < 0) *code = errno;
    return fd;
}
intptr_t dodo_web_file(intptr_t root, const unsigned char *path, size_t length, int *code) {
    char name[4096];
    if (!local_path(path, length, name)) { *code = errno; return -1; }
    int directory = fcntl((int)root, F_DUPFD_CLOEXEC, 0);
    if (directory < 0) { *code = errno; return -1; }
    char *segment = name;
    for (size_t i = 0; i <= length; ++i) {
        if (name[i] != '/' && name[i]) continue;
        int final = !name[i]; name[i] = 0;
        int next = openat(directory, segment, O_RDONLY | O_NOFOLLOW | O_CLOEXEC |
                          (final ? O_NONBLOCK : O_DIRECTORY));
        int saved = errno;
        close(directory);
        if (next < 0) { *code = saved; return -1; }
        directory = next;
        if (final) {
            struct stat info;
            if (fstat(next, &info) < 0) { *code = errno; close(next); return -1; }
            if (!S_ISREG(info.st_mode)) { *code = EACCES; close(next); return -1; }
            return next;
        }
        segment = name + i + 1;
    }
    close(directory); *code = EINVAL; return -1;
}
void dodo_web_close_root(intptr_t root) { close((int)root); }
#endif
