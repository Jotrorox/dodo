/* Hosted support linked only into dodo test executables. */
#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#ifdef _WIN32
#include <windows.h>
#else
#include <sys/resource.h>
#endif

int32_t dodo_test_select(int32_t argc, char **argv) {
    /* A deliberate assertion/trap must not leave a core dump or open a dialog. */
#ifdef _WIN32
    SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX);
#else
    struct rlimit limit = {0, 0};
    (void)setrlimit(RLIMIT_CORE, &limit);
#endif
    setvbuf(stdout, NULL, _IONBF, 0);
    setvbuf(stderr, NULL, _IONBF, 0);
    if (argc != 2) return -1;
    char *end;
    long index = strtol(argv[1], &end, 10);
    if (*end || index < 0 || index > INT32_MAX) return -1;
    return (int32_t)index;
}

void dodo_test_failure(const char *message, size_t length) {
    fwrite(message, 1, length, stderr);
    fputc('\n', stderr);
}
static const char *label(int32_t which) {
    return which == 0 ? "  left: " : which == 1 ? " right: " : "message: ";
}
void dodo_test_signed(int32_t which, int64_t value) {
    fprintf(stderr, "%s%" PRId64 "\n", label(which), value);
}
void dodo_test_unsigned(int32_t which, uint64_t value) {
    fprintf(stderr, "%s%" PRIu64 "\n", label(which), value);
}
void dodo_test_float(int32_t which, double value) {
    fprintf(stderr, "%s%.17g\n", label(which), value);
}
void dodo_test_bool(int32_t which, int32_t value) {
    fprintf(stderr, "%s%s\n", label(which), value ? "true" : "false");
}
void dodo_test_text(int32_t which, const unsigned char *value, size_t length) {
    fprintf(stderr, "%s\"", label(which));
    for (size_t i = 0; i < length; ++i) {
        unsigned char c = value[i];
        if (c == '\n') fputs("\\n", stderr);
        else if (c == '\r') fputs("\\r", stderr);
        else if (c == '\t') fputs("\\t", stderr);
        else if (c == '\\' || c == '"') fprintf(stderr, "\\%c", c);
        else if (c < 32 || c == 127) fprintf(stderr, "\\x%02x", c);
        else fputc(c, stderr);
    }
    fputs("\"\n", stderr);
}
