// OneBarrier deterministic RNG (raw-syscall level) via seccomp-BPF user notification.
//
// Problem: V8/Node, OpenSSL, glibc arc4random, etc. seed their PRNGs from the RAW
// `getrandom(2)` syscall (and sometimes /dev/urandom). Raw syscalls bypass
// LD_PRELOAD symbol interposition, so the time-style libc shim cannot see them.
//
// Solution: install a seccomp filter that returns SECCOMP_RET_USER_NOTIF for
// getrandom; a supervisor thread in the SAME process receives each notification,
// fills the caller's buffer with bytes from a DETERMINISTIC stream (splitmix64
// seeded from a persisted seed), and answers the syscall WITHOUT the kernel ever
// running it. Because the seed is persisted (OB_VRAND=<file>) and the call order
// is deterministic (deterministic input + virtual clock), the live run and the
// post-crash replay observe the IDENTICAL random stream — so any PRNG seeded from
// getrandom (V8 Math.random, OpenSSL, ...) replays deterministically.
//
// Build:  gcc -shared -fPIC -O2 -o librngdet.so rngdet.c -lpthread
// Use:    OB_VRAND=/tmp/seed LD_PRELOAD=./librngdet.so <app>      (deterministic)
//         OB_RNGCOUNT=1      LD_PRELOAD=./librngdet.so <app>      (count only, RET_ALLOW)
//
// Composes with obpreload.so (time/sockets): LD_PRELOAD="./librngdet.so ./libobpreload.so".
#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stddef.h>
#include <stdint.h>
#include <unistd.h>
#include <fcntl.h>
#include <stdarg.h>
#include <sys/types.h>
#include <dlfcn.h>
#include <errno.h>
#include <pthread.h>
#include <sys/prctl.h>
#include <sys/syscall.h>
#include <sys/ioctl.h>
#include <sys/uio.h>
#include <linux/seccomp.h>
#include <linux/filter.h>
#include <linux/audit.h>
#include <linux/unistd.h>

#ifndef SECCOMP_RET_USER_NOTIF
#define SECCOMP_RET_USER_NOTIF 0x7fc00000U
#endif
#ifndef SECCOMP_FILTER_FLAG_NEW_LISTENER
#define SECCOMP_FILTER_FLAG_NEW_LISTENER (1UL << 3)
#endif
#ifndef SECCOMP_USER_NOTIF_FLAG_CONTINUE
#define SECCOMP_USER_NOTIF_FLAG_CONTINUE (1UL << 0)
#endif
#ifndef SECCOMP_IOCTL_NOTIF_RECV
#define SECCOMP_IOCTL_NOTIF_RECV  SECCOMP_IOR(0, struct seccomp_notif)
#define SECCOMP_IOCTL_NOTIF_SEND  SECCOMP_IOWR(1, struct seccomp_notif_resp)
#define SECCOMP_IOCTL_NOTIF_ID_VALID SECCOMP_IOW(2, __u64)
#endif

#ifndef __NR_getrandom
#define __NR_getrandom 318
#endif
#ifndef __NR_openat
#define __NR_openat 257
#endif
#ifndef __NR_open
#define __NR_open 2
#endif
#ifndef SECCOMP_IOCTL_NOTIF_ADDFD
struct seccomp_notif_addfd { __u64 id; __u32 flags; __u32 srcfd; __u32 newfd; __u32 newfd_flags; };
#define SECCOMP_IOCTL_NOTIF_ADDFD SECCOMP_IOWR(3, struct seccomp_notif_addfd)
#endif
#ifndef SECCOMP_ADDFD_FLAG_SEND
#define SECCOMP_ADDFD_FLAG_SEND (1UL << 1)
#endif

static int notif_fd = -1;
static int count_only = 0;
static uint64_t rng_calls = 0, rng_bytes = 0;

// --- deterministic byte stream (splitmix64) ---
static pthread_mutex_t sm_lock = PTHREAD_MUTEX_INITIALIZER;
static uint64_t sm_state;
static uint64_t sm_next(void) {
    uint64_t z = (sm_state += 0x9E3779B97F4A7C15ULL);
    z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9ULL;
    z = (z ^ (z >> 27)) * 0x94D049BB133111EBULL;
    return z ^ (z >> 31);
}
static void det_fill(unsigned char *out, size_t n) {
    pthread_mutex_lock(&sm_lock);
    size_t i = 0;
    while (i < n) {
        uint64_t r = sm_next();
        size_t k = (n - i < 8) ? (n - i) : 8;
        memcpy(out + i, &r, k);
        i += k;
    }
    pthread_mutex_unlock(&sm_lock);
}

// --- /dev/urandom & /dev/random determinizer ---------------------------------
// Some apps (redis 6 getRandomBytes, others) seed their PRNG by reading
// /dev/urandom directly — not the getrandom(2) syscall — so the seccomp trap above
// doesn't see it. Interposing read() doesn't help either: glibc's fread uses an
// internal read path that bypasses the public `read` symbol. So we redirect the
// OPEN: an open of /dev/urandom returns a memfd pre-filled with bytes from a
// deterministic stream, so EVERY read path (read/fread/pread) gets the same bytes.
#define UR_BYTES (1u << 20)   // 1 MiB of deterministic entropy (apps seed with << this)
static int urand_on = 0;
static int vpid_on = 0;
static uint64_t ur_state;
static int is_randpath(const char *p) {
    return p && (!strcmp(p, "/dev/urandom") || !strcmp(p, "/dev/random") ||
                 !strcmp(p, "/dev/srandom") || !strcmp(p, "/dev/hwrng"));
}
static int  (*real_open)(const char *, int, ...);
static int  (*real_open64)(const char *, int, ...);
static int  (*real_openat)(int, const char *, int, ...);
static void ur_resolve(void) {
    if (real_openat) return;
    real_open   = (int (*)(const char *, int, ...))dlsym(RTLD_NEXT, "open");
    real_open64 = (int (*)(const char *, int, ...))dlsym(RTLD_NEXT, "open64");
    real_openat = (int (*)(int, const char *, int, ...))dlsym(RTLD_NEXT, "openat");
}
// Build a memfd holding UR_BYTES of deterministic bytes; return its fd at offset 0.
static int det_urandom_fd(void) {
    int fd = (int)syscall(SYS_memfd_create, "ob-urandom", 0);
    if (fd < 0) return -1;
    unsigned char buf[4096];
    uint64_t s = ur_state;
    for (unsigned off = 0; off < UR_BYTES; off += sizeof buf) {
        for (size_t i = 0; i < sizeof buf; i += 8) {
            uint64_t z = (s += 0x9E3779B97F4A7C15ULL);
            z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9ULL;
            z = (z ^ (z >> 27)) * 0x94D049BB133111EBULL;
            z ^= (z >> 31);
            memcpy(buf + i, &z, 8);
        }
        if (write(fd, buf, sizeof buf) < 0) { close(fd); return -1; }
    }
    lseek(fd, 0, SEEK_SET);
    return fd;
}

int open(const char *path, int flags, ...) {
    if (!real_open) ur_resolve();
    if (urand_on && is_randpath(path)) { int f = det_urandom_fd(); if (f >= 0) return f; }
    mode_t m = 0; if (flags & O_CREAT) { va_list a; va_start(a, flags); m = va_arg(a, int); va_end(a); }
    return real_open(path, flags, m);
}
int open64(const char *path, int flags, ...) {
    if (!real_open64) ur_resolve();
    if (urand_on && is_randpath(path)) { int f = det_urandom_fd(); if (f >= 0) return f; }
    mode_t m = 0; if (flags & O_CREAT) { va_list a; va_start(a, flags); m = va_arg(a, int); va_end(a); }
    return real_open64 ? real_open64(path, flags, m) : real_open(path, flags, m);
}
int openat(int dirfd, const char *path, int flags, ...) {
    if (!real_openat) ur_resolve();
    if (urand_on && is_randpath(path)) { int f = det_urandom_fd(); if (f >= 0) return f; }
    mode_t m = 0; if (flags & O_CREAT) { va_list a; va_start(a, flags); m = va_arg(a, int); va_end(a); }
    return real_openat(dirfd, path, flags, m);
}

// Pin getpid: apps mix it into RNG seeds (redis: srand(time^getpid)); a constant
// makes that seed reproducible across the live and recovered processes.
pid_t getpid(void) { return vpid_on ? (pid_t)4242 : (pid_t)syscall(SYS_getpid); }

// Write n bytes into the target process's buffer at remote address `dst`.
static int write_remote(pid_t pid, uint64_t dst, const void *src, size_t n) {
    struct iovec local = { (void *)src, n };
    struct iovec remote = { (void *)(uintptr_t)dst, n };
    ssize_t w = process_vm_writev(pid, &local, 1, &remote, 1, 0);
    if (w == (ssize_t)n) return 0;
    // fallback: /proc/<pid>/mem
    char path[64];
    snprintf(path, sizeof path, "/proc/%d/mem", pid);
    int fd = open(path, O_WRONLY);
    if (fd < 0) return -1;
    ssize_t pw = pwrite(fd, src, n, (off_t)dst);
    close(fd);
    return pw == (ssize_t)n ? 0 : -1;
}

// Read up to n bytes from the target's memory at `src` (a path string).
static int read_remote(pid_t pid, uint64_t src, void *dst, size_t n) {
    struct iovec l = { dst, n };
    struct iovec r = { (void *)(uintptr_t)src, n };
    ssize_t rd = process_vm_readv(pid, &l, 1, &r, 1, 0);
    return rd > 0 ? (int)rd : -1;
}

static void *supervisor(void *arg) {
    (void)arg;
    struct seccomp_notif_sizes sizes; memset(&sizes, 0, sizeof sizes);
    syscall(__NR_seccomp, SECCOMP_GET_NOTIF_SIZES, 0, &sizes);
    size_t rsz = sizes.seccomp_notif ? sizes.seccomp_notif : sizeof(struct seccomp_notif);
    size_t psz = sizes.seccomp_notif_resp ? sizes.seccomp_notif_resp : sizeof(struct seccomp_notif_resp);
    struct seccomp_notif *req = calloc(1, rsz);
    struct seccomp_notif_resp *resp = calloc(1, psz);
    unsigned char buf[4096];
    for (;;) {
        memset(req, 0, rsz);
        if (ioctl(notif_fd, SECCOMP_IOCTL_NOTIF_RECV, req) < 0) {
            if (errno == EINTR) continue;
            break;
        }
        memset(resp, 0, psz);
        resp->id = req->id;
        if (ioctl(notif_fd, SECCOMP_IOCTL_NOTIF_ID_VALID, &req->id) < 0) continue;
        int nr = req->data.nr;

        if (nr == __NR_openat || nr == __NR_open) {
            // Redirect opens of /dev/urandom etc. to a deterministic memfd, injected
            // into the target with ADDFD; pass everything else through (CONTINUE).
            uint64_t pathaddr = (nr == __NR_openat) ? req->data.args[1] : req->data.args[0];
            char p[256]; memset(p, 0, sizeof p);
            read_remote(req->pid, pathaddr, p, sizeof p - 1);
            if (urand_on && is_randpath(p)) {
                int mfd = det_urandom_fd();
                struct seccomp_notif_addfd af; memset(&af, 0, sizeof af);
                af.id = req->id; af.srcfd = (unsigned)mfd; af.flags = SECCOMP_ADDFD_FLAG_SEND; af.newfd = 0;
                long nf = ioctl(notif_fd, SECCOMP_IOCTL_NOTIF_ADDFD, &af);  // adds fd AND sends response
                if (mfd >= 0) close(mfd);
                __atomic_fetch_add(&rng_calls, 1, __ATOMIC_RELAXED);
                if (nf >= 0) continue;                 // done
                // fall through to CONTINUE on failure
            }
            resp->flags = SECCOMP_USER_NOTIF_FLAG_CONTINUE;
            ioctl(notif_fd, SECCOMP_IOCTL_NOTIF_SEND, resp);
            continue;
        }

        // getrandom: fill the target buffer from the deterministic stream
        uint64_t dst = req->data.args[0];
        size_t len = (size_t)req->data.args[1];
        __atomic_fetch_add(&rng_calls, 1, __ATOMIC_RELAXED);
        __atomic_fetch_add(&rng_bytes, len, __ATOMIC_RELAXED);
        int ok = 0;
        for (size_t off = 0; off < len; ) {
            size_t k = (len - off < sizeof buf) ? (len - off) : sizeof buf;
            det_fill(buf, k);
            if (write_remote(req->pid, dst + off, buf, k) < 0) { ok = -1; break; }
            off += k;
        }
        if (ok == 0) { resp->error = 0; resp->val = (long)len; resp->flags = 0; }
        else         { resp->error = 0; resp->val = 0;          resp->flags = SECCOMP_USER_NOTIF_FLAG_CONTINUE; }
        ioctl(notif_fd, SECCOMP_IOCTL_NOTIF_SEND, resp);
    }
    return NULL;
}

static int install_filter(void) {
    unsigned notif = count_only ? SECCOMP_RET_ALLOW : SECCOMP_RET_USER_NOTIF;
    // Trapping open/openat at the syscall level catches /dev/urandom reads that
    // bypass symbol interposition (glibc fopen), but it also traps the dynamic
    // linker's own opens, which is fragile during early init. OFF by default
    // (use the mount-namespace /dev/urandom redirect instead); opt in with
    // OB_VRAND_OPENAT=1 where it is safe.
    struct sock_filter filt_full[] = {
        BPF_STMT(BPF_LD | BPF_W | BPF_ABS, offsetof(struct seccomp_data, arch)),  // 0
        BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, AUDIT_ARCH_X86_64, 1, 0),             // 1
        BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ALLOW),                              // 2
        BPF_STMT(BPF_LD | BPF_W | BPF_ABS, offsetof(struct seccomp_data, nr)),    // 3
        BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, __NR_getrandom, 3, 0),                // 4 -> 8
        BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, __NR_openat,    2, 0),                // 5 -> 8
        BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, __NR_open,      1, 0),                // 6 -> 8
        BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ALLOW),                              // 7
        BPF_STMT(BPF_RET | BPF_K, notif),                                          // 8
    };
    struct sock_filter filt_gr[] = {   // getrandom only (robust)
        BPF_STMT(BPF_LD | BPF_W | BPF_ABS, offsetof(struct seccomp_data, arch)),
        BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, AUDIT_ARCH_X86_64, 1, 0),
        BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ALLOW),
        BPF_STMT(BPF_LD | BPF_W | BPF_ABS, offsetof(struct seccomp_data, nr)),
        BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, __NR_getrandom, 0, 1),
        BPF_STMT(BPF_RET | BPF_K, notif),
        BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ALLOW),
    };
    int trap_openat = getenv("OB_VRAND_OPENAT") ? 1 : 0;
    struct sock_fprog prog = trap_openat
        ? (struct sock_fprog){ .len = sizeof filt_full / sizeof filt_full[0], .filter = filt_full }
        : (struct sock_fprog){ .len = sizeof filt_gr   / sizeof filt_gr[0],   .filter = filt_gr };
    if (prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0)) return -1;
    if (count_only) {
        // count-only mode can't observe via RET_ALLOW; use a tracer-free no-op.
        return 0;
    }
    long fd = syscall(__NR_seccomp, SECCOMP_SET_MODE_FILTER,
                      SECCOMP_FILTER_FLAG_NEW_LISTENER, &prog);
    if (fd < 0) return -1;
    notif_fd = (int)fd;
    return 0;
}

static void load_seed(void) {
    uint64_t seed = 0x0B1A2C3D4E5F6071ULL;  // fixed default ⇒ reproducible even w/o a file
    const char *path = getenv("OB_VRAND");
    if (path && *path) {
        FILE *f = fopen(path, "r");
        if (f) { if (fscanf(f, "%llu", (unsigned long long *)&seed) != 1) seed = 0x0B1A2C3D4E5F6071ULL; fclose(f); }
        else { f = fopen(path, "w"); if (f) { fprintf(f, "%llu\n", (unsigned long long)seed); fclose(f); } }
    }
    sm_state = seed;
}

__attribute__((constructor))
static void rngdet_init(void) {
    count_only = getenv("OB_RNGCOUNT") ? 1 : 0;
    if (!count_only && !getenv("OB_VRAND")) return;  // inert unless asked
    load_seed();
    // /dev/urandom + getpid pinning travel with OB_VRAND (opt out via OB_NO_URANDOM)
    ur_resolve();
    ur_state = sm_state ^ 0xD1B54A32D192ED03ULL;     // distinct stream from getrandom
    urand_on = getenv("OB_NO_URANDOM") ? 0 : 1;
    vpid_on  = getenv("OB_NO_VPID") ? 0 : 1;
    if (count_only) { urand_on = 0; vpid_on = 0; }
    if (install_filter() != 0) {
        const char *m = "[rngdet] seccomp filter install failed; RNG NOT determinized\n";
        if (write(2, m, strlen(m)) < 0) {}
        return;
    }
    if (!count_only) {
        pthread_t t;
        pthread_create(&t, NULL, supervisor, NULL);
        pthread_detach(t);
    }
}

__attribute__((destructor))
static void rngdet_fini(void) {
    if (getenv("OB_RNGSTATS")) {
        char b[128];
        int n = snprintf(b, sizeof b, "[rngdet] getrandom trapped: %llu calls, %llu bytes\n",
                         (unsigned long long)rng_calls, (unsigned long long)rng_bytes);
        if (write(2, b, n) < 0) {}
    }
}
