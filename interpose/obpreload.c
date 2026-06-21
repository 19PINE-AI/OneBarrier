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
#include <sys/socket.h>
#include <sys/types.h>
#include <unistd.h>

#define MAXFD 65536

static ssize_t (*real_read)(int, void *, size_t) = NULL;
static ssize_t (*real_recv)(int, void *, size_t, int) = NULL;
static int (*real_accept)(int, struct sockaddr *, socklen_t *) = NULL;
static int (*real_accept4)(int, struct sockaddr *, socklen_t *, int) = NULL;
static int (*real_close)(int) = NULL;

static unsigned char is_conn[MAXFD];   /* fd -> accepted server connection?     */
static uint32_t conn_id_of[MAXFD];     /* fd -> stable capture connection id     */
static FILE *cap = NULL;
static uint32_t next_conn_id = 1;
static pthread_mutex_t lock = PTHREAD_MUTEX_INITIALIZER;

static void ob_init(void) {
    if (!real_read)    real_read    = dlsym(RTLD_NEXT, "read");
    if (!real_recv)    real_recv    = dlsym(RTLD_NEXT, "recv");
    if (!real_accept)  real_accept  = dlsym(RTLD_NEXT, "accept");
    if (!real_accept4) real_accept4 = dlsym(RTLD_NEXT, "accept4");
    if (!real_close)   real_close   = dlsym(RTLD_NEXT, "close");
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
    if (cap) {
        uint32_t cid = conn_id_of[fd];
        uint32_t len = (uint32_t)n;
        fwrite(&cid, 4, 1, cap);
        fwrite(&len, 4, 1, cap);
        fwrite(buf, 1, (size_t)n, cap);
        fflush(cap);
    }
    pthread_mutex_unlock(&lock);
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
