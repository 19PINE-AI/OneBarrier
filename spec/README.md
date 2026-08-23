# OneBarrier — formal specification (TLA+)

A machine-checked TLA+ specification of the OneBarrier engine's two correctness
properties, verified across **arbitrary crash / recover interleavings** with TLC.
This is the formal companion to the empirical `ob-jepsen` result (0 lost / 0 torn
under `kill -9`) — what the test checks on the real binary, the spec **proves**
over the whole state space.

## What is specified

`OneBarrierEngine.tla` models the engine of `docs/research/PLAN.md §4` faithfully:

- a per-client **high-water mark** `hw` (the exactly-once dedup key);
- a **durable op-log** `durLog` since the last snapshot, and a **durable snapshot**
  (`snapVal`, `snapHW`, `snapApplied`);
- `Deliver(op)` — the fabric may **re-deliver** (at-least-once); a new op
  (`seq > hw`) is applied once, logged, and acknowledged; a duplicate is
  **suppressed** (no apply, no re-emit);
- `Crash` — **all volatile state is lost**; only the durable snapshot + log
  survive;
- `Recover` — restore the snapshot, then **replay the log with the same dedup**
  (the `Replay` fold) — the exact recovery algorithm.

`acked` is a history (ghost) variable: the set of ops a client was told
succeeded. It is **never rolled back**, so the invariants can quantify over
"what was promised" vs "what survives."

## Invariants proven (TLC, exhaustive over the bounded model)

| Invariant | Meaning |
|---|---|
| `ExactlyOnce` | `value = |applied|` always — each op is applied **at most once** to the committed state, despite re-delivery and recovery replay (a faulty replay that double-counted would violate this) |
| `NoLostAck` | every **acknowledged** op is reflected in the live state across **every** crash + recovery — durable linearizability (the `ob-jepsen` property) |
| `AckedDurable` | an op is acknowledged only after it is **durable** (in the snapshot or log) — the output-commit precondition |
| `SnapshotConsistent` | the durable snapshot is itself consistent (`snapVal = |snapApplied|`) |
| `TypeOK` | type correctness |

## Result

```
$ java -cp tla2tools.jar tlc2.TLC -deadlock -workers 4 \
       -config OneBarrierEngine.cfg OneBarrierEngine.tla
Model checking completed. No error has been found.
```

Verified instances (all exhaustive, no error):

| Clients | MaxSeq | distinct states |
|--------:|-------:|----------------:|
| {1,2}   | 2      | 106             |
| {1,2}   | 3      | 452             |
| {1,2,3} | 2      | 1 608           |

The state spaces are small because the dedup makes re-deliveries **no-ops** (the
point of exactly-once) — what matters is that crash/recover is interleaved at
*every* reachable point and the invariants never break.

## Why it holds (the written proof the model checks)

**ExactlyOnce.** Every `Deliver` either (a) applies a new op — `value`+1 and
`applied`∪{op}, keeping `value = |applied|` — or (b) suppresses a duplicate
(`seq ≤ hw`), changing neither. `Recover` recomputes `value`/`applied` by the same
dedup fold from the durable snapshot+log, which contain each committed op once;
re-applying an op already covered by `hw` is suppressed. So `value = |applied|` is
inductive. ∎

**NoLostAck.** An op is added to `acked` only on a new `Deliver`, which in the same
step appends it to `durLog` (or it is already in `snapApplied` after a snapshot).
Thus `acked ⊆ snapApplied ∪ Range(durLog)` (= `AckedDurable`) is inductive. A
`Crash` clears volatile state but preserves the durable snapshot+log; `Recover`
sets `applied = snapApplied ∪ {ops in durLog}` ⊇ `acked`. So `NoLostAck` holds in
every up state. ∎

TLC discharges both exhaustively across all crash/recover interleavings — no
hand-waving about the inductive step.

## Scope (honest)

This models the **engine** (dedup + durable log + snapshot + crash-recovery), the
layer where exactly-once and durable linearizability live. The **total order** the
engine consumes is 1Pipe's, proven separately in
[`OnePipeTotalOrder.tla`](https://github.com/bojieli/1Pipe/blob/01b307861bc608f758b9297147688b84f90580c5/spec/OnePipeTotalOrder.tla).
Composed: 1Pipe gives a single total order; OneBarrier
applies it exactly-once and durably. The output-commit boundary to *non-cooperating*
peers (the `ob-jepsen` "ambiguous in-flight ops") is the documented impossibility
edge, not modeled as recoverable.

## Reproduce

```bash
# needs Java 11+ and tla2tools.jar (https://github.com/tlaplus/tlaplus/releases)
java -cp tla2tools.jar tlc2.TLC -deadlock -workers 4 \
     -config OneBarrierEngine.cfg OneBarrierEngine.tla
```
