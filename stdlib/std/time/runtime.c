/* Hosted clocks use native headers; the portable time family has no C bridge. */
#ifndef _POSIX_C_SOURCE
#define _POSIX_C_SOURCE 200809L
#endif
#include <stdint.h>
#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
int32_t dodo_time_monotonic(uint64_t *seconds, uint32_t *nanos) {
    LARGE_INTEGER counter, frequency;
    if (!QueryPerformanceFrequency(&frequency) || !QueryPerformanceCounter(&counter)) {
        DWORD code = GetLastError();
        return (int32_t)(code ? code : ERROR_GEN_FAILURE);
    }
    if (counter.QuadPart < 0 || frequency.QuadPart <= 0) return ERROR_INVALID_DATA;
    uint64_t ticks = (uint64_t)counter.QuadPart, rate = (uint64_t)frequency.QuadPart;
    *seconds = ticks / rate;
    /* Binary long division of remainder * 1e9 avoids multiplication overflow.
       rate <= INT64_MAX, so doubling a remainder is always representable. */
    uint64_t remainder = 0, quotient = 0, fraction = ticks % rate;
    for (int bit = 29; bit >= 0; --bit) {
        remainder *= 2; quotient *= 2;
        if (remainder >= rate) { remainder -= rate; ++quotient; }
        if ((UINT64_C(1000000000) >> bit) & 1) {
            remainder += fraction;
            if (remainder >= rate) { remainder -= rate; ++quotient; }
        }
    }
    *nanos = (uint32_t)quotient; return 0;
}
int32_t dodo_time_wall(int64_t *seconds, uint32_t *nanos) {
    FILETIME value; GetSystemTimePreciseAsFileTime(&value);
    uint64_t ticks = ((uint64_t)value.dwHighDateTime << 32) | value.dwLowDateTime;
    *seconds = (int64_t)(ticks / UINT64_C(10000000)) - INT64_C(11644473600);
    *nanos = (uint32_t)((ticks % UINT64_C(10000000)) * 100); return 0;
}
#else
#include <errno.h>
#include <time.h>
int32_t dodo_time_monotonic(uint64_t *seconds, uint32_t *nanos) {
    struct timespec value;
    if (clock_gettime(CLOCK_MONOTONIC, &value)) return errno;
    if (value.tv_sec < 0 || value.tv_nsec < 0 || value.tv_nsec >= 1000000000) return EOVERFLOW;
    *seconds = (uint64_t)value.tv_sec; *nanos = (uint32_t)value.tv_nsec; return 0;
}
int32_t dodo_time_wall(int64_t *seconds, uint32_t *nanos) {
    struct timespec value;
    if (clock_gettime(CLOCK_REALTIME, &value)) return errno;
    if (value.tv_nsec < 0 || value.tv_nsec >= 1000000000) return EOVERFLOW;
    *seconds = (int64_t)value.tv_sec; *nanos = (uint32_t)value.tv_nsec; return 0;
}
#endif
