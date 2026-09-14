#include <stdint.h>
int32_t dodo_time_monotonic(uint64_t *seconds, uint32_t *nanos) { (void)seconds; (void)nanos; return 5; }
int32_t dodo_time_wall(int64_t *seconds, uint32_t *nanos) { (void)seconds; (void)nanos; return 5; }
extern void dodo_main(void) __asm__("dodo.clock_failure.main");
int main(void) { dodo_main(); return 0; }
