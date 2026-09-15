/* Controlled child for std/process; native argv and environment are checked. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#ifdef _WIN32
#include <windows.h>
#else
#include <signal.h>
#include <time.h>
#include <unistd.h>
#endif
int main(int argc, char **argv) {
    if (argc < 2) return 90;
#ifdef _WIN32
    if (!strcmp(argv[1], "signal-handle")) {
        if (argc != 3) return 104;
        /* The parent observes its event, so an unrelated handle that happens
         * to reuse this number in the child cannot cause a false failure. */
        /* Parse locally so the Wine harness still links against legacy msvcrt
         * without adding a C99 conversion helper from MinGW's runtime. */
        uintptr_t value = 0;
        for (const char *digit = argv[2]; *digit; ++digit) {
            if (*digit < '0' || *digit > '9') return 105;
            uintptr_t part = (uintptr_t)(*digit - '0');
            if (value > (UINTPTR_MAX - part) / 10) return 105;
            value = value * 10 + part;
        }
        if (!value) return 105;
        (void)SetEvent((HANDLE)value);
        return 0;
    }
#endif
    if (!strcmp(argv[1], "arguments")) {
        for (int i = 2; i < argc; ++i) {
            size_t count = strlen(argv[i]) + 1;
            if (fwrite(argv[i], 1, count, stdout) != count) return 101;
        }
        fputs("done", stderr); return 0;
    }
    if (!strcmp(argv[1], "cleanup")) {
        FILE *marker = fopen("child-pid", "wb");
        if (!marker) return 102;
#ifdef _WIN32
        fprintf(marker, "%lu", (unsigned long)GetCurrentProcessId());
#else
        fprintf(marker, "%lu", (unsigned long)getpid());
#endif
        if (fclose(marker)) return 103;
        fputs("ready", stdout); fflush(stdout);
#ifdef _WIN32
        Sleep(10000);
#else
        struct timespec delay = {10, 0}; nanosleep(&delay, NULL);
#endif
        return 0;
    }
    if (!strcmp(argv[1], "flood")) {
        char data[4096]; memset(data, 'o', sizeof(data));
        char error[4096]; memset(error, 'e', sizeof(error));
        for (int i = 0; i < 64; ++i) {
            if (fwrite(data, 1, sizeof(data), stdout) != sizeof(data)) return 91;
            if (fwrite(error, 1, sizeof(error), stderr) != sizeof(error)) return 92;
        }
        return 7;
    }
    if (!strcmp(argv[1], "check")) {
        if (argc != 6 || strcmp(argv[2], "space arg") || strcmp(argv[3], "quote\"arg") || strcmp(argv[4], "tail\\") || strcmp(argv[5], "")) return 93;
        const char *value = getenv("DODO_CHILD");
        if (!value || strcmp(value, "native value")) return 94;
        if (getenv("DODO_PARENT_ONLY")) return 95;
        FILE *marker = fopen("cwd-marker", "rb");
        if (!marker) return 96;
        fclose(marker);
        fputs("quoted", stdout); fputs("environment", stderr); return 0;
    }
    if (!strcmp(argv[1], "inherit")) {
        const char *value = getenv("DODO_PARENT_ONLY");
        return value && !strcmp(value, "must not leak") ? 0 : 99;
    }
#ifndef _WIN32
    if (!strcmp(argv[1], "native")) {
        return argc == 3 && (unsigned char)argv[2][0] == 255 && argv[2][1] == 0 ? 0 : 100;
    }
#endif
    if (!strcmp(argv[1], "sleep")) {
#ifdef _WIN32
        Sleep(10000);
#else
        struct timespec delay = {10, 0}; nanosleep(&delay, NULL);
#endif
        return 0;
    }
    if (!strcmp(argv[1], "echo")) {
        char data[128]; size_t count;
        while ((count = fread(data, 1, sizeof(data), stdin))) {
            if (fwrite(data, 1, count, stdout) != count) return 97;
        }
        return 0;
    }
    return 98;
}
