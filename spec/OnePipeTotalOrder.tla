-------------------------- MODULE OnePipeTotalOrder --------------------------
(***************************************************************************)
(* A formal TLA+ specification of 1Pipe's central safety property —        *)
(* TOTAL ORDER — and the barrier mechanism (§4) that establishes it.       *)
(*                                                                         *)
(* Abstraction.  Each process is both sender and receiver.  A message      *)
(* carries (ts, src); the global order is lexicographic on (ts, src) with  *)
(* ties broken by src (paper §4.1).  Senders assign ts = their monotone    *)
(* clock, then advance it, so a sender's future messages have strictly      *)
(* larger ts.  The network may delay/reorder arrivals arbitrarily.          *)
(*                                                                         *)
(* The crux is the *barrier*: a receiver r learns, per sender s, a lower    *)
(* bound `known[r][s]` on the timestamps of s's FUTURE arrivals.  Because   *)
(* barriers ride in-band behind data on FIFO links, r cannot learn a        *)
(* barrier value V for s until it has already received every message from   *)
(* s with ts < V (the FIFO gate — the heart of why this works).  r          *)
(* delivers a buffered message only once its ts is below the aggregated     *)
(* barrier B(r) = min over senders of known[r][s].                          *)
(*                                                                         *)
(* We model-check two safety invariants:                                    *)
(*   Consistent — every receiver's delivered prefix is downward-closed in   *)
(*                the global (ts,src) order (⇒ all receivers agree = TOTAL   *)
(*                ORDER), and each delivery sequence is strictly increasing. *)
(*   Causality  — when r delivers a message with timestamp T, r's own clock *)
(*                already exceeds T (paper §4.1).  This is the property      *)
(*                OneBarrier's consistent-cut snapshot relies on.           *)
(***************************************************************************)
EXTENDS Naturals, Sequences, FiniteSets

CONSTANTS Procs,        \* set of process ids (use a finite set of naturals)
          MaxTs         \* clock bound (keeps the model finite)

ASSUME MaxTs \in Nat /\ MaxTs >= 1

Msg == [ts : 1..MaxTs, src : Procs]

\* The global total order on messages (paper §4.1: by timestamp, ties by src).
Before(m1, m2) == \/ m1.ts < m2.ts
                  \/ (m1.ts = m2.ts /\ m1.src < m2.src)

Range(seq) == { seq[i] : i \in DOMAIN seq }

VARIABLES
  clock,      \* clock[p]   : p's current logical time (monotone non-decreasing)
  sent,       \* sent       : set of messages put on the network (broadcast to all)
  known,      \* known[r][s]: barrier r knows for s (lower bound on s's future ts)
  buffer,     \* buffer[r]  : messages that arrived at r, not yet delivered
  delivered   \* delivered[r]: sequence of messages delivered to r, in order

vars == <<clock, sent, known, buffer, delivered>>

TypeOK ==
  /\ clock \in [Procs -> 1..(MaxTs + 1)]
  /\ sent \subseteq Msg
  /\ known \in [Procs -> [Procs -> 0..(MaxTs + 1)]]
  /\ buffer \in [Procs -> SUBSET Msg]
  /\ delivered \in [Procs -> Seq(Msg)]

Init ==
  /\ clock     = [p \in Procs |-> 1]
  /\ sent      = {}
  /\ known     = [r \in Procs |-> [s \in Procs |-> 0]]
  /\ buffer    = [r \in Procs |-> {}]
  /\ delivered = [r \in Procs |-> << >>]

\* p sends a message stamped with its current clock, then advances the clock.
\* Monotonicity ⇒ every later message from p has a strictly larger timestamp.
Send(p) ==
  /\ clock[p] <= MaxTs
  /\ sent' = sent \cup {[ts |-> clock[p], src |-> p]}
  /\ clock' = [clock EXCEPT ![p] = clock[p] + 1]
  /\ UNCHANGED <<known, buffer, delivered>>

\* p advances its clock with no message (a beacon/idle tick, paper §4.1.2),
\* which lets receivers raise their barriers for p on idle links.
Tick(p) ==
  /\ clock[p] <= MaxTs
  /\ clock' = [clock EXCEPT ![p] = clock[p] + 1]
  /\ UNCHANGED <<sent, known, buffer, delivered>>

\* r receives a message into its buffer (arbitrary network delay / reordering).
Receive(r, m) ==
  /\ m \in sent
  /\ m \notin buffer[r]
  /\ m \notin Range(delivered[r])
  /\ buffer' = [buffer EXCEPT ![r] = buffer[r] \cup {m}]
  /\ UNCHANGED <<clock, sent, known, delivered>>

\* r raises its barrier for s to s's current clock — the LOWER BOUND on s's
\* future timestamps.  THE FIFO GATE: r may do this only once it has already
\* received every message from s with ts < clock[s] (barriers ride behind data
\* on FIFO links).  This single precondition is what makes total order hold.
LearnBarrier(r, s) ==
  /\ clock[s] > known[r][s]
  /\ \A m \in sent : (m.src = s /\ m.ts < clock[s])
                       => (m \in buffer[r] \/ m \in Range(delivered[r]))
  /\ known' = [known EXCEPT ![r][s] = clock[s]]
  /\ UNCHANGED <<clock, sent, buffer, delivered>>

\* The aggregated receive barrier (paper §4.1, Eq 4.1): min over all senders.
Barrier(r) == LET vals == { known[r][s] : s \in Procs }
              IN  CHOOSE b \in vals : \A v \in vals : b <= v

\* Deliver the smallest buffered message whose ts is strictly below the barrier
\* (strict: a future arrival could still tie the barrier value).  This is the
\* actual delivery rule — we do NOT assume sortedness; the invariants check it.
Deliver(r) ==
  /\ \E m \in buffer[r] :
        /\ m.ts < Barrier(r)
        /\ \A m2 \in buffer[r] : (m2 # m) => Before(m, m2)
        /\ buffer'    = [buffer    EXCEPT ![r] = buffer[r] \ {m}]
        /\ delivered' = [delivered EXCEPT ![r] = Append(delivered[r], m)]
  /\ UNCHANGED <<clock, sent, known>>

Next ==
  \/ \E p \in Procs : Send(p)
  \/ \E p \in Procs : Tick(p)
  \/ \E r \in Procs, m \in Msg : Receive(r, m)
  \/ \E r \in Procs, s \in Procs : LearnBarrier(r, s)
  \/ \E r \in Procs : Deliver(r)

Spec == Init /\ [][Next]_vars

(***************************************************************************)
(* Safety invariants (model-checked by TLC).                               *)
(***************************************************************************)

\* Each receiver delivers in strictly increasing global order.
Sorted ==
  \A r \in Procs :
    \A i, j \in DOMAIN delivered[r] :
      (i < j) => Before(delivered[r][i], delivered[r][j])

\* Every delivered prefix is DOWNWARD-CLOSED in the global order: no message a
\* receiver has yet to deliver is ordered before any message it has delivered.
\* Equivalently, delivered[r] is a prefix of the one global (ts,src) order — so
\* any two receivers agree on the order of common messages = TOTAL ORDER.
DownwardClosed ==
  \A r \in Procs :
    \A m \in sent :
      (m \notin Range(delivered[r]))
        => (\A dm \in Range(delivered[r]) : ~Before(m, dm))

Consistent == Sorted /\ DownwardClosed

\* When r delivers a message of timestamp T, r's clock already exceeds T
\* (paper §4.1): the property OneBarrier's timestamp-T cut relies upon.
Causality ==
  \A r \in Procs :
    \A m \in Range(delivered[r]) : m.ts < clock[r]

\* Cross-receiver agreement, stated directly (implied by Consistent): any two
\* messages delivered by both r1 and r2 appear in the same relative order.
AgreeOrder ==
  \A r1, r2 \in Procs :
    \A m1, m2 \in (Range(delivered[r1]) \cap Range(delivered[r2])) :
      Before(m1, m2) => \neg (\E i, j \in DOMAIN delivered[r2] :
                               /\ delivered[r2][i] = m2
                               /\ delivered[r2][j] = m1
                               /\ i < j)
=============================================================================
