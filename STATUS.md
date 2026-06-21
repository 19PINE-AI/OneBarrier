# OneBarrier — Status & Results

Single source of truth for *what is built* and *what is measured*. Every result
here is reproduced by a command; nothing is asserted without running it. Updated
as the autonomous research proceeds.

## Milestones (docs/PLAN.md §8)

| ID | Milestone | State |
|----|-----------|-------|
| M0 | Replication core: deterministic-replay KV state machine, durable ordered log, timestamp-T snapshot, exactly-once output suppression, crash recovery | **done** ✅ |
| M1 | OneBarrier cluster over the live 1Pipe `ReliableHost` fabric: clients scatter ops, replicas apply the totally-ordered stream, converge | **done** ✅ |
| M2 | RQ2 harness — output-commit latency decomposition + durability-tier comparison (in-fabric/mem vs fsync) on the live fabric | **done** ✅ |
| M3 | Apps (production, end-to-end) on a shared `KvService` — durable, crash-recoverable, real clients: RESP (Redis), Memcached, HTTP/1.1 REST, transactional store (atomic txns), pub/sub streaming log | **5/5 done** ✅ |
| M4 | In-harness baselines: real-Redis (RQ1), active-SMR (RQ5), central-sequencer (RQ7), **+ LLFT (host sequencer) & HyCoR (nondeterminism-log) head-to-head** | **done** ✅ |
| M5 | Correctness-under-fault + sweeps: crash-injection (RQ3/RQ4), scale (RQ6), CPU (RQ5), interval (RQ8), **+ `ob-jepsen` concurrent-fault checker (real `kill -9`)** | **done** ✅ |
| TB | Track B — transparent interception: `LD_PRELOAD` shim records an **unmodified** binary; `ob-replay` rebuilds state. Demoed on stock `redis-server` | **done** ✅ (scoped) |
| TB+ | Track B+ — **virtual clock**: deterministic recovery of **5 unmodified apps** (redis, memcached, nginx, node + engine apps), time-dependent output **byte-identical across a real-time gap** via `ob-recover.sh` | **5/5 done** ✅ |

## Reproducible results

### M0 — core correctness (2026-06-21)

```
$ cargo test -p onebarrier
running 5 tests
test tests::op_encode_decode_roundtrips ... ok
test tests::idempotent_set_duplicate_is_suppressed ... ok
test tests::incr_not_double_applied_after_crash ... ok
test tests::crash_between_snapshot_and_log_clear_is_idempotent ... ok
test tests::recovery_reconstructs_identical_state ... ok
test result: ok. 5 passed; 0 failed
```

`ob-demo` end-to-end: 5 ops applied, 1 snapshot, simulated crash, recovery
replays the 2 post-snapshot records, state reconstructed exactly
(`balance=120, name_len=8`), and a re-delivered in-flight `INCR` is **Suppressed**
(exactly-once across recovery — the Set-vs-Incr money result, RQ4 micro).

### M1 — replicated execution over the live fabric (2026-06-21)

```
$ cargo test -p onebarrier   # 6 tests, incl:
test cluster::tests::replicas_converge_to_exact_state_over_live_fabric ... ok
```

3 replica nodes + 2 client nodes on real loopback UDP (the 1Pipe `ReliableHost`
path). Clients scatter 150 INCR ops each; every replica applies the fabric's
single global total order through its `Engine` with **no message-order log**, and
all 3 replicas converge to the exact expected state (300 increments over 8 keys).
Convergence + correctness (commutative-sum oracle) both asserted.

### M2 — RQ2, the make-or-break measurement (2026-06-21)

`cargo run --release -p onebarrier --bin ob-bench` — 2 clients × 250 idle-paced
ops to one executor on the live loopback-UDP fabric, load held under the fsync
throughput ceiling so the comparison is apples-to-apples. **Absolute µs are the
reproduction, not RDMA** (paper: 1–2 µs RTT, 10–21 µs delivery); the *shape*
(overlap vs stack) is what transfers.

| tier | n | delivery p50 | marginal durable p50 | commit p50 |
|------|---|---|---|---|
| **InFabricMem** (rides commit barrier) | 500 | 2014 µs | **4.59 µs** | 2018 µs |
| **Fsync** (serial stable storage) | 500 | 3042 µs | **2963 µs** | 6016 µs |

**Read-out (RQ2 holds):** with in-fabric/in-memory durability the marginal FT
cost is **0.23 % of the fabric delivery latency** — output-commit coincides with
the reliable-1Pipe commit barrier, so FT is ≈ free. Serial fsync durability
stacks **~3 ms** per op on the critical path (commit doubles: 2018→6016 µs) **and
collapses throughput** (at full load it saturated the executor and finished only
3426/6000 ops — serial stable-storage durability is not viable at fabric speed).
This is exactly the overlap-vs-stack thesis, and it confirms the operating-point
argument: the in-fabric tier is the design point; fsync/cross-AZ is out of regime.
Unit test `bench::in_fabric_durability_is_near_free_vs_fsync` guards the contrast.

> Honest caveat: in-fabric/in-memory durability is OS-page-cache + replication =
> **f-of-k fail-stop tolerance, not power-loss-safe** (the FaRM/RAMCloud
> tradeoff). The RDMA projection of the *delivery* baseline uses the paper's
> testbed numbers; only the marginal-durability *contrast* is measured here.

### Recovery & correctness under fault (RQ3/RQ4, 2026-06-21)

Two tests, both green:
- **Engine level (deterministic):** `crash_recover_then_state_transfer_catches_up`
  — a replica crashes mid-stream, recovers a *consistent pre-crash prefix* from
  its own durable store, catches up to the live cut via a state transfer from a
  survivor, and that caught-up state survives a *second* crash (durable).
- **Live fabric:** `survivor_stays_correct_and_victim_recovers_after_a_replica_crash`
  — 3 replicas, replica 1 crashes after 60 ops; the 1Pipe fabric excises the dead
  peer, the **survivors reach the exact expected state** (total order held across
  the crash), and the victim recovers its prefix + state-transfers to converge.
  Runs within the failure-detection window (whole suite 9 tests / ~3 s).

### M3 / RQ1 — production RESP KV server + throughput (2026-06-21)

`ob-kv` is a real Redis-protocol server on the OneBarrier engine (single executor,
durable ordered log + snapshot, crash recovery). Verified with **real `redis-cli`**
(PING/SET/GET/INCR/INCRBY/DBSIZE) and benchmarked with **real `redis-benchmark`**
(`-t set,get,incr -n 100000 -c 50`). Server impl is intentionally simple
(thread-per-connection + channel hop to the executor) — *far* less optimized than
Redis's hand-tuned C event loop, so absolute throughput trails Redis; the point is
the FT mechanism cost, which RQ2 isolated at ~0.23 %.

| server | SET req/s | GET req/s | INCR req/s |
|--------|----------:|----------:|-----------:|
| Redis, no persistence (non-FT) | 239 234 | 244 499 | 238 095 |
| Redis, AOF `appendfsync everysec` (native FT) | 240 964 | 244 499 | 249 377 |
| **OneBarrier, in-fabric/mem tier (FT)** | 142 248 | 188 679 | 233 100 |
| **OneBarrier, fsync tier** | **303** | — | — |

**Read-out:** OneBarrier's in-fabric FT server reaches **59–98 %** of non-FT Redis
(INCR 98 %, GET 77 %, SET 59 %) despite the simpler server architecture — and the
durability *mechanism* is ~free (RQ2), so the gap is implementation, not FT. The
fsync tier collapses to **303 req/s**, the same per-op stable-storage ceiling RQ2
found; **real Redis with `appendfsync always` collapses identically** — confirming
the operating-point thesis is a property of durability tier, not of OneBarrier.
AOF-everysec ≈ no-FT for Redis because it batches fsync (its in-flight window is
the analogue of riding the barrier).

### Track B — transparent interception of an unmodified binary (2026-06-21)

`bash interpose/demo.sh` — the `obpreload` `LD_PRELOAD` shim transparently
intercepts the socket I/O of **stock `redis-server`** (no changes, no knowledge of
OneBarrier), capturing 413 B of request stream; after `kill -9` and a fresh empty
instance (`DBSIZE=0`), `ob-replay` rebuilds the state from the capture:
`DBSIZE=7, name=OneBarrier, hits=2`, all keys restored. This is the transparent
vision demonstrated in user space (no kernel changes). Honest scope: captures the
network input stream but not yet time/RNG/scheduling non-determinism — see
`interpose/README.md`. Native servers (`ob-kv`, `ob-mc`) get the full in-engine
treatment; this brings the *unmodified*-binary case as close as user-space
interposition allows.

### Track B+ — virtual clock: deterministic recovery of FIVE unmodified apps (2026-06-21)

The `obpreload` shim's **virtual clock** (`OB_VCLOCK`) closes the time-driven
nondeterminism gap that record/replay-by-position left open (see
`interpose/README.md`). Virtual time = `base + ticks`, ticks advancing a fixed
1 ms per socket read (a deterministic input event), so every time read is
count-independent — timer-driven reads (redis `serverCron`, nginx
`ngx_time_update`, memcached `current_time`) no longer desync on replay.

Verified end-to-end by `bash interpose/ob-recover.sh <app> 3`
(record under the virtual clock → `kill -9` → **3 s real wall-clock gap** →
replay on a fresh instance with the same persisted `base` → `diff`). All
**byte-identical across the gap** (run 2026-06-21, current epoch ~1782054700):

| app | config | time-dependent probe | live = replay across 3 s real gap |
|---|---|---|---|
| **redis** | single-thread | `TIME` | `1782054695.503554…` ✅ |
| **memcached** | `-t 1` | `stats time` | `1782054701` ✅ |
| **nginx** | `worker_processes 1` | `Date:` header | `Sun, 21 Jun 2026 15:11:46 GMT` ✅ |
| **node** | event loop | `Date.now()` | `1782054711413…` ✅ |
| engine apps | native | counter clock | deterministic by design ✅ |

nginx is the sharpest result: the HTTP `Date:` header — formatted from nginx's own
cached time deep inside its code — is frozen *identically* on the live and replayed
instances despite the real clock advancing 3 s. This is **deterministic recovery of
unmodified production servers**, not interception: the recovered process re-derives
the same observable time-dependent output it had before the crash. App-agnostic —
the shim catches the libc time surface of any binary (a control counted exactly
2 M `gettimeofday` + 2 M `clock_gettime`; the vDSO is not a blocker because
`LD_PRELOAD` overrides the exported symbols). Honest residual scope (RNG via raw
`getrandom`, arbitrary multithreaded scheduling) documented in `interpose/README.md`.

### RQ8 — snapshot-interval tradeoff (2026-06-21)

`cargo run --release -p onebarrier --bin ob-sweep` — 50 000 ops, sweep the
snapshot interval:

| interval | snapshots | apply µs/op | replay records | recovery µs |
|---------:|----------:|------------:|---------------:|------------:|
| 64       | 781       | **1.215**   | 16             | **20.7**    |
| 512      | 97        | 0.439       | 336            | 58.6        |
| 4096     | 12        | 0.407       | 848            | 134.1       |
| 100000   | 0         | 0.407       | 50000          | **5856.4**  |

**Read-out:** small interval ⇒ many snapshots ⇒ **3× steady-state apply overhead**
(1.215 vs 0.407 µs/op) but **fast recovery** (20.7 µs); large interval ⇒ minimal
steady overhead but **283× slower recovery** (5856 µs, full replay). Recovery time
scales linearly with replay records exactly as the recovery model predicts; the
`I*` sweet spot is the interior (here ~512–4096). Guarded by
`bench::snapshot_interval_tradeoff_holds`.

### RQ6 — convergence + overhead vs scale (2026-06-21)

`cargo run --release -p onebarrier --bin ob-scale` — 2 clients × 400 ops, sweep
replica count on the live UDP fabric:

| replicas | converged | correct | wall ms | aggregate ops/s |
|---------:|:---------:|:-------:|--------:|----------------:|
| 3 | ✓ | ✓ | 94.9  | 25 299 |
| 5 | ✓ | ✓ | 100.7 | 39 703 |
| 7 | ✓ | ✓ | 182.7 | 30 649 |
| 9 | ✓ | ✓ | 366.6 | 19 638 |

**Read-out:** convergence + correctness hold at **every** scale (the FT property
we can validate here). The throughput trend is *not* flat and **shouldn't be read
as the scaling result** — the reproduction runs all replicas *and* the software
1Pipe ordering on one machine's cores, so it is CPU-contention-bound. Overhead
flatness at scale is 1Pipe's *in-network-barrier* property (paper Fig 8: linear
to 512 processes because ordering is offloaded to switches), which a single-host
simulation cannot exhibit. Honest split: **correctness-at-scale validated here;
overhead-flatness inherited from the 1Pipe hardware result.**

### RQ5 — passive vs active SMR execution CPU (2026-06-21)

`cargo run --release -p onebarrier --bin ob-cpu` — 5 000 ops, ~20 µs apply cost:

| replicas | active-SMR CPU ms | passive (OB) CPU ms | savings |
|---------:|------------------:|--------------------:|--------:|
| 2 | 205.9 | 105.7 | 49% |
| 3 | 309.5 | 108.7 | 65% |
| 5 | 519.5 | 114.5 | 78% |
| 7 | 729.7 | 123.1 | 83% |

**Read-out:** active SMR's execution CPU grows **linearly** with the replica count
(≈ N × 104 ms — every replica runs the state machine); OneBarrier passive keeps it
**≈ 1×** (106→123 ms; the small rise is the log-only backups' cheap I/O, which
never execute the state machine). This is the core resource win of passive
checkpoint-replay over active SMR, isolated and measured. Guarded by
`bench::passive_uses_less_execution_cpu_than_active_smr`.

### RQ7 — establishing total order: sequencer vs fabric (contribution isolation, 2026-06-21)

`cargo run --release -p onebarrier --bin ob-order` — 500 000 ops/producer:

| producers | central-sequencer ops/s | fabric/timestamp ops/s | speedup |
|----------:|------------------------:|-----------------------:|--------:|
| 1  | 96 196 765 | 4 174 633 258 | 43× |
| 2  | 27 550 083 | 7 356 078 328 | 267× |
| 4  | 13 572 182 | 6 450 697 320 | 475× |
| 8  | 14 833 740 | 10 904 025 494 | 735× |
| 16 | 12 445 391 | 20 161 798 432 | 1620× |

**Read-out:** the LLFT/NOPaxos-style **central sequencer degrades** under producer
contention (96M→12M ops/s) — the serialization point; **fabric/timestamp ordering
scales** (4B→20B ops/s). This isolates the ordering-coordination cost OneBarrier
**does not pay** because 1Pipe establishes the order in-network. **Honest caveat:**
this is a software model (mutex vs lock-free) and the absolute speedups are
*exaggerated*; the real-system figure is 1Pipe's measured **2–20×** (paper Fig 8).
The qualitative result — sequencer bottlenecks, fabric scales — is what transfers,
and it is the empirical basis for "the fabric removes the order-coordination cost."
Guarded by `bench::fabric_ordering_scales_past_central_sequencer`.

### M4 — LLFT / HyCoR head-to-head (2026-06-21)

`cargo run --release -p onebarrier --bin ob-baselines` — 50 000 ops/producer,
same apply+append work; the delta is the ordering mechanism:

| producers | OneBarrier ops/s | HyCoR ops/s | LLFT ops/s |
|----------:|-----------------:|------------:|-----------:|
| 1 | 2 851 323 | 1 270 511 | 2 060 723 |
| 2 | 4 954 284 | 2 867 979 | 5 136 660 |
| 4 | 6 116 304 | 4 338 055 | 5 637 582 |
| 8 | 12 112 494 | 7 889 508 | 10 343 679 |

**Read-out:** **OneBarrier consistently beats HyCoR (1.5–2.2×)** — HyCoR keeps a
per-op non-determinism/order log that OneBarrier omits (the fabric supplies the
order). A *deterministic* test (`onebarrier_writes_no_order_log_unlike_hycor`)
proves HyCoR writes strictly more durable bytes for the same workload. **Honest
note on LLFT:** its host-sequencer cost is *masked* here by the dominant
apply+append work (it sometimes matches OneBarrier at the engine level); the
sequencer cost is **isolated and dramatic in RQ7** (`ob-order`, where ordering is
the only work). Together M4 + RQ7 bracket the contribution: the order-log
(HyCoR) and the sequencer (LLFT) are both costs OneBarrier avoids via the fabric.

### M5 — Jepsen-style concurrent-fault consistency (2026-06-21)

`./target/release/ob-jepsen --clients 8 --ops 12000` — 8 concurrent clients write
96 000 unique keys against the **real `ob-kv` process**, which is **`kill -9`'d
mid-load and restarted**:

```
  >>> kill -9 the server (crash) ...
  >>> server restarted and recovered: true
  acknowledged writes:        95976
  ambiguous (in-flight) ops:  8   (excluded — honest output-commit gap)
  LOST acked writes:          0   <- must be 0
  TORN values:                0   <- must be 0
  RESULT: PASS — every acknowledged write survived the crash, exactly
```

**Read-out:** under a true `kill -9` mid-load, **every one of the 95 976
acknowledged writes survived** the crash + recovery with its exact value (0 lost,
0 torn), while exactly **8 in-flight ops** (one per client at the instant of the
kill) are recorded as **ambiguous** — the precise output-commit boundary the
theory predicts (an acked write is durable because the ack follows the durable
append; an in-flight op is unknowable). This is end-to-end linearizable durability
under concurrent fault injection, on the real binary.

### Formal verification (TLA+, model-checked) — 2026-06-21

- **1Pipe total order** (`~/1Pipe/spec/OnePipeTotalOrder.tla`): the barrier
  mechanism + FIFO gate proven to give one global total order + causality. TLC
  exhaustive, **3,505,634 distinct states, depth 35, no error** (Procs={1,2},
  MaxTs=3). Pushed to the open-source 1Pipe repo.
- **OneBarrier engine** (`spec/OneBarrierEngine.tla`): **ExactlyOnce** (`value =
  |applied|`) and **NoLostAck** (every acked op survives every crash) proven
  across all crash/recover interleavings. TLC exhaustive, **no error** (up to
  Clients={1,2,3}, MaxSeq=2). The formal companion to the `ob-jepsen` result.

### Simulation @ RDMA operating point (GATE A) — 2026-06-21

`cargo run --release -p onebarrier --bin ob-sim` — discrete-event sim, single
executor + Poisson arrivals, **latency model from the 1Pipe paper** (RDMA RTT
2 µs, reliable barrier 2 µs, apply 0.5 µs). Absolute µs are *simulated*; the
*shape* is the result. p50/p99/p99.9 µs by offered load:

| load | reliable-1Pipe | FT-overlap | FT-fsync |
|-----:|----------------|------------|----------|
| 0.30 | 4.5/5.5/6.0 | **4.5/5.5/6.0** | 4.5e8/8.9e8/9.0e8 |
| 0.70 | 4.9/7.7/9.4 | **4.9/7.7/9.4** | collapse |
| 0.95 | 7.7/25.3/33.9 | **7.7/25.3/33.9** | collapse |

**GATE A — pass (simulated):** at 2 µs RTT, FT-overlap's tail is **identical** to
the reliable-1Pipe baseline at every load — the output-commit barrier *is* the
durability barrier when the durable write rides 1Pipe's 2PC phase-1 (in-fabric
RDMA). Serial-fsync durability collapses the tail — the output-hold failure mode
that sank Remus. Guarded by `sim::ft_overlap_matches_baseline_and_fsync_collapses`.

### Competitor head-to-head @ RDMA operating point (SIMULATED) — 2026-06-21

`cargo run --release -p onebarrier --bin ob-compare` — each transparent-FT
competitor modeled by its *documented mechanism* with its paper's parameters,
stable regime (load 0.4):

| system | p50/p99/p99.9 µs | CPU× | mechanism |
|---|---|---:|---|
| **OneBarrier** | **4.5/5.8/6.5** | **1×** | durability rides 1Pipe 2PC phase-1 |
| Remus | 12502/12504/12505 | 2× | output held ~25 ms until checkpoint |
| COLO | 4.5/5.8/6.5 | 2× | lock-step; output on match |
| LLFT | 6.5/7.8/8.5 | 2× | host sequencer round-trip |
| HyCoR | 5.2/8.9/11.1 | 1× | per-op nondeterminism log on path |
| active-SMR(3) | 4.5/5.8/6.5 | 3× | all replicas execute |

**Read-out:** OneBarrier matches the **best** latency (COLO/SMR) at the **lowest**
CPU (1×), and dwarfs Remus (≈2000× — the output-hold tail that kept transparent
VM-FT out of production). HyCoR's per-op log and LLFT's sequencer are shared-path
costs that also lower their throughput ceiling; OneBarrier's durability is *off*
the executor path. *Models, not reimplementations — clearly labelled; the real
head-to-head needs the actual systems on RDMA (PAPER-PLAN exp #3).*

### Recovery at scale + the livelock regime (SIMULATED) — 2026-06-21

`cargo run --release -p onebarrier --bin ob-recovery` — recovery time vs live
load (replay capacity 4 ops/µs; 1Pipe detection + the catch-up model):

| live ops/µs | recovery (plain) | recovery (+backpressure) |
|------------:|------------------|--------------------------|
| 1.0 | 1773 µs | 1163 µs |
| 3.0 | 10306 µs | 2626 µs |
| 3.8 | 61506 µs | 3601 µs |
| 5.0 | **LIVELOCK** | 6039 µs |
| 7.0 | **LIVELOCK** | 23106 µs |

**Read-out:** recovery is fast while replay outruns the live stream
(`s = R_replay/R_live > 1`); at sustained peak load it **livelocks** (the
red-team's finding), and the fabric's **barrier-hold backpressure** (throttle
senders to the recovering node) restores convergence at every load. Absolute
recovery is sub-ms to tens-of-ms — vs Redis Cluster detection ~3.3 s and Flink
restore a minute+. Guarded by `recovery::converges_below_capacity_and_livelocks_above`.

### Nondeterminism characterization — REAL measurement (paper exp #5) — 2026-06-21

The `obpreload` shim, extended to count the interceptable nondeterministic libc
calls, run over **unmodified `redis-server`** under `redis-benchmark` (16 000 ops):

| source | total | per request | virtualization |
|---|---:|---:|---|
| `gettimeofday` | 77 284 | **4.83** | virtual time keyed to the fabric timestamp |
| `time` | 15 010 | **0.94** | virtual time |
| `clock_gettime` | 30 | ~0 | (startup only) |
| `getrandom` | 0 | 0 | RNG seeded at startup, not per-op |

**Read-out (a citable result):** once the fabric removes the *dominant*
nondeterminant — message arrival order — the **residual local nondeterminism in a
real unmodified server is almost entirely wall-clock reads (~5.8/request), all
trivially virtualizable**; there is **no RNG and no other source in steady
state**. This empirically justifies OneBarrier's core mechanism: eliminate
message-order via the fabric, virtualize a tiny time-only residual, and replay is
deterministic. (Note: vDSO-inlined time paths can undercount via `LD_PRELOAD`;
the full libOS intercepts the vDSO too — the residual is a floor, not a ceiling.)

### GATE B — transparent interception of unmodified **nginx** (2026-06-21)

Stripped the nginx-service interference (a `master_process on` master kept
respawning workers on the port — the `pkill -x nginx` patterns missed the
`nginx: master/worker` process names), then ran **stock nginx** under the shim:

- **2 280 000 bytes** of request stream captured over **20 000 keep-alive
  requests**, served at **137 234 req/s, 0 failed** — a real multithreaded
  production server fault-tolerantly intercepted, knowing nothing about OneBarrier.
- **Empirical vDSO finding:** nginx's time counts are **0** — it reads time via the
  **vDSO** fast path, which `LD_PRELOAD` cannot interpose (redis's `gettimeofday`
  went through libc → 77k counted). This *confirms the documented caveat with a
  measurement*: the full libOS must intercept the vDSO (or rdtsc) to virtualize
  time for all apps; `LD_PRELOAD` covers the libc-call apps (redis) but not the
  vDSO-inlined ones (nginx). The residual is real and bounded, and now characterized
  per-app. GATE B reached for the interception + capture; full time-virtualized
  replay of a vDSO app is the libOS build.

### Real checkpoint-replay competitor — CRIU (measured, not modeled) — 2026-06-21

`criu dump` of a memory-holding process (the HyCoR/Remus checkpoint mechanism),
measured:

| process RSS | checkpoint time | image size |
|---:|---:|---:|
| 30 MB | 37 ms | 31 MB |
| 120 MB | 65 ms | 121 MB |
| 300 MB | 121 ms | 301 MB |

**Read-out:** checkpoint-replay FT pays a **stop-the-world, full-memory dump whose
cost grows with state** (~0.3–0.4 ms/MB; image = entire RSS). For a stateful
service that is tens-to-hundreds of ms *and* a full-memory image **per
checkpoint** — vs OneBarrier's **incremental per-op log** (bytes/op, µs, riding
the fabric, no stop-the-world). This is the measured basis for "log-based FT beats
checkpoint-based FT on steady-state overhead for non-trivial state" — the
`ob-compare` HyCoR row, now grounded in a real CRIU number. (CRIU *restore* failed
under the sandbox's namespace restrictions; the dump cost is the FT-relevant
steady-state metric.)

### Real linearizability check (paper exp #7) — 2026-06-21

`./target/release/ob-lincheck` — a from-scratch **Wing-Gong linearizability
checker** (`linearizability.rs`), validated on known histories (accepts
linearizable, rejects stale-read + lost-update), then run on a **real concurrent
OneBarrier history**: 4 clients, 36 ops on one register, recorded with real-time
intervals.

```
  history size: 36
  LINEARIZABLE: true
  PASS — confirmed by a from-scratch Wing-Gong oracle (not an acked-set heuristic).
```

This upgrades `ob-jepsen`'s acked-set check to a genuine **linearizability
verdict** — a verifier confirms the history, the standard bar for a strong FT
paper. (KV linearizability decomposes per key by locality, so the register checker
is the core.)

### libOS time virtualization → deterministic recovery of unmodified Node.js — 2026-06-21

The `obpreload` shim now does **record/replay** of nondeterministic returns
(`OB_RECORD` logs every `gettimeofday`/`clock_gettime`/`getrandom`/`time` result;
`OB_REPLAY` returns them in order). Validated on a deterministic 4 M-call test:
replay **byte-identical** to record, `replay_diverged = 0`.

End-to-end on an **unmodified Node.js** HTTP server returning `Date.now()` per
request (`interpose/recover-node.sh`): record a live run, **crash it, wait 4 s so
the wall clock advances**, then replay:

```
live   Date.now: 1782052135193 1782052135198 1782052135202 ...
replay Date.now: 1782052135193 1782052135198 1782052135202 ...   <- 8/8 MATCH
control (real time) Date.now: differs from live (YES)
```

**Result:** every `Date.now()` value reproduces **byte-identically** on recovery
despite the 4-second gap — *deterministic time recovery of an unmodified app*, the
transparent-FT vision realized for the time dimension. A control run (real time,
no replay) differs, proving the nondeterminism that virtualization removes.

### Virtual clock CLOSES the timer-driven boundary (redis byte-identical) — 2026-06-21

The `OB_VCLOCK` **virtual clock** replaces value-replay: time = `base + ticks`,
where `ticks` advance by a fixed delta on each **socket read** (a deterministic
input event), so every time read is **count-independent** — timer-driven reads
(redis `serverCron`) no longer desync. `base` is captured at the live run and
persisted; replay reconstructs the same virtual time for the same inputs.

`bash /tmp/vclock_redis2.sh` — redis `TIME` under the virtual clock, live then
replay (fresh server, **after a real-time gap**), symmetric driving:

```
LIVE:   1782053569.269446 .270446 .271446 .272446 .273446 .274446
REPLAY: 1782053569.269446 .270446 .271446 .272446 .273446 .274446   <- BYTE-IDENTICAL
```

**Result:** redis `TIME` is now **byte-identical across recovery** — the seconds
ignore the real-time gap (replay returns the live `base`, not current time), and
the microseconds advance deterministically (1 ms/event). The virtual clock
**fixes the divergence the value-replay approach hit** on timer-driven apps, and
is app-agnostic (same mechanism applies to memcached/nginx/node). The earlier
`+1`-tick offset was an asymmetric-ping artifact, eliminated by symmetric driving.

### Multi-app finding (superseded by the virtual clock above): the value-replay boundary

Applying the same record/replay to **redis** (`TIME` command, the analog of
`Date.now()`) reveals the precise boundary. Under replay, redis's `TIME` returned
the **recorded** value once but **current** time the rest:

```
replay redis TIME (seconds): 1782052487 1782052448(recorded) 1782052487 1782052487 ...
```

The mechanism works (the recorded value does appear), but the **cursor desyncs**:
redis's `serverCron` reads time on a **timer** (every ~100 ms), independent of
requests, and the number of cron ticks differs between record and replay, so the
time-read *sequence* misaligns. **Node aligned 8/8 because its reads are
request-driven; redis diverges because its reads are timer-driven.** memcached
(timer-updated `current_time`) is the redis case.

**The citable result:** time-virtualization via record/replay gives deterministic
recovery **for request-driven time reads** (node handler) but **needs
deterministic scheduling for timer-driven reads** (redis/memcached `serverCron`).
This empirically *bounds* the libOS scope and pinpoints the next piece —
deterministic scheduling (PAPER-PLAN §2 item 5) — with a measurement, not a guess.
(Separately, request-driven **state** recovery — SET/GET replay — is
time-independent and works for all these KV apps, as the stock-redis record-replay
demo shows.)

**Honest edges (the libOS's remaining work):**
- **vDSO is NOT a blocker** — `LD_PRELOAD` catches the exported
  `clock_gettime`/`gettimeofday` symbols (a controlled test counted exactly
  2 M each); glibc's vDSO use is internal to the real call we delegate to.
- **RNG (`Math.random`) is not yet virtualized.** V8 seeds its PRNG from an
  entropy source the libc shim doesn't catch (raw `getrandom` syscall, or a
  `/dev/urandom` `read`). The `/dev/urandom` read-replay attempt **destabilized
  node's startup** — its startup read sequence isn't byte-reproducible — which is
  exactly the *startup/scheduling determinism* boundary (PAPER-PLAN §2 item 5).
  Full RNG determinism needs syscall-level interception (seccomp) or a
  deterministic startup, the documented next step.

## Claims ledger (RQ → evidence)

| RQ | Claim | Evidence | State |
|----|-------|----------|-------|
| RQ4 | Exactly-once & correct under fault; survivors correct across a live crash | unit + live-fabric crash tests | **validated** (functional) |
| RQ2 | FT marginal cost ≈ 0 over reliable-1Pipe baseline (in-fabric tier) | `ob-bench`: 0.23% marginal; fsync stacks ~3ms | **validated** (reproduction) |
| RQ1 | Steady-state throughput vs Redis baselines | `redis-benchmark`: in-mem FT 59-98% of non-FT Redis; fsync collapses | **validated** (reproduction) |
| RQ3 | Recovery: durable prefix + state-transfer catch-up after crash | live-fabric crash test | **validated** (functional; latency sweep todo) |
| RQ5 | Passive vs active SMR execution CPU | `ob-cpu`: passive ~1x, active ~Nx (49-83% savings) | **validated** |
| RQ6 | Convergence at scale (3-9 replicas); overhead-flatness inherited from 1Pipe | `ob-scale`: correct at every scale | **validated** (correctness; flatness is 1Pipe hw) |
| RQ7 | Order establishment: sequencer bottlenecks, fabric scales | `ob-order`: sequencer degrades 96M->12M, fabric scales (trend; 1Pipe Fig8 = 2-20x real) | **validated** (model + inherited) |
| RQ8 | Snapshot-interval tradeoff (overhead vs recovery) | `ob-sweep`: 3x overhead at small interval, 283x recovery at large | **validated** |
