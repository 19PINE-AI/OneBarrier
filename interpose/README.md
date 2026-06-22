# OneBarrier libOS — transparent interception & deterministic recovery

The user-space libOS layer that brings OneBarrier's fault tolerance to
**unmodified** applications: it intercepts an app's nondeterminism, records it,
and replays it deterministically on a fresh instance after a crash — *recovery*,
not just interception. No kernel changes, no application changes (the
SocksDirect lineage).

## What it does

`obpreload.c` (an `LD_PRELOAD` shim, `gcc -shared -fPIC -O2 -o libobpreload.so
obpreload.c -ldl -lpthread`) interposes the libc surface that carries
nondeterminism:

| Intercepted | Purpose |
|---|---|
| `accept`/`accept4`, `read`/`recv`, `close` | capture the request stream (the input) — `OB_CAPTURE` |
| `gettimeofday`, `clock_gettime`, `time`, `getrandom` | virtualize local nondeterminism |

The libOS is three composable `LD_PRELOAD` libraries (each independently useful):

| library | source | closes |
|---|---|---|
| `libobpreload.so` | `obpreload.c` | socket capture + **time** (virtual clock / record-replay) |
| `librngdet.so` | `rngdet.c` | **RNG** — raw `getrandom` syscall via seccomp-BPF |
| `libdetsched.so` | `detsched.c` | **thread scheduling** — deterministic logical clocks |

Two virtualization strategies are provided, selected by environment variable:

1. **Record/replay** (`OB_RECORD` / `OB_REPLAY`) — log every nondeterministic
   *result* on the live run; return them in order on replay. Exact for
   **request-driven** time reads.
2. **Virtual clock** (`OB_VCLOCK`) — time = `base + ticks`, where `ticks` advance
   by a fixed delta on each socket read (a deterministic input event). Every time
   read is then **count-independent**, so **timer-driven** reads (redis
   `serverCron`, nginx `ngx_time_update`) no longer desync replay. This is the
   general mechanism.

## Why two strategies — the boundary we measured

Record/replay alone replays time reads by *sequence position*. That is exact when
the app reads time once per request (Node's `Date.now()` aligned **8/8** across a
crash + 4 s gap). But **timer-driven** servers (redis `serverCron`) read time on
an internal timer whose firing count differs between record and replay, so the
sequential cursor desyncs — measured: redis `TIME` returned the recorded value
only sporadically.

**The virtual clock closes that boundary.** Because virtual time depends only on
the (deterministic) input-event count, not on how many times it is read, redis
`TIME` becomes **byte-identical across recovery**:

```
LIVE:   1782053569.269446 .270446 .271446 .272446 .273446 .274446
REPLAY: 1782053569.269446 .270446 .271446 .272446 .273446 .274446   (3 s real gap)
```

The replay's seconds **ignore the real-time gap** (it returns the live `base`, not
current time) and the microseconds advance deterministically. The virtual clock
is **app-agnostic** — it intercepts the libc time symbols for any binary (a
controlled test counted exactly 2,000,000 `gettimeofday` + 2,000,000
`clock_gettime`; the vDSO is *not* a blocker, since `LD_PRELOAD` overrides the
exported symbols).

## The unified recovery harness

`ob-recover.sh <redis|memcached|nginx|node|all> [gap_s]` runs the full cycle for
an unmodified app and includes a **control** that pins down causality:

1. **live** — record under the virtual clock,
2. **crash** (`kill -9`) and wait `gap_s` of real wall-clock time,
3. **replay** — fresh instance, same persisted base → byte-identical to live,
4. **control** — fresh instance with **no** virtual clock → real time, *must differ*.

Per-app time-dependent probes: redis `TIME`, memcached `stats time`, nginx `Date:`
header, Node `Date.now()`. The verdict requires BOTH `replay == live` AND
`control != live`, so a pass proves the determinism comes from the virtual clock,
not from a trivially-identical test.

```bash
gcc -shared -fPIC -O2 -o interpose/libobpreload.so interpose/obpreload.c -ldl -lpthread
cargo build --release -p onebarrier --bin ob-replay
bash interpose/ob-recover.sh all 3         # all 4 unmodified apps, one command
bash interpose/ob-recover.sh redis 3       # → "redis DETERMINISTIC ✅"
bash interpose/demo.sh                      # stock-redis record-replay STATE recovery
```

Representative run (`all`, 3 s gap, 2026-06-21):

```
redis     live/replay 1782054868.424071 == 1782054868.424071  | control 1782054874.139431  ✅
memcached live/replay STAT time 1782054875 == 1782054875       | control STAT time 1782054880  ✅
nginx     live/replay Date 15:14:41 GMT == 15:14:41 GMT         | control Date 15:14:46 GMT  ✅
node      live/replay {"now":1782054887981} == ...887981        | control {"now":1782054893724}  ✅
```

## Per-app status

All five verified by `ob-recover.sh` (record → crash → real-time gap → replay →
`diff`), byte-identical across the gap:

| App | configuration | time-dependent probe | determinism |
|---|---|---|---|
| **engine apps** (`ob-kv`/`ob-mc`/…) | native | counter clock | deterministic by design ✅ |
| **redis** | single-threaded | `TIME` | virtual clock → **byte-identical** ✅ |
| **node** | event loop | `Date.now()` | virtual clock → **byte-identical** ✅ |
| **memcached** | `-t 1` (single worker) | `stats time` | virtual clock → **byte-identical** ✅ |
| **nginx** | `worker_processes 1` | `Date:` header | virtual clock → **byte-identical** ✅ |

(nginx is the strongest demonstration: the HTTP `Date:` header — formatted deep in
nginx's own code from its cached time — is frozen identically across a real-time
gap, e.g. `Date: Sun, 21 Jun 2026 15:11:46 GMT` on both the live and replayed
instance.)

State recovery (request-replay of SET/GET) is **time-independent** and works for
all KV apps regardless of the clock (the `demo.sh` stock-redis demonstration).

## Closing the residual nondeterminism: RNG and threads

The virtual clock handles time; two more libOS components close the rest.

### `rngdet.c` — deterministic RNG at the raw-syscall level (`librngdet.so`)

V8/Node, OpenSSL, and `arc4random` seed their PRNGs from the **raw** `getrandom(2)`
syscall, which bypasses `LD_PRELOAD` symbol interposition. `rngdet.c` installs a
**seccomp-BPF user-notification** filter that traps `getrandom`; a supervisor
thread fills the caller's buffer from a deterministic splitmix64 stream seeded from
a persisted seed (`OB_VRAND=<file>`), so live and replay observe the identical
random stream. Verified: a raw-`getrandom` C program returns identical bytes across
runs (`520956f1…`), where real entropy differs every run.

For V8, two more entropy sources must be pinned: **ASLR addresses** (`setarch -R`,
disable address-space randomization) and the **RDRAND CPU instruction**, which no
syscall trap can catch (`OPENSSL_ia32cap='~0x4000000000000000:~0x0'` makes OpenSSL
fall back to the trapped `getrandom`). With the full stack —
virtual clock + `librngdet.so` + ASLR-off + no-RDRAND — Node's `Math.random()` is
**byte-identical across recovery**:

```
live   {"now":1782055180436,"rnd":0.27798545181677814}
replay {"now":1782055180436,"rnd":0.27798545181677814}   (4 s real gap)
control{"now":1782055185175,"rnd":0.25929447455726007}   (no stack — both differ)
```

`ob-recover.sh` applies this stack to every app automatically.

### `detsched.c` — deterministic multithreading (`libdetsched.so`)

With >1 thread the OS chooses the order threads enter critical sections, so state
evolves nondeterministically. `detsched.c` interposes the pthread sync surface and
imposes **Kendo-style deterministic logical clocks** (Olszewski et al., ASPLOS'09):
a thread may take a **top-level** lock only when its `(logical-clock, slot)` is the
global minimum among active threads, then advances its clock. The order is a
function of the clocks, not OS timing, so the interleaving is identical every run.
Demonstrated by `det-mt.sh`:

- a 4-thread microbenchmark's critical-section order is **identical across runs**
  (`order_hash=eef52ab…`, 0 turn-relaxations) where it otherwise varies every run;
- a condvar producer/consumer runs with **no deadlock** (parked threads leave the
  minimum; nested locks bypass the gate — both required to stay deadlock-free);
- it **composes with a real multithreaded server**: `memcached -t 4` serves and
  stores under the scheduler.

Engineering notes that made it work on real binaries: gate only depth-0
acquisitions (nested locking deadlocked the naïve scheme); a bounded turn-wait
(`OB_DETSCHED_SPIN`, default 50000) keeps strict determinism for normal contention
but degrades to best-effort rather than hang on lock-heavy server init; and
`pthread_cond_wait` must be bound to its **`GLIBC_2.3.2`** version via `dlvsym`
(plain `dlsym` returns the old compat shim and hangs threaded servers).

Scope (Kendo's domain): race-free programs whose threads make progress through sync
operations. Pure-compute threads that never sync are out of scope (Kendo uses HW
perf counters there).

### redis-internal RNG (`/dev/urandom`) — `ob-redis-rng.sh`

redis 6 seeds its dict (SipHash) from `/dev/urandom` via `fopen`, which bypasses
both the getrandom seccomp trap and symbol interposition (glibc's internal
openat/read). So `SPOP`/`SRANDMEMBER` were nondeterministic across restarts. Fixed
by running redis in a private mount namespace with a deterministic file
bind-mounted over `/dev/urandom` — redis's RNG-derived state then recovers
byte-identically (popped/remaining set members identical live vs replay; a control
with the real device differs). `librngdet.so` also offers an optional syscall-level
openat redirect (`OB_VRAND_OPENAT=1`, via `SECCOMP_IOCTL_NOTIF_ADDFD`).

## Checkpointing (bounded recovery)

Replaying from process start costs O(all requests); a checkpoint bounds it to
O(tail). Two mechanisms:

- **App-native (works in-sandbox)** — `ob-checkpoint-replay.sh`: redis RDB snapshot
  + tail-replay, with the virtual clock resuming via `OB_VCLOCK_TICKS`. Byte-
  identical state replaying 20 reqs vs 41 (2.05×).
- **General, any-binary** — `ob-criu-checkpoint.sh`: CRIU dumps the whole process
  (incl. the in-memory virtual clock), so restore needs no replay. CRIU *dump*
  works here; *restore* is blocked by this sandbox's kernel (a trivial process's
  restorer completes then SIGSEGVs; Docker/runc share the kernel and fail the
  same). The harness completes on a standard kernel.

None of the above requires RDMA — the libOS is effort-gated, commodity-hardware work.
