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
        uint64_t dst = req->data.args[0];
        size_t len = (size_t)req->data.args[1];
        __atomic_fetch_add(&rng_calls, 1, __ATOMIC_RELAXED);
        __atomic_fetch_add(&rng_bytes, len, __ATOMIC_RELAXED);
        memset(resp, 0, psz);
        resp->id = req->id;
        // ensure the notification is still live before touching remote memory
        if (ioctl(notif_fd, SECCOMP_IOCTL_NOTIF_ID_VALID, &req->id) < 0) continue;
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
    struct sock_filter filt[] = {
        // arch check
        BPF_STMT(BPF_LD | BPF_W | BPF_ABS, offsetof(struct seccomp_data, arch)),
        BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, AUDIT_ARCH_X86_64, 1, 0),
        BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ALLOW),
        // nr
        BPF_STMT(BPF_LD | BPF_W | BPF_ABS, offsetof(struct seccomp_data, nr)),
        BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, __NR_getrandom, 0, 1),
        BPF_STMT(BPF_RET | BPF_K, count_only ? SECCOMP_RET_ALLOW : SECCOMP_RET_USER_NOTIF),
        BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ALLOW),
    };
    struct sock_fprog prog = { .len = sizeof filt / sizeof filt[0], .filter = filt };
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
