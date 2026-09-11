/* The open(2) entry point is variadic. Keep the C calling-convention detail
 * here until Dodo supports general foreign variadic declarations. */
#if !defined(_WIN32)
#define _POSIX_C_SOURCE 200809L
#include <fcntl.h>
#include <stdint.h>
#include <stddef.h>
#include <sys/stat.h>
_Static_assert(sizeof(void *) == 8, "Dodo Linux native handles require x86_64");
_Static_assert(sizeof(struct stat) == 144, "Dodo Linux stat layout mismatch");
_Static_assert(offsetof(struct stat, st_mode) == 24, "Dodo Linux stat mode offset mismatch");
_Static_assert(offsetof(struct stat, st_mtim) == 88, "Dodo Linux stat timestamp offset mismatch");
int32_t dodo_fs_open(const unsigned char *path, int32_t flags, uint32_t mode) {
    return open((const char *)path, flags, (mode_t)mode);
}
#endif
