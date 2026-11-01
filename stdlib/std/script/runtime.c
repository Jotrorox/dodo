/* Host boundary of the script runtimes (std/script/lua): the parts of the C
 * library whose names, constants or layouts differ between C libraries.
 * Compiled against the target's own headers, so the Dodo side only sees
 * portable signatures. Functions with identical names and ABIs everywhere
 * (fopen, fread, getc, strftime, remove, ...) are declared directly in Dodo. */
#if !defined(_WIN32)
#ifndef _POSIX_C_SOURCE
#define _POSIX_C_SOURCE 200809L
#endif
#endif
#include <errno.h>
#include <locale.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#if defined(_WIN32)
#include <io.h>
#else
#include <sys/wait.h>
#include <unistd.h>
#endif

/* The Dodo side keeps a struct tm in an opaque, 8-byte aligned 128-byte
 * block and reads/writes the standard fields through the accessors below. */
_Static_assert(sizeof(struct tm) <= 128, "struct tm must fit the Dodo block");

int32_t dodo_script_errno(void) { return errno; }
void dodo_script_clear_errno(void) { errno = 0; }

/* The C library's own standard streams (so output written through them is
 * ordered with any other C code, and stderr stays unbuffered). */
FILE *dodo_script_std_stream(int32_t which) {
    if (which == 0) return stdin;
    if (which == 1) return stdout;
    return stderr;
}

/* 64-bit file positions. */
int32_t dodo_script_seek(FILE *f, int64_t offset, int32_t whence) {
    int origin = whence == 0 ? SEEK_SET : whence == 1 ? SEEK_CUR : SEEK_END;
#if defined(_WIN32)
    return _fseeki64(f, offset, origin);
#else
    if ((int64_t)(off_t)offset != offset) { errno = EOVERFLOW; return -1; }
    return fseeko(f, (off_t)offset, origin);
#endif
}
int64_t dodo_script_tell(FILE *f) {
#if defined(_WIN32)
    return _ftelli64(f);
#else
    return (int64_t)ftello(f);
#endif
}

/* Read bytes up to (not including) a newline into out[0..capacity) and
 * return how many were stored. *last is '\n' or EOF when the line ended, or
 * 0 when out filled up first. The stream stays locked for the whole run. */
size_t dodo_script_read_line(FILE *f, char *out, size_t capacity, int32_t *last) {
    size_t n = 0;
    int c = 0;
#if defined(_WIN32)
    while (n < capacity && (c = getc(f)) != EOF && c != '\n') out[n++] = (char)c;
#else
    flockfile(f);
    while (n < capacity && (c = getc_unlocked(f)) != EOF && c != '\n') out[n++] = (char)c;
    funlockfile(f);
#endif
    *last = n == capacity ? 0 : c;
    return n;
}

/* mode: 0 unbuffered, 1 fully buffered, 2 line buffered. */
int32_t dodo_script_setvbuf(FILE *f, int32_t mode, size_t size) {
    int m = mode == 0 ? _IONBF : mode == 1 ? _IOFBF : _IOLBF;
    return setvbuf(f, NULL, m, size);
}

/* Child processes. Every pending output is flushed before the shell starts
 * so the child's output follows ours. */
FILE *dodo_script_popen(const char *command, const char *mode) {
    fflush(NULL);
#if defined(_WIN32)
    return _popen(command, mode);
#else
    return popen(command, mode);
#endif
}

/* Turn a wait status into (exited, code). On Windows the status is the exit
 * code itself. */
static void script_decode_status(int status, int32_t *exited, int32_t *code) {
#if defined(_WIN32)
    *exited = 1; *code = status;
#else
    if (WIFEXITED(status)) { *exited = 1; *code = WEXITSTATUS(status); }
    else if (WIFSIGNALED(status)) { *exited = 0; *code = WTERMSIG(status); }
    else { *exited = 1; *code = status; }
#endif
}

/* 1: the process finished and (exited, code) are set; 0: the operation
 * failed and errno describes why. */
int32_t dodo_script_pclose(FILE *f, int32_t *exited, int32_t *code) {
    errno = 0;
#if defined(_WIN32)
    int status = _pclose(f);
#else
    int status = pclose(f);
#endif
    if (status != 0 && errno != 0) return 0;
    script_decode_status(status, exited, code);
    return 1;
}
int32_t dodo_script_system(const char *command, int32_t *exited, int32_t *code) {
    errno = 0;
    int status = system(command);
    if (status != 0 && errno != 0) return 0;
    script_decode_status(status, exited, code);
    return 1;
}
/* Whether a command processor is available. */
int32_t dodo_script_has_shell(void) { return system(NULL) != 0; }

/* Calendar time. */
int64_t dodo_script_time(void) {
#if defined(_WIN32)
    return (int64_t)_time64(NULL);
#else
    return (int64_t)time(NULL);
#endif
}
double dodo_script_clock(void) {
    return (double)clock() / (double)CLOCKS_PER_SEC;
}

/* Fill *out with the broken-down local (utc == 0) or UTC time of t;
 * 0 when the time cannot be represented. */
int32_t dodo_script_broken_down(int64_t t, int32_t utc, struct tm *out) {
#if defined(_WIN32)
    __time64_t v = (__time64_t)t;
    return (utc ? _gmtime64_s(out, &v) : _localtime64_s(out, &v)) == 0;
#else
    time_t v = (time_t)t;
    if ((int64_t)v != t) return 0;
    return (utc ? gmtime_r(&v, out) : localtime_r(&v, out)) != NULL;
#endif
}

/* fields: sec, min, hour, mday, mon, year, wday, yday, isdst. */
void dodo_script_tm_get(const struct tm *tm, int32_t *fields) {
    fields[0] = tm->tm_sec; fields[1] = tm->tm_min; fields[2] = tm->tm_hour;
    fields[3] = tm->tm_mday; fields[4] = tm->tm_mon; fields[5] = tm->tm_year;
    fields[6] = tm->tm_wday; fields[7] = tm->tm_yday; fields[8] = tm->tm_isdst;
}
void dodo_script_tm_set(struct tm *tm, const int32_t *fields) {
    memset(tm, 0, sizeof *tm);
    tm->tm_sec = fields[0]; tm->tm_min = fields[1]; tm->tm_hour = fields[2];
    tm->tm_mday = fields[3]; tm->tm_mon = fields[4]; tm->tm_year = fields[5];
    tm->tm_wday = fields[6]; tm->tm_yday = fields[7]; tm->tm_isdst = fields[8];
}

/* mktime normalizes *tm in place; *ok is 0 when the result is (time_t)-1. */
int64_t dodo_script_mktime(struct tm *tm, int32_t *ok) {
#if defined(_WIN32)
    __time64_t t = _mktime64(tm);
#else
    time_t t = mktime(tm);
#endif
    *ok = t != (time_t)-1;
    return (int64_t)t;
}

size_t dodo_script_strftime(char *out, size_t size, const char *format, const struct tm *tm) {
    return strftime(out, size, format, tm);
}

/* getenv under a private name (several Lua libraries read the environment). */
const char *dodo_script_getenv(const char *name) { return getenv(name); }

/* category: 0 all, 1 collate, 2 ctype, 3 monetary, 4 numeric, 5 time. */
const char *dodo_script_setlocale(int32_t category, const char *locale) {
    static const int categories[] = {LC_ALL, LC_COLLATE, LC_CTYPE, LC_MONETARY, LC_NUMERIC, LC_TIME};
    if (category < 0 || category > 5) return NULL;
    return setlocale(categories[category], locale);
}

/* A fresh temporary file name in out (capacity >= 32); 1 on success. POSIX
 * systems create the file (mkstemp) so the name cannot be taken by others. */
int32_t dodo_script_tmpname(char *out, size_t capacity) {
#if defined(_WIN32)
    if (capacity < L_tmpnam) return 0;
    return tmpnam(out) != NULL;
#else
    static const char pattern[] = "/tmp/lua_XXXXXX";
    if (capacity < sizeof pattern) return 0;
    memcpy(out, pattern, sizeof pattern);
    int fd = mkstemp(out);
    if (fd == -1) return 0;
    close(fd);
    return 1;
#endif
}
