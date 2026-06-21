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
| M3 | Application suite (production-grade, end-to-end): Redis/Memcached/Nginx/Node/SQLite-class on the libOS socket shim | todo |
| M4 | In-harness baselines: LLFT-style host-virtual-time order, HyCoR-style nondeterminism logging, SMR (N active), logging-FT | todo |
| M5 | Jepsen-style linearizability checker (RQ4) + recovery/scale sweeps (RQ3/RQ5/RQ6) | todo |

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

## Claims ledger (RQ → evidence)

| RQ | Claim | Evidence | State |
|----|-------|----------|-------|
| RQ4 | Exactly-once & correct under fault; survivors correct across a live crash | unit + live-fabric crash tests | **validated** (functional) |
| RQ2 | FT marginal cost ≈ 0 over reliable-1Pipe baseline (in-fabric tier) | `ob-bench`: 0.23% marginal; fsync stacks ~3ms | **validated** (reproduction) |
| RQ1 | Steady-state overhead vs baselines | — | not yet |
| RQ3 | Recovery: durable prefix + state-transfer catch-up after crash | live-fabric crash test | **validated** (functional; latency sweep todo) |
| RQ5 | Cores vs SMR | — | not yet |
| RQ6 | Overhead flat vs scale | — | not yet |
| RQ7 | Contribution ablation (order-log off; LLFT/HyCoR head-to-head) | — | not yet |
| RQ8 | Boundary costs (quiesce, edge buffering, snapshot interval) | — | not yet |
