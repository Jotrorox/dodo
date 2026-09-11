/* Controlled child for std/process; native argv and environment are checked. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#ifdef _WIN32
#include <windows.h>
#else
#include <signal.h>
#include <time.h>
#include <unistd.h>
#endif
int main(int argc, char **argv) {
    if (argc < 2) return 90;
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
