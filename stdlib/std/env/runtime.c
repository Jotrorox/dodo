/* Process observations copy into caller-owned native-unit storage. No pointers
 * into process-global environment storage escape this boundary. */
#ifndef _GNU_SOURCE
#define _GNU_SOURCE
#endif
#include <stdint.h>
#include <stddef.h>
#include <string.h>
#include <limits.h>
#define DODO_INVALID (-1)
#define DODO_LIMIT (-3)
static int dodo_argc;
static char **dodo_argv;
void dodo_env_init_args(int32_t argc, char **argv) { dodo_argc = argc; dodo_argv = argv; }
#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
int32_t dodo_env_capture(void *storage, size_t capacity, size_t *used) {
    wchar_t *source = GetEnvironmentStringsW();
    if (!source) return (int32_t)GetLastError();
    size_t count = 0;
    while (source[count]) { while (source[count]) ++count; ++count; }
    *used = count;
    if (count <= capacity && count) memcpy(storage, source, count * sizeof(wchar_t));
    FreeEnvironmentStringsW(source);
    return count <= capacity ? 0 : DODO_LIMIT;
}
int32_t dodo_env_current_dir(void *storage, size_t capacity, size_t *used) {
    if (capacity > UINT32_MAX) return DODO_INVALID;
    DWORD result = GetCurrentDirectoryW((DWORD)capacity, storage);
    if (!result) return (int32_t)GetLastError();
    if (result >= capacity) { *used = result; return DODO_LIMIT; }
    *used = (size_t)result + 1; return 0;
}
/* Parse the standard Windows backslash/quote rules directly, retaining native
 * UTF-16 code units and avoiding CommandLineToArgvW's LocalAlloc allocation. */
int32_t dodo_env_arguments(void *raw, size_t capacity, size_t *used) {
    const wchar_t *input = GetCommandLineW(); wchar_t *output = raw;
    size_t count = 0; int argument = 0;
#define EMIT(c) do { if (count < capacity) output[count] = (c); ++count; } while (0)
    while (*input) {
        while (*input == L' ' || *input == L'\t') ++input;
        if (!*input) break;
        int quoted = 0;
        /* argv[0] has separate CRT parsing: quotes group its path, and
         * backslashes are literal even immediately before a quote. */
        if (!argument) {
            while (*input && (quoted || (*input != L' ' && *input != L'\t'))) {
                if (*input == L'"') { quoted = !quoted; ++input; }
                else { EMIT(*input); ++input; }
            }
        } else {
            while (*input && (quoted || (*input != L' ' && *input != L'\t'))) {
                size_t slashes = 0;
                while (*input == L'\\') { ++slashes; ++input; }
                if (*input == L'"') {
                    for (size_t i = 0; i < slashes / 2; ++i) EMIT(L'\\');
                    if (slashes & 1) { EMIT(L'"'); ++input; }
                    else if (quoted && input[1] == L'"') { EMIT(L'"'); input += 2; }
                    else { quoted = !quoted; ++input; }
                } else {
                    for (size_t i = 0; i < slashes; ++i) EMIT(L'\\');
                    if (!*input || (!quoted && (*input == L' ' || *input == L'\t'))) break;
                    EMIT(*input); ++input;
                }
            }
        }
        EMIT(0); ++argument;
    }
#undef EMIT
    *used = count; return count <= capacity ? 0 : DODO_LIMIT;
}
int32_t dodo_env_name_equal(const void *left, size_t left_len, const void *right, size_t right_len) {
    if (left_len > INT_MAX || right_len > INT_MAX) return 0;
    return CompareStringOrdinal(left, (int)left_len, right, (int)right_len, TRUE) == CSTR_EQUAL;
}
#else
#include <errno.h>
#include <unistd.h>
extern char **environ;
int32_t dodo_env_capture(void *storage, size_t capacity, size_t *used) {
    size_t count = 0;
    for (char **entry = environ; *entry; ++entry) {
        size_t length = strlen(*entry) + 1;
        if (length > SIZE_MAX - count) return DODO_LIMIT;
        if (count <= capacity && length <= capacity - count) memcpy((char *)storage + count, *entry, length);
        count += length;
    }
    *used = count; return count <= capacity ? 0 : DODO_LIMIT;
}
int32_t dodo_env_current_dir(void *storage, size_t capacity, size_t *used) {
    if (!capacity) return DODO_LIMIT;
    if (!getcwd(storage, capacity)) return errno == ERANGE ? DODO_LIMIT : errno;
    *used = strlen(storage) + 1; return 0;
}
int32_t dodo_env_arguments(void *storage, size_t capacity, size_t *used) {
    size_t count = 0;
    for (int i = 0; i < dodo_argc; ++i) {
        size_t length = strlen(dodo_argv[i]) + 1;
        if (length > SIZE_MAX - count) return DODO_LIMIT;
        if (count <= capacity && length <= capacity - count) memcpy((char *)storage + count, dodo_argv[i], length);
        count += length;
    }
    *used = count; return count <= capacity ? 0 : DODO_LIMIT;
}
int32_t dodo_env_name_equal(const void *left, size_t left_len, const void *right, size_t right_len) {
    return left_len == right_len && (!left_len || !memcmp(left, right, left_len));
}
#endif
