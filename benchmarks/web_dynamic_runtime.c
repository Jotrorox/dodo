/* Only process I/O and timer boundaries; routing runs entirely in Dodo. */
#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#ifdef _WIN32
#include <fcntl.h>
#include <io.h>
#endif

/* Linked from the existing std/time C runtime, independently of hosted imports. */
int32_t dodo_time_monotonic(uint64_t *seconds, uint32_t *nanos);

int32_t dodo_bench_header(uint8_t *output) {
#ifdef _WIN32
    if (_setmode(_fileno(stdin), _O_BINARY) == -1) return 1;
#endif
    return fread(output, 1, 16, stdin) == 16 ? 0 : 1;
}

uint64_t dodo_bench_nanos(void) {
    uint64_t seconds;
    uint32_t nanos;
    if (dodo_time_monotonic(&seconds, &nanos)) {
        fputs("benchmark monotonic clock failed\n", stderr);
        exit(1);
    }
    return seconds * UINT64_C(1000000000) + nanos;
}

void dodo_bench_output(uint64_t nanos, uint64_t checksum) {
    printf("%" PRIu64 " %" PRIu64 "\n", nanos, checksum);
}
