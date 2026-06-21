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

## Honest scope (the libOS's remaining work)

- **RNG**: `getrandom` via libc is virtualized; V8's `Math.random` seed comes via
  the raw `getrandom` syscall / a `/dev/urandom` read whose startup sequence is
  not byte-reproducible (the read-replay attempt destabilized node startup).
  Closing it needs **seccomp-BPF user-notification** to trap the syscall — the
  documented next piece (`docs/PAPER-PLAN.md` §2).
- **Multithreading**: memcached (default threads) and nginx (multi-worker) need
  single-worker config *or* a deterministic-scheduling layer (CoreDet/Crane). The
  share-nothing single-worker case is the supported scope; arbitrary
  multithreaded determinism is the frontier.
- **Process-state capture**: recovery currently replays from process start;
  checkpoint-based recovery (CRIU/libOS snapshot + tail replay) is future work
  (CRIU dump measured; restore was sandbox-blocked).

None of this requires RDMA — the libOS is effort-gated, commodity-hardware work.
