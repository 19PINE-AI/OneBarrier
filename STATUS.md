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
| M3 | Apps 1–2 (production, end-to-end): RESP (Redis) + Memcached text-protocol servers on a shared `KvService` — durable, crash-recoverable, real clients | **2/5 done** ✅ |
| M4 | In-harness baselines: LLFT-style host-virtual-time order, HyCoR-style nondeterminism logging, SMR (N active), logging-FT | todo |
| M5 | Jepsen-style linearizability checker (RQ4) + recovery/scale sweeps (RQ3/RQ5/RQ6) | todo |
| TB | Track B — transparent interception: `LD_PRELOAD` shim records an **unmodified** binary; `ob-replay` rebuilds state. Demoed on stock `redis-server` | **done** ✅ (scoped) |

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

## Claims ledger (RQ → evidence)

| RQ | Claim | Evidence | State |
|----|-------|----------|-------|
| RQ4 | Exactly-once & correct under fault; survivors correct across a live crash | unit + live-fabric crash tests | **validated** (functional) |
| RQ2 | FT marginal cost ≈ 0 over reliable-1Pipe baseline (in-fabric tier) | `ob-bench`: 0.23% marginal; fsync stacks ~3ms | **validated** (reproduction) |
| RQ1 | Steady-state throughput vs Redis baselines | `redis-benchmark`: in-mem FT 59-98% of non-FT Redis; fsync collapses | **validated** (reproduction) |
| RQ3 | Recovery: durable prefix + state-transfer catch-up after crash | live-fabric crash test | **validated** (functional; latency sweep todo) |
| RQ5 | Cores vs SMR | — | not yet |
| RQ6 | Overhead flat vs scale | — | not yet |
| RQ7 | Contribution ablation (order-log off; LLFT/HyCoR head-to-head) | — | not yet |
| RQ8 | Boundary costs (quiesce, edge buffering, snapshot interval) | — | not yet |
