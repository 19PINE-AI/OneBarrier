/*
 * obpreload — OneBarrier transparent interception shim (LD_PRELOAD).
 *
 * Interposes libc socket I/O (accept/accept4, read/recv, close) on an UNMODIFIED
 * binary and tees the inbound request bytes of every accepted server connection
 * into a OneBarrier capture log. The captured request stream is the
 * non-deterministic input; replaying it against a fresh instance deterministically
 * rebuilds state (transparent record-replay fault tolerance) — see ob-replay.
 *
 * This is the SocksDirect-lineage interception point, done in user space with no
 * kernel changes and no application changes. It demonstrates the transparent
 * vision; full generality (capturing all non-determinism — time, RNG, thread
 * scheduling — and multi-threaded replay) is the libOS's remaining work.
 *
 *   Build:  gcc -shared -fPIC -O2 -o libobpreload.so obpreload.c -ldl -lpthread
 *   Use:    OB_CAPTURE=/tmp/cap.log LD_PRELOAD=./libobpreload.so <server> ...
 *
 * Capture record (little-endian):  [u32 conn_id][u32 len][len bytes]
 */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/random.h>
#include <sys/socket.h>
#include <sys/syscall.h>
#include <sys/time.h>
#include <sys/types.h>
#include <time.h>
#include <unistd.h>
#include <fcntl.h>

#define MAXFD 65536

static ssize_t (*real_read)(int, void *, size_t) = NULL;
static ssize_t (*real_recv)(int, void *, size_t, int) = NULL;
static int (*real_accept)(int, struct sockaddr *, socklen_t *) = NULL;
static int (*real_accept4)(int, struct sockaddr *, socklen_t *, int) = NULL;
static int (*real_close)(int) = NULL;

/* Nondeterminism characterization (docs/PAPER-PLAN.md exp #5): the libc sources
 * of local nondeterminism the libOS must virtualize once the fabric has removed
 * message-order nondeterminism.  Counted by interposing the libc symbols. */
static int (*real_gettimeofday)(struct timeval *, void *) = NULL;
static int (*real_clock_gettime)(clockid_t, struct timespec *) = NULL;
static ssize_t (*real_getrandom)(void *, size_t, unsigned int) = NULL;
static time_t (*real_time)(time_t *) = NULL;
static long nd_requests = 0, nd_gettimeofday = 0, nd_clock_gettime = 0,
           nd_getrandom = 0, nd_time = 0;

/* record/replay state (see the rr_* section below) */
static FILE *rec_fp = NULL;
static unsigned char *rep_buf = NULL;
static size_t rep_len = 0, rep_cursor = 0;
static long rep_diverged = 0;
static int ob_mode = 0; /* 0 none, 1 record, 2 replay */

/* Virtual clock (OB_VCLOCK): deterministic time = base + ticks; ticks advance by
 * a fixed delta on each socket read (a deterministic input event), so time reads
 * are count-independent and timer-driven reads no longer desync replay. base is
 * captured at the live run start and persisted; the replay run reconstructs the
 * same virtual time for the same inputs -> byte-identical time-dependent output. */
#define VCLOCK_TICK_NS 1000000LL
static long long vclock_base_ns = 0;
static long vclock_ticks = 0;
static int vclock_on = 0;
/* OB_VCLOCK_TICKS=<file>: checkpoint/resume the tick count, enabling tail-replay
 * recovery from an app-native snapshot (e.g. redis RDB) instead of from process
 * start. On init the shim loads the starting tick offset from the file; on each
 * input event it persists the current tick count there. A recovery that restores
 * a checkpoint's state then sets this file to the checkpoint's tick value resumes
 * virtual time exactly where the checkpoint was taken. */
static int vclock_tick_fd = -1;
static pthread_mutex_t lock = PTHREAD_MUTEX_INITIALIZER;

static void nd_dump(void) {
    const char *path = getenv("OB_NDSTATS");
    if (!path) return;
    FILE *f = fopen(path, "w");
    if (!f) return;
    fprintf(f, "requests %ld\ngettimeofday %ld\nclock_gettime %ld\ngetrandom %ld\ntime %ld\nmode %d\nreplay_diverged %ld\n",
            nd_requests, nd_gettimeofday, nd_clock_gettime, nd_getrandom, nd_time, ob_mode, rep_diverged);
    fclose(f);
    if (rec_fp) fflush(rec_fp); /* keep the record durable across shutdown */
}

/* ---- record/replay of nondeterministic returns (time virtualization) ----
 * RECORD mode (OB_RECORD): append every time/random RESULT to a log.
 * REPLAY mode (OB_REPLAY): return the recorded results in order, so an
 * unmodified app re-executing the same inputs produces byte-identical output
 * (e.g. matching HTTP Date headers) — deterministic recovery.
 * Entry format: [u8 type][payload]; for fixed types payload is the result
 * struct, for getrandom it is [u32 len][len bytes]. */

static void rr_init(void) {
    const char *rep = getenv("OB_REPLAY");
    const char *rec = getenv("OB_RECORD");
    if (rep) {
        FILE *f = fopen(rep, "rb");
        if (f) {
            fseek(f, 0, SEEK_END);
            long n = ftell(f);
            fseek(f, 0, SEEK_SET);
            if (n > 0 && (rep_buf = malloc((size_t)n)) && fread(rep_buf, 1, (size_t)n, f) == (size_t)n)
                rep_len = (size_t)n;
            fclose(f);
            ob_mode = 2;
        }
    } else if (rec) {
        rec_fp = fopen(rec, "wb");
        if (rec_fp) {
            setvbuf(rec_fp, NULL, _IOFBF, 1 << 16);
            ob_mode = 1;
        }
    }
}

static long rr_writes = 0;
static void rr_record_fixed(unsigned char type, const void *p, size_t len) {
    pthread_mutex_lock(&lock);
    if (rec_fp) {
        fputc(type, rec_fp);
        fwrite(p, 1, len, rec_fp);
        if ((++rr_writes & 0xF) == 0) fflush(rec_fp);
    }
    pthread_mutex_unlock(&lock);
}
/* Replay a fixed-size entry of `type` into `p`. Returns 1 if replayed. */
static int rr_replay_fixed(unsigned char type, void *p, size_t len) {
    int ok = 0;
    pthread_mutex_lock(&lock);
    if (rep_cursor + 1 + len <= rep_len && rep_buf[rep_cursor] == type) {
        memcpy(p, rep_buf + rep_cursor + 1, len);
        rep_cursor += 1 + len;
        ok = 1;
    } else {
        rep_diverged++;
    }
    pthread_mutex_unlock(&lock);
    return ok;
}

static unsigned char is_conn[MAXFD];   /* fd -> accepted server connection?     */
static uint32_t conn_id_of[MAXFD];     /* fd -> stable capture connection id     */
static FILE *cap = NULL;
static uint32_t next_conn_id = 1;

static void ob_init(void) {
    if (!real_read)    real_read    = dlsym(RTLD_NEXT, "read");
    if (!real_recv)    real_recv    = dlsym(RTLD_NEXT, "recv");
    if (!real_accept)  real_accept  = dlsym(RTLD_NEXT, "accept");
    if (!real_accept4) real_accept4 = dlsym(RTLD_NEXT, "accept4");
    if (!real_close)   real_close   = dlsym(RTLD_NEXT, "close");
    if (!real_gettimeofday) real_gettimeofday = dlsym(RTLD_NEXT, "gettimeofday");
    if (!real_clock_gettime) real_clock_gettime = dlsym(RTLD_NEXT, "clock_gettime");
    if (!real_getrandom) real_getrandom = dlsym(RTLD_NEXT, "getrandom");
    if (!real_time)    real_time    = dlsym(RTLD_NEXT, "time");
    if (!cap) {
        const char *path = getenv("OB_CAPTURE");
        if (path) {
            cap = fopen(path, "ab");
            if (cap) setvbuf(cap, NULL, _IOFBF, 1 << 16);
        }
    }
}

static void mark_conn(int fd) {
    if (fd < 0 || fd >= MAXFD) return;
    pthread_mutex_lock(&lock);
    ob_init();
    is_conn[fd] = 1;
    conn_id_of[fd] = next_conn_id++;
    pthread_mutex_unlock(&lock);
}

static void capture(int fd, const void *buf, ssize_t n) {
    if (n <= 0 || fd < 0 || fd >= MAXFD || !is_conn[fd]) return;
    pthread_mutex_lock(&lock);
    nd_requests++;
    if (vclock_on) {
        long t = __atomic_add_fetch(&vclock_ticks, VCLOCK_TICK_NS, __ATOMIC_RELAXED);
        if (vclock_tick_fd >= 0) { long long v = t; if (pwrite(vclock_tick_fd, &v, 8, 0) < 0) {} }
    }
    if (cap) {
        uint32_t cid = conn_id_of[fd];
        uint32_t len = (uint32_t)n;
        fwrite(&cid, 4, 1, cap);
        fwrite(&len, 4, 1, cap);
        fwrite(buf, 1, (size_t)n, cap);
        fflush(cap);
    }
    if ((nd_requests & 0x3F) == 0) nd_dump(); /* periodic snapshot (survives kill) */
    pthread_mutex_unlock(&lock);
}

/* Resolve all real symbols once at load.  Robust against the LD_PRELOAD early-
 * init trap: the interceptors below NEVER call dlsym themselves — if the real
 * symbol isn't resolved yet (a call during ld.so init, before this constructor
 * runs) they fall back to the raw syscall.  No recursion, no TLS. */
static void vclock_init(void) {
    const char *p = getenv("OB_VCLOCK");
    if (!p) return;
    FILE *f = fopen(p, "rb");
    if (f) {
        if (fread(&vclock_base_ns, 8, 1, f) != 1) vclock_base_ns = 0;
        fclose(f);
    } else {
        struct timespec ts = {0, 0};
        if (real_clock_gettime) real_clock_gettime(CLOCK_REALTIME, &ts);
        vclock_base_ns = (long long)ts.tv_sec * 1000000000LL + ts.tv_nsec;
        FILE *w = fopen(p, "wb");
        if (w) { fwrite(&vclock_base_ns, 8, 1, w); fclose(w); }
    }
    /* tick checkpoint/resume file: load starting offset, keep an fd for persisting */
    const char *tp = getenv("OB_VCLOCK_TICKS");
    if (tp && *tp) {
        FILE *tf = fopen(tp, "rb");
        if (tf) { long long v = 0; if (fread(&v, 8, 1, tf) == 1) vclock_ticks = (long)v; fclose(tf); }
        vclock_tick_fd = open(tp, O_WRONLY | O_CREAT, 0644);
    }
    vclock_on = 1;
}
__attribute__((constructor)) static void ob_ctor(void) {
    ob_init();
    rr_init();
    vclock_init();
    atexit(nd_dump);
}

int gettimeofday(struct timeval *tv, void *tz) {
    long c = __atomic_add_fetch(&nd_gettimeofday, 1, __ATOMIC_RELAXED);
    if (vclock_on) {
        long long t = vclock_base_ns + __atomic_load_n(&vclock_ticks, __ATOMIC_RELAXED);
        if (tv) { tv->tv_sec = t / 1000000000LL; tv->tv_usec = (t % 1000000000LL) / 1000; }
        return 0;
    }
    if (ob_mode == 2 && rr_replay_fixed(1, tv, sizeof(*tv))) return 0;
    int r = real_gettimeofday ? real_gettimeofday(tv, tz) : (int)syscall(SYS_gettimeofday, tv, tz);
    if (ob_mode == 1) rr_record_fixed(1, tv, sizeof(*tv));
    if (real_gettimeofday && (c % 50000) == 0) nd_dump();
    return r;
}
int clock_gettime(clockid_t id, struct timespec *ts) {
    long c = __atomic_add_fetch(&nd_clock_gettime, 1, __ATOMIC_RELAXED);
    if (vclock_on) {
        long long t = vclock_base_ns + __atomic_load_n(&vclock_ticks, __ATOMIC_RELAXED);
        if (ts) { ts->tv_sec = t / 1000000000LL; ts->tv_nsec = t % 1000000000LL; }
        return 0;
    }
    if (ob_mode == 2 && rr_replay_fixed(2, ts, sizeof(*ts))) return 0;
    int r = real_clock_gettime ? real_clock_gettime(id, ts) : (int)syscall(SYS_clock_gettime, id, ts);
    if (ob_mode == 1) rr_record_fixed(2, ts, sizeof(*ts));
    if (real_clock_gettime && (c % 50000) == 0) nd_dump();
    return r;
}
time_t time(time_t *t) {
    __atomic_fetch_add(&nd_time, 1, __ATOMIC_RELAXED);
    if (vclock_on) {
        time_t v = (time_t)((vclock_base_ns + __atomic_load_n(&vclock_ticks, __ATOMIC_RELAXED)) / 1000000000LL);
        if (t) *t = v;
        return v;
    }
    time_t val;
    if (ob_mode == 2 && rr_replay_fixed(4, &val, sizeof(val))) {
        if (t) *t = val;
        return val;
    }
    val = real_time ? real_time(t) : (time_t)syscall(SYS_time, t);
    if (ob_mode == 1) rr_record_fixed(4, &val, sizeof(val));
    return val;
}
ssize_t getrandom(void *buf, size_t len, unsigned int flags) {
    __atomic_fetch_add(&nd_getrandom, 1, __ATOMIC_RELAXED);
    if (ob_mode == 2) {
        pthread_mutex_lock(&lock);
        if (rep_cursor + 5 <= rep_len && rep_buf[rep_cursor] == 3) {
            uint32_t rl;
            memcpy(&rl, rep_buf + rep_cursor + 1, 4);
            if (rep_cursor + 5 + rl <= rep_len) {
                size_t cp = rl < len ? rl : len;
                memcpy(buf, rep_buf + rep_cursor + 5, cp);
                rep_cursor += 5 + rl;
                pthread_mutex_unlock(&lock);
                return (ssize_t)cp;
            }
        }
        rep_diverged++;
        pthread_mutex_unlock(&lock);
    }
    ssize_t r = real_getrandom ? real_getrandom(buf, len, flags) : syscall(SYS_getrandom, buf, len, flags);
    if (ob_mode == 1 && r > 0) {
        pthread_mutex_lock(&lock);
        if (rec_fp) {
            uint32_t rl = (uint32_t)r;
            fputc(3, rec_fp);
            fwrite(&rl, 4, 1, rec_fp);
            fwrite(buf, 1, (size_t)r, rec_fp);
        }
        pthread_mutex_unlock(&lock);
    }
    return r;
}

int accept(int sockfd, struct sockaddr *addr, socklen_t *addrlen) {
    ob_init();
    int fd = real_accept(sockfd, addr, addrlen);
    if (fd >= 0) mark_conn(fd);
    return fd;
}

int accept4(int sockfd, struct sockaddr *addr, socklen_t *addrlen, int flags) {
    ob_init();
    int fd = real_accept4(sockfd, addr, addrlen, flags);
    if (fd >= 0) mark_conn(fd);
    return fd;
}

ssize_t read(int fd, void *buf, size_t count) {
    ob_init();
    ssize_t n = real_read(fd, buf, count);
    capture(fd, buf, n);
    return n;
}

ssize_t recv(int fd, void *buf, size_t len, int flags) {
    ob_init();
    ssize_t n = real_recv(fd, buf, len, flags);
    capture(fd, buf, n);
    return n;
}

int close(int fd) {
    ob_init();
    if (fd >= 0 && fd < MAXFD) {
        pthread_mutex_lock(&lock);
        is_conn[fd] = 0;
        conn_id_of[fd] = 0;
        pthread_mutex_unlock(&lock);
    }
    return real_close(fd);
}
