# OneBarrier — Research Plan

*Transparent, low-overhead fault tolerance for unmodified share-nothing servers,
as a byproduct of total-order communication.*

Status: living document. Last updated 2026-06-21. Built on the public
[1Pipe reproduction](https://github.com/bojieli/1Pipe). This plan is deliberately
self-critical: the novelty section below records prior art that *substantially
anticipates* the original pitch, and the contribution is reframed accordingly.

---

## 0. One-sentence claim (and the honest qualifier)

**Claim.** Once all inter-process communication of unmodified share-nothing
servers is routed through an in-network *total-order reliable* fabric (1Pipe),
transparent passive fault tolerance costs ≈ the total-order baseline — because
the same 2PC commit barrier that delivers a message reliably is the barrier
output-commit was waiting for.

**Qualifier (calibrated to the 1Pipe paper's real numbers).** Recovery-state
durability is supplied by **in-fabric RDMA replication**, which the paper already
demonstrates at **1 RTT** (§2.2.2; Ceph 4 KB write 160→58 µs). At 1Pipe's
operating point — **RDMA RTT 1–2 µs, best-effort delivery 10 µs, reliable +2–10 µs
(≤21 µs), recovery 50–500 µs** (§6–7, Fig 9–10) — that 1-RTT replica write rides
*under* the 1.5-RTT 2PC commit barrier, so the output-commit durability overlaps
and FT's marginal cost over the reliable-fabric baseline is ≈ 0. The "stacks on
the critical path" cases (fsync-to-SSD 100 µs–ms, cross-AZ >1 RTT) are a
*different, weaker-latency design point*, not the 1Pipe regime — flagged as an
out-of-regime sensitivity in RQ2, not a central risk. Honest scope of the
durability guarantee: **in-memory f-of-k fail-stop tolerance, not crash-consistent
persistence across correlated power loss** (the FaRM/RAMCloud tradeoff).

**Operating-point note (this is load-bearing for novelty — see §3).** Transparent
FT was abandoned because the output-commit hold + ordering cost were a large
fraction of application latency at the **millisecond** scale every prior system
lived at (Remus held output tens of ms; HyCoR logs nondeterminism over a normal
network; LLFT sequences in host software over TCP). At 1–2 µs RTT with ordering
in the **P4 switch** and replication in **1 RTT**, that cost *structure* collapses:
FT becomes a byproduct of a fabric deployed for communication-correctness reasons,
negligible under app processing. Reframing a decades-old "too expensive" verdict
at a new operating point is the contribution's spine.

---

## 1. Problem & motivation

Transparent FT (no application rewrite) has repeatedly failed to reach
production. The reason was never snapshotting — it was three coupled costs:

1. **Logging non-determinism** (esp. message order) — the dominant per-event
   overhead of deterministic replay.
2. **Coordinating the distributed cut** — Chandy–Lamport marker propagation.
3. **Output-commit latency** — withholding externally-visible output until the
   producing state is recoverable (Strom–Yemini, TOCS 1985). This is what sank
   Remus (NSDI 2008): tens of ms of output-hold per externalizing message.

Meanwhile the industry chose the *other* path: rewrite apps to separate state
(durable execution — Temporal/DBOS/Restate; exactly-once stream processing —
Flink). That path won mindshare but not the legacy fleet: millions of lines of
unmodified Redis/Memcached/Nginx/Node/SQLite-class code will never be ported.
The transparent niche is real **where the rewrite tax is high and the external
side-effect surface is small or idempotent.**

## 2. Key insight

A total-order *reliable* fabric pays — up front, for communication-correctness
reasons — for exactly the three things FT needs:

- (a) the **message order** → the replay order-log is removed (the fabric is the
  order);
- (b) an **empty-channel uncoordinated cut** → a timestamp-T snapshot replaces
  Chandy–Lamport (each in-cut message is on the same side at sender and receiver
  by the identical `ts ≤ T` predicate — *stronger* than a CL cut: no in-flight
  channel state to capture);
- (c) the **output-commit barrier coincides** with the fabric's reliable-delivery
  2PC commit barrier → FT adds no latency over the reliable-fabric baseline,
  *when durability rides phase-1 of that 2PC.*

The single genuinely new engineering idea is (c): **fold the durable-replica
write into 1Pipe's 2PC phase-1 so the phase-2 barrier ack doubles as the
durability ack.**

## 3. Novelty — the honest reckoning

A 3-angle adversarial prior-art red-team (SMR lens, FT-for-free lens,
output-commit-coincidence lens) found the thesis **substantially anticipated**:

| Prior system | Venue | What it already did | Gap OneBarrier must own |
|---|---|---|---|
| **LLFT** | Computer Journal 2013 (arXiv 2010) | Transparent FT of **unmodified socket apps** with **no separate replay order-log**, by tagging messages with **totally-ordered virtual time** | LLFT's order source is a *host-level virtual-time sequencer*, not an *in-network* fabric; it pays a software ordering cost OneBarrier offloads to 1Pipe. Must show the in-network fabric removes that cost *and* the output-commit overlap. |
| **HyCoR** | UCLA 2021 | **Checkpoint-replay** for **unmodified containers** instead of N active replicas; brief deterministic-replay window | HyCoR **still logs non-determinism** (incl. order) over a normal network. OneBarrier removes the order-log entirely via the fabric. Distinction is empirical: overhead vs HyCoR's logging. |
| **NOPaxos / Eris / Derecho** | OSDI'16 / SOSP'17 / TOCS'19 | Offload total order to the network for **consensus/SMR** at near-unreplicated latency | These are *active* SMR (N replicas execute). OneBarrier is *passive* (1 live + log + snapshot). Resource-cost distinction. |
| **Sundial / Huygens** | OSDI'20 / NSDI'18 | Clock-synchronized **consistent snapshots** | Snapshot primitive only; not transparent app FT with output-commit. |
| **Remus / VMware FT** | NSDI'08 / OSR'10 | Transparent VM-FT, output buffered until sync | The output-hold latency OneBarrier claims to remove via the barrier coincidence. |

**The kill-shots, stated plainly:**
- *"It's passive SMR; the order-log vanishing is Schneider's 36-year-old SMR
  property; HyCoR already did passive checkpoint-replay for unmodified apps."*
- *"LLFT already did transparent, no-order-log FT of unmodified socket apps via
  totally-ordered virtual time — you just substitute 1Pipe for the order source."*
- *"1Pipe is a network, not stable storage; output-commit needs durability the
  fabric does not provide, so the coincidence is incomplete."*

**What survives (the narrowed, defensible contribution).** Not a new primitive —
a **co-design + measurement** result:

> OneBarrier is the first system to show that routing unmodified share-nothing
> servers through a single **in-network total-order reliable fabric** makes
> transparent passive FT overhead ≈ the total-order baseline, by **folding the
> output-commit durability write into the fabric's 2PC commit barrier** — and to
> *quantify* that overlap and distinguish it head-to-head from LLFT (host-level
> virtual time + software order cost) and HyCoR (still logs non-determinism).

This is an NSDI-class *realization-and-measurement* paper, not an OSDI *new-idea*
paper, **unless** experiment RQ2 shows the barrier-coincidence buys something
LLFT/HyCoR demonstrably cannot. The experiments below are therefore the
contribution, not decoration. (Decision point flagged for the author: if RQ2
does not separate us from LLFT empirically, reposition as a 1Pipe *application*
paper.)

**The operating-point argument against the "early prior art" objection.** LLFT
(2010/13), HyCoR (2021), Remus (2008), VMware FT all live at the **millisecond**
scale (software/host-virtual-time ordering, hypervisor nondeterminism logging,
TCP). Their transparent-FT overhead was a *large fraction of* application latency,
which is why the technique never shipped broadly. OneBarrier's claim is not "the
same idea" — it is that **at the in-network-total-order + RDMA operating point
(1–2 µs RTT, ordering in the P4 switch, 1-RTT replication) transparent passive FT
becomes essentially free**, because the cost *structure* changes: order and
durable replication are byproducts of a fabric deployed for other reasons, not
mechanisms added on the FT critical path. The "is it just faster hardware?"
critique is answerable: the hardware changes which *cost regime* governs, and the
result (FT marginal cost ≈ 0 over the fabric baseline; a long-dismissed technique
made practical) is a non-obvious design conclusion. RQ7's LLFT/HyCoR head-to-head
must quantify exactly this ms-vs-µs gap, or the objection lands.

## 4. System design

```
   unmodified app (Redis / Memcached / Nginx / Node / SQLite)
        │  POSIX syscalls (sockets, time, rng)
   ┌────▼─────────────────────────────────────────────┐
   │  OneBarrier libOS shim (SocksDirect lineage)      │
   │   • POSIX socket  →  1Pipe ReliableHost.send/poll │
   │   • virtualize local non-determinism (time, rng)  │
   │   • compute-side QUIESCE at barrier-T             │
   │   • deterministic-replay engine + output suppress │
   │   • durable ordered log + timestamp-T snapshot    │
   └────┬─────────────────────────────────────────────┘
        │
   ┌────▼─────────┐  in-fabric RDMA replica (durability = 2PC phase-1)
   │ 1Pipe fabric │  total order + reliable 2PC commit barrier + recovery cut
   └──────────────┘
```

Mechanisms (mapping to the pinned 1Pipe dependency):
- **Order / delivery / commit barrier / recovery cut** → `1pipe-net::ReliableHost`
  (`send`, `poll → Vec<Delivered>{msg_ts, sender_id, payload}`), `ReliableEndpoint`,
  the decentralized recovery-cut agreement. *Already implemented and tested.*
- **Deterministic replay** → apply `Delivered` in `msg_ts` order; no order-log.
- **timestamp-T snapshot** → checkpoint state when the commit barrier passes T
  (empty-channel cut; uncoordinated).
- **Compute-side quiesce** → at barrier-T, stop dispatching `>T`, drain in-flight
  `≤T` handlers before reading state (required off the single-threaded model;
  one of the two consistent-cut holes we found).
- **Durable ordered log + output suppression** → persist `(ts, op)`; suppress
  re-emitted outputs on replay via per-client `(client, seq)` high-water-mark.
- **Output-commit = 2PC phase-1 durability** → the new idea.

## 5. Scope & non-goals (stated up front — generality overreach sank v1)

- **In scope:** share-nothing / event-loop / single-threaded-per-core servers,
  intra-DC, external effects that are fabric-internal (cooperating) or idempotent.
- **Out of scope:** arbitrary multithreaded shared-memory apps (need the quiesce
  extension; bounded result, not headline); transparency to non-cooperating
  internet peers (handled by buffer-at-ingress, confined to the edge);
  **crash-consistent persistence across correlated power loss** (in-fabric
  replication is f-of-k fail-stop, not disk-durable).
- **The impossibility boundary (provable, not a bug):** transparent handling of
  non-replayable external effects to a *non-cooperating* peer is impossible
  without either peer cooperation (idempotency/dedup) or output-commit latency
  (Elnozahy et al. survey, ACM CS 2002; Strom–Yemini). Declared, not papered over.

## 6. The external-effect / idempotency taxonomy (an experimental axis)

| Bucket | Example | Handling | Cost |
|---|---|---|---|
| 1. Fabric-internal | service→service | order + commit barrier; seq# suppress | ~0 |
| 2. Idempotent external | `SET k v`, HTTP PUT | allow duplicate replay | ~0 |
| 3. Dedup-able external | payment w/ idem-key | attach request-ID; dedup | small |
| 4. Naive non-idempotent peer | `INCR` to dumb peer | buffer-at-ingress until committed | output-commit stall, confined to edge |

Money microbenchmark: **Redis `SET` (idempotent) vs `INCR` (not).** A naive
log-replay double-counts `INCR` after a crash; OneBarrier suppresses it via the
durable seq# high-water-mark. One figure proves the output-suppression mechanism
and demarcates where transparency ends.

## 7. Experiment plan (RQ1–RQ8)

Baselines gathered (real, cited) to beat or position against:
- Redis Cluster failover: ~3.3 s detection; ≥2 s replica wait (redis.io).
- Flink restart: a minute+ per checkpoint; 2.0 disaggregated up to 49× faster.
- Remus: 25 ms checkpoints, seconds failover, tens-of-ms output hold (NSDI'08).
- COLO: ~2.1 s failover (SoCC'13).
- CheckFreq: <3.5% overhead (FAST'21); JIT-checkpoint: <1 s wasted work (EuroSys'24).
- 1Pipe substrate: ~10 µs best-effort latency, reliable = +1 RTT, recovery
  50–500 µs, ~90% of non-FT throughput (SIGCOMM'21).

Run in **de-risking order** — RQ2 first; it decides whether the thesis exists.

| # | Research question | Experiment | Falsifier / baseline |
|---|---|---|---|
| **RQ2** | Is FT free over the reliable-fabric baseline? | Decompose request latency; show durable-replica write overlapping 1Pipe 2PC phase-1. **Ablation: force serial durability → latency must jump by one durability-RTT.** Sweep durability tier (RDMA-replica / fsync / cross-AZ). | OneBarrier latency ≠ reliable-1Pipe latency. **Make-or-break.** |
| **RQ1** | Steady-state overhead | Redis/Memcached YCSB+memtier: p50/p99/p99.9 CDF, throughput | native, **1Pipe-only**, logging-replay-FT, SMR (N active), Redis AOF+replica, Remus |
| **RQ3** | Recovery speed & convergence | Inject crash at varying load × snapshot interval; client-observed throughput dip; verify the **replay catch-up condition** `R_replay > R_live` and the barrier-hold backpressure that restores it | Redis replica promotion (we will be *slower* — own this), Flink restore (we beat) |
| **RQ4** | Correctness under faults | **Jepsen-style linearizability** through repeated fault injection (crash, crash-mid-snapshot, crash-mid-output, log-holder loss, partition) | any divergence from a serial reference over the same total order |
| **RQ5** | Cost vs SMR | Cores for equal availability: 1 live+log vs N active | SMR with total-order broadcast |
| **RQ6** | Scale | Overhead vs node count (8→32+); barrier is in-network | 1Pipe-only |
| **RQ7** | Contribution ablation | (a) **order-log off** (fabric supplies order) vs logging order — shows the cost killed; (b) quiesce off on a multithreaded app → torn snapshot; (c) predictor on/off for local nondet → minor | — |
| **RQ8** | Boundary costs (honest) | quiesce/drain latency vs threads; edge-buffering penalty (confined to ingress); per-app idempotency-bucket fractions; snapshot-interval square-root rule `I* ≈ √(2·C_snap / (p_fail·R_live·C_replay/(R_replay−R_live)))` | — |

**Money graphs:** (1) Redis latency CDF — OneBarrier hugs 1Pipe-only, others fan
right; (2) RQ2 output-commit decomposition + ablation; (3) recovery timeline vs
Redis/Flink; (4) cores vs SMR; (5) RQ7 contribution breakdown.

**Head-to-head vs prior art (required by §3):** add LLFT-style host-virtual-time
ordering and a HyCoR-style nondeterminism-logging mode as *baselines in our own
harness*, to show the in-network fabric + barrier-coincidence beats them on the
exact axis we claim (RQ2/RQ1). Without this, reviewers reject on §3 prior art.

## 8. Implementation roadmap (on the local 1Pipe fabric)

- **M0 (this repo, started):** OneBarrier core — KV state machine (SET/INCR/GET),
  durable log, timestamp-T snapshot, deterministic replay + output suppression,
  recovery. Pure-logic + unit tests (RQ4 micro: post-recovery state ≡ serial
  reference; INCR-not-double-applied). ✅ scaffolding.
- **M1:** Wire the engine to `ReliableHost` → a real OneBarrier node over UDP;
  `ob net`/`ob node` mirroring `1pipe net`/`node`; kill-and-recover demo.
- **M2:** RQ2 harness — latency decomposition + durability-tier sweep + serial
  ablation. The make-or-break measurement.
- **M3:** Redis/Memcached integration via the libOS socket shim (transparency).
- **M4:** Baselines in-harness (LLFT-style, HyCoR-style, SMR, logging-FT).
- **M5:** Jepsen-style checker (RQ4) + recovery/scale sweeps (RQ3/RQ5/RQ6).

## 9. Risks & threats to validity

- **Novelty thin (§3).** Mitigation: make RQ2 + the LLFT/HyCoR head-to-head the
  spine. If they don't separate us, reposition as a 1Pipe application paper.
- **Simulated substrate.** 1Pipe here is a faithful reproduction over UDP/sim,
  *not* the RDMA/Tofino testbed; absolute latencies track the model, not silicon.
  State this exactly as [1Pipe's `docs/CLAIMS.md`](https://github.com/bojieli/1Pipe/blob/01b307861bc608f758b9297147688b84f90580c5/docs/CLAIMS.md)
  does. Our RDMA-overlap claim
  (RQ2) is therefore *modeled* unless/until real RDMA is available — label it.
- **Durability tier confusion.** Always state f-of-k in-fabric vs disk-durable.
- **Replay livelock at peak load.** Real; needs barrier-hold backpressure (erodes
  recovery latency under load). Measure it, don't hide it.
- **Hot-standby is faster to fail over.** We win on resource cost and on no-native-
  FT apps, not raw failover latency. Say so.

## 10. Open questions

- Does RQ2's barrier-coincidence overlap survive on *real* RDMA (vs modeled)?
- Can the compute-quiesce extension cover a genuinely multithreaded app cheaply
  enough to broaden scope beyond single-threaded servers?
- Is there a sharper distinction from LLFT than "in-network vs host-level order
  source" — e.g., does the empty-channel uncoordinated cut give an asymptotic
  coordination-cost win at scale (RQ6) that LLFT's virtual time cannot?
</content>
