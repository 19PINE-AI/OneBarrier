# OneBarrier — Status & Results

Single source of truth for *what is built* and *what is measured*. Every result
here is reproduced by a command; nothing is asserted without running it. Updated
as the autonomous research proceeds.

## Milestones (docs/PLAN.md §8)

| ID | Milestone | State |
|----|-----------|-------|
| M0 | Replication core: deterministic-replay KV state machine, durable ordered log, timestamp-T snapshot, exactly-once output suppression, crash recovery | **done** ✅ |
| M1 | Networked OneBarrier node over the live 1Pipe `ReliableHost` fabric; kill-and-recover demo | todo |
| M2 | RQ2 harness — output-commit latency decomposition + durability-tier sweep + serial ablation (the make-or-break measurement) | todo |
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

## Claims ledger (RQ → evidence)

| RQ | Claim | Evidence | State |
|----|-------|----------|-------|
| RQ4 | Exactly-once across recovery; post-recovery state ≡ serial reference | `onebarrier` unit tests + `ob-demo` | partial (micro) |
| RQ2 | FT marginal cost ≈ 0 over reliable-1Pipe baseline (in-fabric RDMA tier) | — | not yet |
| RQ1 | Steady-state overhead vs baselines | — | not yet |
| RQ3 | Recovery speed & replay catch-up condition | — | not yet |
| RQ5 | Cores vs SMR | — | not yet |
| RQ6 | Overhead flat vs scale | — | not yet |
| RQ7 | Contribution ablation (order-log off; LLFT/HyCoR head-to-head) | — | not yet |
| RQ8 | Boundary costs (quiesce, edge buffering, snapshot interval) | — | not yet |
