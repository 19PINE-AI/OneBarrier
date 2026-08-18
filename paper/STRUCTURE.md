# OneBarrier paper — proposed structure (think-first, before prose)

## The central decision: what is the paper *about*?

The project has two pillars, and they pull in opposite directions on "novelty vs evidence":

- **Pillar 1 — the fabric coincidence ("FT as a byproduct of total-order comms").**
  The intended thesis spine. *Strong idea, weaker evidence:* substantially
  anticipated by LLFT (host virtual-time order) and HyCoR (checkpoint-replay of
  unmodified containers); the latency win is *simulated/reproduced* (RDMA RTT is
  projected, not measured) — real RDMA verbs over SoftRoCE confirm ~1.5 µs per-op
  but are CPU-bound so cannot show the overlap directly.

- **Pillar 2 — the transparent determinism libOS.** *Strongest, most novel,
  most-measured part.* Real, runs **unmodified** redis/memcached/nginx/node/
  PostgreSQL; byte-identical deterministic recovery (time + RNG + threads), CRIU
  general checkpoint incl. a multi-process RDBMS, correctness under fault
  (linearizability + exactly-once torture), recovery-time, and overhead — all
  measured on real binaries.

**Recommendation (Framing C — unify on the operating-point regime change, let the
libOS carry the evidence).** Transparent FT was abandoned because three coupled
costs were large at the *millisecond* scale; we make it practical by two
co-designed shifts: (1) a user-space libOS that makes unmodified apps
deterministically recoverable at a few-percent overhead, and (2) routing them
through a *microsecond* in-network total-order reliable fabric, where all three
costs collapse and FT's output-commit write folds into the fabric's 2PC barrier.
Be honest: realization + measurement, with an explicit novelty reckoning vs
LLFT/HyCoR, and explicit about simulated (RDMA latency) vs real (everything else).

Title (primary): **OneBarrier: Transparent Fault Tolerance for Unmodified Servers
as a Byproduct of Total-Order Communication.**
Author: Bojie Li, Pine AI (matching the UaC template). Target: NSDI-class
realization-and-measurement preprint, ~14 pp main + appendices.

## Section plan (with the evidence each section draws on)

1. **Introduction.** Transparent FT keeps failing to ship; the 3 coupled costs
   (nondeterminism order-log, distributed cut, output-commit hold) — all large at
   ms scale (Remus held tens of ms). Industry chose rewrite-the-app (Temporal/
   DBOS/Flink), abandoning the legacy fleet. Insight: a total-order *reliable*
   fabric pre-pays for all three; at µs/RDMA the cost regime changes. Two-pillar
   contribution + honest scope up front. Contributions list.

2. **Why transparent FT failed, and what changed.** Strom–Yemini output commit;
   Remus; deterministic replay & the order-log; Chandy–Lamport cut. 1Pipe
   (total order, 2PC commit barrier, 1-RTT replication, 1–2 µs RTT). The
   operating-point argument: ms→µs changes which cost regime governs (this is
   load-bearing for novelty). [PLAN §0–2]

3. **Overview: FT as a byproduct of the fabric.** The coincidence, stated as
   (a) order ⇒ no replay order-log; (b) timestamp-T snapshot ⇒ empty-channel cut
   replaces Chandy–Lamport; (c) output-commit barrier coincides with the fabric's
   2PC commit barrier (fold the durable-replica write into phase-1). Architecture
   figure (app → libOS shim → fabric). [PLAN §2, §4]

4. **A transparent determinism libOS for unmodified servers.** *The big real
   contribution.*
   - 4.1 The determinism boundary (request-driven vs timer-driven nondeterminism;
     the boundary we found and closed). [Track B+, "value-replay boundary"]
   - 4.2 Virtual clock (time virtualization; closes timer-driven desync). [Track B+]
   - 4.3 RNG virtualization: getrandom seccomp trap + /dev/urandom mount-ns +
     ASLR-off + RDRAND-disable; redis SipHash/SPOP. [Track B++, B++++]
   - 4.4 Threads: Kendo deterministic scheduling *and why the practical path is
     share-nothing sharding* (single-thread shards beat -t4). [Track B++, B+++++]
   - 4.5 Process-state checkpoint (CRIU) and bounded recovery; multi-process
     PostgreSQL. [Track B+++, B++++++++]
   - 4.6 Output suppression / exactly-once (per-client high-water-mark). [M0]
   - Engineering realities (LD_PRELOAD early-init, glibc symbol-versioning,
     memcached maintenance threads) — short, they make "unmodified" credible.

5. **Fault tolerance over the fabric.** Deterministic replay without an order-log;
   timestamp-T snapshot (uncoordinated, empty-channel); output-commit = 2PC
   phase-1 durability; recovery (durable prefix + state-transfer catch-up).
   [M0/M1, PLAN §4]

6. **Implementation.** OneBarrier engine (Rust, on the 1Pipe `ReliableHost`
   fabric; 18 binaries, 31 tests). libOS shim (C, SocksDirect lineage). Formal
   specs: TLA+ for 1Pipe total order (3.5M states) + OneBarrier exactly-once.
   [STATUS Implementation + Formal verification]

7. **Evaluation.** Lead with the real libOS; then the coincidence.
   - 7.1 **FT marginal cost ≈ 0** — RQ2 (0.23 % marginal vs ~3 ms fsync stack);
     GATE A sim at the RDMA operating point; real RDMA verbs over SoftRoCE
     (1.5 µs, honest CPU-bound caveat). [M2, GATE A, SoftRoCE]
   - 7.2 **Deterministic recovery of 5 unmodified apps** — byte-identical time/
     RNG; end-to-end *state* recovery (redis TTLs, node random+timestamp sessions);
     memcached eviction bit-exactness; PostgreSQL via CRIU. [Track B+, B+++, B++++++, B++++++++]
   - 7.3 **Correctness under fault** — Wing–Gong linearizability on a recovered
     unmodified server; exactly-once Jepsen torture (191k acked, 0 lost/torn). [B+++]
   - 7.4 **Recovery time / availability** — linear in log length (35–536 ms for
     10k–1M reqs); checkpoint bounds it to the tail. [B+++++++]
   - 7.5 **Overhead of the libOS** — time ~0–4 %, RNG ~0 %, capture ~5 %;
     deterministic scheduler tradeoff and the share-nothing sharding answer
     (4 shards beat -t4). [B+++++]
   - 7.6 **Competitors & resource cost** — LLFT/HyCoR/Remus/CRIU head-to-head;
     passive vs active CPU (49–83 % savings); scale; snapshot-interval tradeoff;
     order establishment (sequencer vs fabric). [M4, RQ5–8]
   - 7.7 **Formal verification** — TLC model-checked properties.

8. **What is new (honest novelty reckoning).** The LLFT/HyCoR/NOPaxos/Remus
   table; the kill-shots and what survives (realization + measurement + the libOS
   + the operating-point regime change). Honesty here is a *strength*. [PLAN §3]

9. **Limitations and scope.** Simulated RDMA latency vs real verbs; durability =
   f-of-k fail-stop, not power-loss-safe (FaRM/RAMCloud tradeoff); share-nothing
   scope; multithreading tradeoff; no P4 hardware. [PLAN §0, §5, §9]

10. **Related work.** Transparent FT (Remus/COLO/LLFT/HyCoR); deterministic replay
    (rr/Castor); network-offloaded order (NOPaxos/Eris/Derecho); durable execution
    (Temporal/DBOS/Restate/Flink); CRIU/snapshot-restore; libOS/SocksDirect.

11. **Conclusion.**

Appendices: TLA+ specs; reproduction commands (every result has one); libOS
engineering deep-dive; PostgreSQL CRIU-in-KVM recipe.

## Open structural choices to confirm with the author
1. **Framing** — C (operating-point unifier, recommended) vs A (fabric-spine, the
   original vision) vs B (lead with the libOS as the headline). I recommend C.
2. **Title** — primary above; alt: "Making Transparent Fault Tolerance Practical
   for Unmodified Share-Nothing Servers."
3. **Emphasis split** — how much page budget to the fabric-coincidence (partly
   simulated) vs the libOS (real). I propose ~35 % fabric/FT-theory, ~50 % libOS+
   eval, ~15 % novelty/limits/related.
4. **Venue posture** — keep the candid LLFT/HyCoR reckoning in the body (strength)
   or soften to related work? I recommend keeping it in the body.
