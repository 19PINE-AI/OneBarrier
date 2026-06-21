-------------------------- MODULE OneBarrierEngine --------------------------
(***************************************************************************)
(* A formal TLA+ specification of the OneBarrier engine's two correctness   *)
(* properties, model-checked across arbitrary CRASH / RECOVER interleavings: *)
(*                                                                         *)
(*   ExactlyOnce — the deterministic-replay engine applies each op at most  *)
(*                 once to the committed state, even though the fabric may   *)
(*                 re-deliver (at-least-once) and recovery replays the log.  *)
(*                 Modeled as an INCR counter: `value` must always equal the *)
(*                 number of DISTINCT applied ops (no double-count, no skip).*)
(*                                                                         *)
(*   NoLostAck   — every op whose output was ACKNOWLEDGED on the live path   *)
(*                 survives every crash + recovery (durable linearizability).*)
(*                 This is the property the ob-jepsen test checked           *)
(*                 empirically (0 lost / 0 torn under kill -9); here it is    *)
(*                 proven by exhaustive model checking.                      *)
(*                                                                         *)
(* Mechanism modeled faithfully: a per-client high-water mark `hw` (the      *)
(* exactly-once dedup key), a durable op-log `durLog` since the last         *)
(* snapshot, a durable snapshot (`snapVal`,`snapHW`,`snapApplied`), and a    *)
(* recovery that restores the snapshot then REPLAYS the log with the SAME    *)
(* dedup (the `Replay` fold).  A crash loses all volatile state; only the    *)
(* durable snapshot + log survive.  `acked` is a history (ghost) variable —  *)
(* the set of ops a client was told succeeded — and is never rolled back.    *)
(***************************************************************************)
EXTENDS Naturals, Sequences, FiniteSets

CONSTANTS Clients,   \* set of client ids
          MaxSeq     \* per-client sequence-number bound (keeps the model finite)

ASSUME MaxSeq \in Nat /\ MaxSeq >= 1

Op == [client : Clients, seq : 1..MaxSeq]

Range(s) == { s[i] : i \in DOMAIN s }

VARIABLES
  value,       \* live INCR counter (volatile)
  hw,          \* hw[c]: highest seq applied for client c (volatile dedup key)
  applied,     \* ghost: set of op-ids reflected in `value` (value = |applied|)
  acked,       \* ghost history: ops acknowledged to a client (never rolled back)
  durLog,      \* durable: ops logged since the last snapshot (survives crash)
  snapVal,     \* durable snapshot: counter value at the cut
  snapHW,      \* durable snapshot: hw map at the cut
  snapApplied, \* durable snapshot: applied set at the cut
  crashed      \* TRUE between a Crash and its Recover

vars == <<value, hw, applied, acked, durLog, snapVal, snapHW, snapApplied, crashed>>

TypeOK ==
  /\ value \in 0..(Cardinality(Clients) * MaxSeq)
  /\ hw \in [Clients -> 0..MaxSeq]
  /\ applied \subseteq Op
  /\ acked \subseteq Op
  /\ durLog \in Seq(Op)
  /\ snapVal \in 0..(Cardinality(Clients) * MaxSeq)
  /\ snapHW \in [Clients -> 0..MaxSeq]
  /\ snapApplied \subseteq Op
  /\ crashed \in BOOLEAN

Init ==
  /\ value = 0
  /\ hw = [c \in Clients |-> 0]
  /\ applied = {}
  /\ acked = {}
  /\ durLog = << >>
  /\ snapVal = 0
  /\ snapHW = [c \in Clients |-> 0]
  /\ snapApplied = {}
  /\ crashed = FALSE

\* The fabric delivers an op (totally ordered, but possibly RE-delivered, so the
\* engine must dedup).  In-order per client: the op is either the next expected
\* seq (new) or a re-delivery of one already applied (<= hw).
Deliver(op) ==
  /\ ~crashed
  /\ op.seq <= hw[op.client] + 1
  /\ IF op.seq > hw[op.client]
     THEN \* new op: apply exactly once, log it durably, acknowledge it
       /\ value' = value + 1
       /\ hw' = [hw EXCEPT ![op.client] = op.seq]
       /\ applied' = applied \cup {op}
       /\ durLog' = Append(durLog, op)
       /\ acked' = acked \cup {op}
     ELSE \* duplicate: suppress — no apply, no re-emit, no log
       /\ UNCHANGED <<value, hw, applied, durLog, acked>>
  /\ UNCHANGED <<snapVal, snapHW, snapApplied, crashed>>

\* Cut a durable timestamp-T snapshot and truncate the log.
Snapshot ==
  /\ ~crashed
  /\ snapVal' = value
  /\ snapHW' = hw
  /\ snapApplied' = applied
  /\ durLog' = << >>
  /\ UNCHANGED <<value, hw, applied, acked, crashed>>

\* Crash: all VOLATILE state is lost; only the durable snapshot + log survive.
\* `acked` is client-side history and is NOT rolled back.
Crash ==
  /\ ~crashed
  /\ crashed' = TRUE
  /\ value' = 0
  /\ hw' = [c \in Clients |-> 0]
  /\ applied' = {}
  /\ UNCHANGED <<acked, durLog, snapVal, snapHW, snapApplied>>

\* Replay-with-dedup fold: restore the snapshot, then re-apply the durable log,
\* suppressing any op already covered by the high-water mark (idempotent).
RECURSIVE Replay(_, _, _, _)
Replay(s, val, ap, h) ==
  IF s = << >>
  THEN [value |-> val, applied |-> ap, hw |-> h]
  ELSE LET op == Head(s) IN
         IF op.seq > h[op.client]
         THEN Replay(Tail(s), val + 1, ap \cup {op}, [h EXCEPT ![op.client] = op.seq])
         ELSE Replay(Tail(s), val, ap, h)

\* Recover: load the latest snapshot, then replay the durable log with dedup.
Recover ==
  /\ crashed
  /\ LET r == Replay(durLog, snapVal, snapApplied, snapHW) IN
       /\ value' = r.value
       /\ applied' = r.applied
       /\ hw' = r.hw
  /\ crashed' = FALSE
  /\ UNCHANGED <<acked, durLog, snapVal, snapHW, snapApplied>>

Next ==
  \/ \E op \in Op : Deliver(op)
  \/ Snapshot
  \/ Crash
  \/ Recover

Spec == Init /\ [][Next]_vars

(***************************************************************************)
(* Safety invariants (model-checked by TLC).                               *)
(***************************************************************************)

\* EXACTLY-ONCE: the counter equals the number of distinct applied ops — no op
\* is ever double-counted (e.g. by a faulty recovery replay) and none skipped.
ExactlyOnce == ~crashed => (value = Cardinality(applied))

\* NO LOST ACKNOWLEDGED WRITE: every op acknowledged to a client is reflected in
\* the live state once the engine is up — across every crash + recovery.  This is
\* durable linearizability for the workload (the ob-jepsen property, proven).
NoLostAck == ~crashed => (acked \subseteq applied)

\* The durable snapshot is itself consistent (counter = |applied| at the cut).
SnapshotConsistent == snapVal = Cardinality(snapApplied)

\* Acknowledged ops are always durably recorded (in the snapshot or the log) — the
\* output-commit precondition: an op is acked only after it is durable.
AckedDurable == \A op \in acked : (op \in snapApplied \/ op \in Range(durLog))
=============================================================================
