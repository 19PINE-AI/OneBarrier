// OneBarrier deterministic multithreading (DMT) via LD_PRELOAD.
//
// Problem: with >1 thread, the ORDER in which threads enter critical sections is
// chosen by the OS scheduler and varies run-to-run, so a multithreaded server's
// state evolution is nondeterministic and cannot be replayed.
//
// Solution: Kendo-style deterministic logical clocks (Olszewski et al., ASPLOS'09).
// Each thread carries a logical clock (LC). A thread may enter a synchronization
// operation (mutex/rwlock acquire) only when its (LC, slot) pair is the global
// MINIMUM among active threads; it then advances its LC. Because the order is a
// function of the deterministic LC values — not of OS timing — the interleaving of
// critical sections is identical on every run (for race-free programs). This makes
// a multithreaded program's lock-acquisition order replayable.
//
// Build:  gcc -shared -fPIC -O2 -o libdetsched.so detsched.c -ldl -lpthread
// Use:    OB_DETSCHED=1 LD_PRELOAD=./libdetsched.so <app>
//
// Scope/assumptions (Kendo's domain): well-synchronized (race-free) programs whose
// threads make progress through sync operations. A thread that parks on a condvar
// is removed from the minimum; a thread that exits is removed. Threads that spin in
// pure computation without ever syncing are out of scope (Kendo uses HW perf
// counters for that case; not attempted here).
#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <unistd.h>
#include <sched.h>
#include <dlfcn.h>
#include <pthread.h>

#define MAXT 1024
struct slot { _Atomic long lc; _Atomic int active; };
static struct slot slots[MAXT];
static _Atomic int next_slot = 0;
static int enabled = 0;
static __thread int my_slot = -1;
static __thread int lock_depth = 0;   // # locks this thread currently holds

static int  (*real_lock)(pthread_mutex_t *);
static int  (*real_unlock)(pthread_mutex_t *);
static int  (*real_trylock)(pthread_mutex_t *);
static int  (*real_rdlock)(pthread_rwlock_t *);
static int  (*real_wrlock)(pthread_rwlock_t *);
static int  (*real_rwunlock)(pthread_rwlock_t *);
static int  (*real_cond_wait)(pthread_cond_t *, pthread_mutex_t *);
static int  (*real_create)(pthread_t *, const pthread_attr_t *, void *(*)(void *), void *);
static int  (*real_join)(pthread_t, void **);

static int alloc_slot(void) {
    int s = __atomic_fetch_add(&next_slot, 1, __ATOMIC_SEQ_CST);
    if (s >= MAXT) return -1;
    __atomic_store_n(&slots[s].lc, 0, __ATOMIC_SEQ_CST);
    __atomic_store_n(&slots[s].active, 1, __ATOMIC_SEQ_CST);
    return s;
}
static void ensure_registered(void) {
    if (my_slot < 0) my_slot = alloc_slot();
}

// Production-safety bound: if a thread cannot get its deterministic turn within
// this many yields, it proceeds anyway (best-effort) rather than hang. In a
// well-synchronized program the turn arrives in O(threads) yields, far under the
// bound, so determinism is exact; the bound only trips on pathological init
// handshakes in complex real servers, trading strict determinism for liveness.
// 0 = unbounded (strict). Override with OB_DETSCHED_SPIN=<n>. Default balances
// strict determinism for normal contention (turns resolve in O(threads) yields,
// far below the cap → 0 relaxations) against fast startup for lock-heavy server
// init (an unbounded cap stalls memcached's init for seconds).
static long spin_cap = 50000;
static _Atomic long relaxations = 0;

// Block until this thread's (LC, slot) is the minimum among active threads.
static void await_turn(void) {
    if (!enabled || my_slot < 0) return;
    long spins = 0;
    for (;;) {
        long mylc = __atomic_load_n(&slots[my_slot].lc, __ATOMIC_SEQ_CST);
        int n = __atomic_load_n(&next_slot, __ATOMIC_SEQ_CST);
        int ismin = 1;
        for (int i = 0; i < n; i++) {
            if (i == my_slot) continue;
            if (!__atomic_load_n(&slots[i].active, __ATOMIC_SEQ_CST)) continue;
            long lc = __atomic_load_n(&slots[i].lc, __ATOMIC_SEQ_CST);
            if (lc < mylc || (lc == mylc && i < my_slot)) { ismin = 0; break; }
        }
        if (ismin) return;
        if (spin_cap > 0 && ++spins >= spin_cap) {
            __atomic_fetch_add(&relaxations, 1, __ATOMIC_RELAXED);
            return;  // best-effort: proceed without the turn
        }
        sched_yield();
    }
}
static void advance(void) {
    if (enabled && my_slot >= 0)
        __atomic_fetch_add(&slots[my_slot].lc, 1, __ATOMIC_SEQ_CST);
}

// Resolve real symbols. Done in the high-priority constructor below before any
// app thread runs; the lazy `if(!real_x) resolve()` in each wrapper is a belt-and-
// braces guard. We must NEVER skip the real lock/unlock (an earlier no-op shortcut
// corrupted libc-internal locks and hung threaded servers) — always call through.
static void resolve(void) {
    if (real_lock) return;
    real_unlock   = dlsym(RTLD_NEXT, "pthread_mutex_unlock");
    real_trylock  = dlsym(RTLD_NEXT, "pthread_mutex_trylock");
    real_rdlock   = dlsym(RTLD_NEXT, "pthread_rwlock_rdlock");
    real_wrlock   = dlsym(RTLD_NEXT, "pthread_rwlock_wrlock");
    real_rwunlock = dlsym(RTLD_NEXT, "pthread_rwlock_unlock");
    // pthread_cond_wait has two glibc ABI versions; plain dlsym returns the OLD
    // GLIBC_2.2.5 compat shim, which is incompatible with the (un-interposed)
    // GLIBC_2.3.2 pthread_cond_signal an app uses → lost wakeups / hangs. Bind the
    // matching modern version explicitly.
    real_cond_wait= dlvsym(RTLD_NEXT, "pthread_cond_wait", "GLIBC_2.3.2");
    if (!real_cond_wait) real_cond_wait = dlsym(RTLD_NEXT, "pthread_cond_wait");
    real_create   = dlsym(RTLD_NEXT, "pthread_create");
    real_join     = dlsym(RTLD_NEXT, "pthread_join");
    real_lock     = dlsym(RTLD_NEXT, "pthread_mutex_lock");   // set last: gates `resolve`
}

// Gate only TOP-LEVEL (depth-0) acquisitions: a thread waits for its deterministic
// turn only when it holds no locks, so it can never block another thread while
// parked at the gate. Nested acquisitions take the real lock directly. This keeps
// the order of top-level critical-section entry deterministic while being
// deadlock-free for correctly-locked (race-free, no app-level lock-order bug)
// programs — including nested locking, which deadlocked the naive scheme.
int pthread_mutex_lock(pthread_mutex_t *m) {
    if (!real_lock) { resolve(); }
    if (!enabled) return real_lock(m);
    ensure_registered();
    if (lock_depth == 0) { await_turn(); int r = real_lock(m); lock_depth++; advance(); return r; }
    int r = real_lock(m); lock_depth++; return r;
}
int pthread_mutex_trylock(pthread_mutex_t *m) {
    if (!real_trylock) { resolve(); }
    if (!enabled) return real_trylock(m);
    ensure_registered();
    if (lock_depth == 0) await_turn();
    int r = real_trylock(m);
    if (r == 0) { lock_depth++; if (lock_depth == 1) advance(); }
    return r;
}
int pthread_rwlock_rdlock(pthread_rwlock_t *m) {
    if (!real_rdlock) { resolve(); }
    if (!enabled) return real_rdlock(m);
    ensure_registered();
    if (lock_depth == 0) { await_turn(); int r = real_rdlock(m); lock_depth++; advance(); return r; }
    int r = real_rdlock(m); lock_depth++; return r;
}
int pthread_rwlock_wrlock(pthread_rwlock_t *m) {
    if (!real_wrlock) { resolve(); }
    if (!enabled) return real_wrlock(m);
    ensure_registered();
    if (lock_depth == 0) { await_turn(); int r = real_wrlock(m); lock_depth++; advance(); return r; }
    int r = real_wrlock(m); lock_depth++; return r;
}
int pthread_mutex_unlock(pthread_mutex_t *m) {
    if (!real_unlock) { resolve(); }
    if (enabled && lock_depth > 0) lock_depth--;
    return real_unlock(m);
}
int pthread_rwlock_unlock(pthread_rwlock_t *m) {
    if (!real_rwunlock) { resolve(); }
    if (enabled && lock_depth > 0) lock_depth--;
    return real_rwunlock(m);
}

// While parked on a condvar, leave the minimum so others make progress.
int pthread_cond_wait(pthread_cond_t *c, pthread_mutex_t *m) {
    if (!real_cond_wait) resolve();
    ensure_registered();
    if (enabled && my_slot >= 0) __atomic_store_n(&slots[my_slot].active, 0, __ATOMIC_SEQ_CST);
    int r = real_cond_wait(c, m);
    if (enabled && my_slot >= 0) { __atomic_store_n(&slots[my_slot].active, 1, __ATOMIC_SEQ_CST); advance(); }
    return r;
}

// A thread blocked in join makes no sync progress; remove it from the minimum so
// the threads it is waiting for can run (otherwise a joiner at LC=0 deadlocks all).
int pthread_join(pthread_t th, void **ret) {
    if (!real_join) resolve();
    if (!enabled) return real_join(th, ret);
    int prev = -1;
    if (my_slot >= 0) { prev = __atomic_load_n(&slots[my_slot].active, __ATOMIC_SEQ_CST);
                        __atomic_store_n(&slots[my_slot].active, 0, __ATOMIC_SEQ_CST); }
    int r = real_join(th, ret);
    if (my_slot >= 0 && prev) __atomic_store_n(&slots[my_slot].active, 1, __ATOMIC_SEQ_CST);
    return r;
}

struct tramp { void *(*fn)(void *); void *arg; int slot; };
static void *trampoline(void *p) {
    struct tramp t = *(struct tramp *)p;
    free(p);
    my_slot = t.slot;                // slot fixed by the parent ⇒ deterministic
    void *ret = t.fn(t.arg);
    if (my_slot >= 0) __atomic_store_n(&slots[my_slot].active, 0, __ATOMIC_SEQ_CST);
    return ret;
}
int pthread_create(pthread_t *th, const pthread_attr_t *a, void *(*fn)(void *), void *arg) {
    if (!real_create) resolve();
    if (!enabled) return real_create(th, a, fn, arg);
    struct tramp *t = malloc(sizeof *t);
    // Allocate the slot HERE, in the (deterministically-ordered) creating thread,
    // so a thread's slot follows program creation order, not OS start timing.
    t->fn = fn; t->arg = arg; t->slot = alloc_slot();
    return real_create(th, a, trampoline, t);
}

__attribute__((constructor(101)))
static void detsched_init(void) {
    resolve();
    enabled = getenv("OB_DETSCHED") ? 1 : 0;
    const char *sc = getenv("OB_DETSCHED_SPIN");
    if (sc) spin_cap = atol(sc);
    if (enabled) my_slot = alloc_slot();   // main thread = slot 0
}

__attribute__((destructor))
static void detsched_fini(void) {
    if (enabled && getenv("OB_DETSCHED_STATS")) {
        char b[96];
        int n = snprintf(b, sizeof b, "[detsched] threads=%d turn-relaxations=%ld\n",
                         __atomic_load_n(&next_slot, __ATOMIC_SEQ_CST),
                         __atomic_load_n(&relaxations, __ATOMIC_RELAXED));
        if (write(2, b, n) < 0) {}
    }
}
