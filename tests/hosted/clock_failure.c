#include <stdint.h>
int32_t dodo_time_monotonic(uint64_t *seconds, uint32_t *nanos) { (void)seconds; (void)nanos; return 5; }
int32_t dodo_time_wall(int64_t *seconds, uint32_t *nanos) { (void)seconds; (void)nanos; return 5; }
/* Mach-O prefixes C symbol names with an underscore; asm labels bypass that. */
#define DODO_STRINGIFY(x) #x
#define DODO_SYMBOL(prefix, name) DODO_STRINGIFY(prefix) name
extern void dodo_main(void) __asm__(DODO_SYMBOL(__USER_LABEL_PREFIX__, "dodo.clock_failure.main"));
int main(void) { dodo_main(); return 0; }
