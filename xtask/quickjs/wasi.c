// WASI's calls answered inside the module, so it imports nothing (.sb/codemode-compute-plan.md §3). wasi-libc
// declares each as `__imported_wasi_snapshot_preview1_<name>` with an import attribute; a definition here
// resolves the symbol, and wasm-ld imports only what stays undefined. No file, no environment, no clock (time
// is 0), randomness from a fixed seed; writes to stdout/stderr are dropped, said written.
#include <stdint.h>

#define W(name) __imported_wasi_snapshot_preview1_##name
#define ESUCCESS 0
#define EBADF 8
#define ENOSYS 52

typedef int32_t i32;
typedef int64_t i64;

i32 W(args_get)(i32 a, i32 b) { return ESUCCESS; }
i32 W(args_sizes_get)(i32 count, i32 size) {
    *(uint32_t *)(uintptr_t)count = 0;
    *(uint32_t *)(uintptr_t)size = 0;
    return ESUCCESS;
}
i32 W(environ_get)(i32 a, i32 b) { return ESUCCESS; }
i32 W(environ_sizes_get)(i32 count, i32 size) {
    *(uint32_t *)(uintptr_t)count = 0;
    *(uint32_t *)(uintptr_t)size = 0;
    return ESUCCESS;
}
i32 W(clock_res_get)(i32 id, i32 out) {
    *(uint64_t *)(uintptr_t)out = 1;
    return ESUCCESS;
}
i32 W(clock_time_get)(i32 id, i64 precision, i32 out) {
    *(uint64_t *)(uintptr_t)out = 0;
    return ESUCCESS;
}
i32 W(random_get)(i32 buf, i32 len) {
    static uint64_t s = 0x9e3779b97f4a7c15u;
    uint8_t *p = (uint8_t *)(uintptr_t)buf;
    for (i32 i = 0; i < len; i++) {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        p[i] = (uint8_t)s;
    }
    return ESUCCESS;
}
i32 W(fd_write)(i32 fd, i32 iovs, i32 n, i32 written) {
    if (fd != 1 && fd != 2)
        return EBADF;
    uint32_t total = 0;
    const uint32_t *v = (const uint32_t *)(uintptr_t)iovs;
    for (i32 i = 0; i < n; i++)
        total += v[2 * i + 1];
    *(uint32_t *)(uintptr_t)written = total;
    return ESUCCESS;
}
_Noreturn void W(proc_exit)(i32 code) { __builtin_trap(); }
i32 W(sched_yield)(void) { return ESUCCESS; }

i32 W(fd_advise)(i32 a, i64 b, i64 c, i32 d) { return EBADF; }
i32 W(fd_allocate)(i32 a, i64 b, i64 c) { return EBADF; }
i32 W(fd_close)(i32 a) { return EBADF; }
i32 W(fd_datasync)(i32 a) { return EBADF; }
i32 W(fd_fdstat_get)(i32 a, i32 b) { return EBADF; }
i32 W(fd_fdstat_set_flags)(i32 a, i32 b) { return EBADF; }
i32 W(fd_fdstat_set_rights)(i32 a, i64 b, i64 c) { return EBADF; }
i32 W(fd_filestat_get)(i32 a, i32 b) { return EBADF; }
i32 W(fd_filestat_set_size)(i32 a, i64 b) { return EBADF; }
i32 W(fd_filestat_set_times)(i32 a, i64 b, i64 c, i32 d) { return EBADF; }
i32 W(fd_pread)(i32 a, i32 b, i32 c, i64 d, i32 e) { return EBADF; }
i32 W(fd_prestat_get)(i32 a, i32 b) { return EBADF; }
i32 W(fd_prestat_dir_name)(i32 a, i32 b, i32 c) { return EBADF; }
i32 W(fd_pwrite)(i32 a, i32 b, i32 c, i64 d, i32 e) { return EBADF; }
i32 W(fd_read)(i32 a, i32 b, i32 c, i32 d) { return EBADF; }
i32 W(fd_readdir)(i32 a, i32 b, i32 c, i64 d, i32 e) { return EBADF; }
i32 W(fd_renumber)(i32 a, i32 b) { return EBADF; }
i32 W(fd_seek)(i32 a, i64 b, i32 c, i32 d) { return EBADF; }
i32 W(fd_sync)(i32 a) { return EBADF; }
i32 W(fd_tell)(i32 a, i32 b) { return EBADF; }
i32 W(path_create_directory)(i32 a, i32 b, i32 c) { return ENOSYS; }
i32 W(path_filestat_get)(i32 a, i32 b, i32 c, i32 d, i32 e) { return ENOSYS; }
i32 W(path_filestat_set_times)(i32 a, i32 b, i32 c, i32 d, i64 e, i64 f, i32 g) { return ENOSYS; }
i32 W(path_link)(i32 a, i32 b, i32 c, i32 d, i32 e, i32 f, i32 g) { return ENOSYS; }
i32 W(path_open)(i32 a, i32 b, i32 c, i32 d, i32 e, i64 f, i64 g, i32 h, i32 i) { return ENOSYS; }
i32 W(path_readlink)(i32 a, i32 b, i32 c, i32 d, i32 e, i32 f) { return ENOSYS; }
i32 W(path_remove_directory)(i32 a, i32 b, i32 c) { return ENOSYS; }
i32 W(path_rename)(i32 a, i32 b, i32 c, i32 d, i32 e, i32 f) { return ENOSYS; }
i32 W(path_symlink)(i32 a, i32 b, i32 c, i32 d, i32 e) { return ENOSYS; }
i32 W(path_unlink_file)(i32 a, i32 b, i32 c) { return ENOSYS; }
i32 W(poll_oneoff)(i32 a, i32 b, i32 c, i32 d) { return ENOSYS; }
i32 W(sock_accept)(i32 a, i32 b, i32 c) { return ENOSYS; }
i32 W(sock_recv)(i32 a, i32 b, i32 c, i32 d, i32 e, i32 f) { return ENOSYS; }
i32 W(sock_send)(i32 a, i32 b, i32 c, i32 d, i32 e) { return ENOSYS; }
i32 W(sock_shutdown)(i32 a, i32 b) { return ENOSYS; }
