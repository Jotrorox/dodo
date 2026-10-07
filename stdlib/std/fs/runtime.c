/* The open(2) entry point is variadic, and struct stat, struct dirent and the
 * O_* values differ between architectures and C libraries. Keep those details
 * here, where the target's own headers define them, so the Dodo adapter only
 * sees Dodo-owned layouts and flag bits. */
#if !defined(_WIN32)
/* renameat2 (glibc), renamex_np, O_NOFOLLOW and st_mtimespec (Darwin) are
 * extensions that strict POSIX mode hides. */
#if defined(__APPLE__)
#define _DARWIN_C_SOURCE
#else
#define _GNU_SOURCE
#endif
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <stddef.h>
#include <sys/stat.h>
_Static_assert(sizeof(void *) == 8, "Dodo POSIX native handles require a 64-bit target");

/* Mirrors `Stat` in std/fs/posix.dodo. */
typedef struct {
    uint64_t device;
    uint64_t inode;
    uint64_t links;
    int64_t size;
    int64_t modified_seconds;
    int64_t modified_nanos;
    uint32_t mode;
    uint32_t uid;
    uint32_t gid;
    uint32_t padding;
} dodo_fs_stat_t;
_Static_assert(sizeof(dodo_fs_stat_t) == 64, "Dodo stat record layout mismatch");
_Static_assert(offsetof(dodo_fs_stat_t, mode) == 48, "Dodo stat record mode offset mismatch");

/* Portable bits chosen by open_file in std/fs/posix.dodo. */
enum {
    DODO_OPEN_READ = 1,
    DODO_OPEN_WRITE = 2,
    DODO_OPEN_APPEND = 4,
    DODO_OPEN_TRUNCATE = 8,
    DODO_OPEN_CREATE = 16,
    DODO_OPEN_EXCLUSIVE = 32,
    DODO_OPEN_NOFOLLOW = 64,
};

int32_t dodo_fs_open(const unsigned char *path, uint32_t flags, uint32_t mode) {
    int writes = (flags & (DODO_OPEN_WRITE | DODO_OPEN_APPEND)) != 0;
    int native = O_CLOEXEC;
    if ((flags & DODO_OPEN_READ) && writes) {
        native |= O_RDWR;
    } else if (writes) {
        native |= O_WRONLY;
    } else {
        native |= O_RDONLY;
    }
    if (flags & DODO_OPEN_APPEND) native |= O_APPEND;
    if (flags & DODO_OPEN_TRUNCATE) native |= O_TRUNC;
    if (flags & DODO_OPEN_CREATE) native |= O_CREAT;
    if (flags & DODO_OPEN_EXCLUSIVE) native |= O_EXCL;
    if (flags & DODO_OPEN_NOFOLLOW) native |= O_NOFOLLOW;
    return open((const char *)path, native, (mode_t)mode);
}

static void dodo_fs_copy_stat(const struct stat *source, dodo_fs_stat_t *output) {
    output->device = (uint64_t)source->st_dev;
    output->inode = (uint64_t)source->st_ino;
    output->links = (uint64_t)source->st_nlink;
    output->size = (int64_t)source->st_size;
#if defined(__APPLE__)
    output->modified_seconds = (int64_t)source->st_mtimespec.tv_sec;
    output->modified_nanos = (int64_t)source->st_mtimespec.tv_nsec;
#else
    output->modified_seconds = (int64_t)source->st_mtim.tv_sec;
    output->modified_nanos = (int64_t)source->st_mtim.tv_nsec;
#endif
    output->mode = (uint32_t)source->st_mode;
    output->uid = (uint32_t)source->st_uid;
    output->gid = (uint32_t)source->st_gid;
    output->padding = 0;
}

int32_t dodo_fs_stat(const unsigned char *path, int32_t follow, dodo_fs_stat_t *output) {
    struct stat data;
    int code = follow ? stat((const char *)path, &data) : lstat((const char *)path, &data);
    if (code == 0) dodo_fs_copy_stat(&data, output);
    return code;
}

int32_t dodo_fs_fstat(int32_t fd, dodo_fs_stat_t *output) {
    struct stat data;
    int code = fstat(fd, &data);
    if (code == 0) dodo_fs_copy_stat(&data, output);
    return code;
}

const unsigned char *dodo_fs_entry_name(const struct dirent *entry) {
    return (const unsigned char *)entry->d_name;
}

DIR *dodo_fs_open_directory(const unsigned char *path) {
    return opendir((const char *)path);
}

/* Null with errno zero marks the end of the stream; readdir leaves errno
 * unchanged there, so clear it first. */
struct dirent *dodo_fs_read_directory(DIR *directory) {
    errno = 0;
    return readdir(directory);
}

int32_t dodo_fs_close_directory(DIR *directory) {
    return closedir(directory);
}

unsigned char *dodo_fs_realpath(const unsigned char *path, unsigned char *output, size_t capacity) {
    if (capacity < PATH_MAX) {
        errno = ERANGE;
        return NULL;
    }
    return (unsigned char *)realpath((const char *)path, (char *)output);
}

int32_t dodo_fs_rename_exclusive(const unsigned char *old, const unsigned char *new) {
#if defined(__APPLE__)
    return renamex_np((const char *)old, (const char *)new, RENAME_EXCL);
#else
    return renameat2(AT_FDCWD, (const char *)old, AT_FDCWD, (const char *)new, RENAME_NOREPLACE);
#endif
}
#endif
