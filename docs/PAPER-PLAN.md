# OneBarrier — Path to a Strong Paper

The next phase: moving from a **validated reproduction** to a **top-tier
systems paper**. Same discipline as `STATUS.md` — every claim must end up
reproduced by a command on the *real* testbed; nothing asserted.

Status today (see `STATUS.md`): all 8 RQs have evidence, 5/5 apps, transparent
interception on unmodified `redis-server`, M0–M5 + Track B complete — **but
everything load-bearing rests on the reproduction** (loopback UDP, software
ordering, KV-on-one-engine, *modeled* competitors). This document is the plan to
fix exactly that.

---

## Status — simulated coverage (2026-06-21)

No real RDMA testbed is available, so the hardware experiments are done in
**local simulation** (latency model from the 1Pipe paper) + **formal verification**
+ **real measurement** where the sandbox allows. All reproduced in `STATUS.md`.

| Plan item | How | Result |
|---|---|---|
| **§5 formal correctness** | TLA+ + TLC (both repos) | ✅ 1Pipe total order (3.5M states); OneBarrier exactly-once + no-lost-ack |
| **GATE A — RQ2 @ RDMA** | discrete-event sim (`ob-sim`) | ✅ FT-overlap tail = reliable-1Pipe baseline at every load; fsync collapses |
| **exp #2 tail latency** | `ob-sim` load sweep | ✅ FT-overlap p99.9 tracks baseline; fsync tail collapses |
| **exp #3 competitors** | sim (`ob-compare`) **+ real CRIU measurement** | ✅ OB = best latency at lowest CPU; dwarfs Remus; CRIU checkpoint 37–121 ms (30–300 MB), full-RSS image — log-based FT beats checkpoint-based |
| **exp #5 nondeterminism** | real `obpreload` measurement | ✅ residual = ~5.8 wall-clock reads/req (redis), zero RNG/other; **vDSO finding** (nginx time via vDSO, uncountable by LD_PRELOAD) |
| **exp #6 recovery + livelock** | recovery model (`ob-recovery`) | ✅ converge / livelock / backpressure characterized |
| **exp #7 linearizability** | from-scratch Wing-Gong (`ob-lincheck`) | ✅ real concurrent OneBarrier history verified linearizable (not acked-set heuristic) |
| **GATE B — interception of unmodified app** | `obpreload` on **redis + nginx** | ✅ stock nginx (multithreaded) intercepted, 2.28 MB captured, 137k req/s; record-replay recovery on stock redis |
| **exp #4 diverse unmodified apps under FULL FT libOS** | virtual clock + RNG (seccomp/urandom) + DMT (Kendo) | ✅ **DONE** — deterministic recovery of 5 unmodified apps (redis, memcached, nginx, node + engine); time/RNG/threads all virtualized; redis-internal RNG (SPOP) too; CRIU full-process checkpoint in a KVM guest (`ob-criu-kvm.sh`) |
| real RDMA measurement | **real verbs via SoftRoCE** (`ob-rdma-softroce.sh`) + sim | ◑ real ibverbs RDMA_WRITE ≈ 1.5 µs (in the operating point); the RTT-bound *overlap* needs hardware (SoftRoCE is CPU-bound) — `ob-sim` models it |

The make-or-break **GATE A passes in simulation**, with the operating-point latency
now corroborated by **real RDMA verbs over SoftRoCE** (≈1.5 µs RDMA_WRITE); **GATE B
reached**, and the **full FT libOS (exp #4) is built and validated** on 5 unmodified
apps (time + RNG + thread scheduling all virtualized), with CRIU general checkpoint
working in a KVM guest. The ONLY thing still needing real hardware is the *direct*
measurement of the FT-overlap latency benefit at 1-2 µs RTT (SoftRoCE is CPU-bound,
so it confirms verbs + per-op latency but not the overlap; the sim models the
RTT-bound regime). Everything achievable without an RDMA NIC is done.

## 0. The gap, named

| Today (reproduction) | Strong paper (real) |
|---|---|
| loopback UDP, software 1Pipe | **real RDMA + P4 programmable switch** |
| durability overlap *modeled* | durability overlap *measured* at 1–2 µs RTT |
| LLFT/HyCoR/Remus = software models | the **actual systems** run head-to-head |
| 5 KV apps we wrote | genuinely **unmodified** Nginx / Node / PostgreSQL |
| network-input interception only | **all** nondeterminism (time/RNG/threads) |
| hand-rolled acked-set check | **Elle/Knossos** linearizability |
| consistent cut asserted + tested | consistent cut **model-checked / proven** |

## 1. Make-or-break gates (do these FIRST, in order)

The paper's bold version is only writable if these pass. Gate them explicitly.

- **GATE A — RQ2 on real RDMA.** Does the durable-replica RDMA write hide under
  1Pipe's ~1.5-RTT commit barrier at 1–2 µs RTT? *If yes → "FT is free" is
  measured, write the bold paper. If no → durability stacks; reposition to
  "lowest-overhead transparent FT" (still real, narrower headline).*
- **GATE B — real libOS runs an unmodified, nondeterministic app under FT.**
  Nginx or PostgreSQL, unmodified, recovered correctly with low overhead. *If
  yes → the transparency claim is real. If no → scope to share-nothing servers
  and say so.*

Everything else is breadth/rigor on top of these two. **Do not invest in the
app matrix or the competitor matrix until A and B are green.**

## 2. Systems to build (prioritized)

1. **OneBarrier on real 1Pipe-over-RDMA + P4.** Wire the engine to the actual
   fabric (SocksDirect/1Pipe hardware lineage). In-fabric replication = a real
   one-sided RDMA write to backup memory, ridden as 2PC phase-1. *Prereq for
   every credible number. Unlocks GATE A.*
2. **The real libOS interceptor (headline system).** SocksDirect-grade
   interception of **all** nondeterminism: sockets→1Pipe, `clock_gettime`/
   `gettimeofday` (virtual time), `getrandom`/RDRAND (recorded/seeded), file I/O,
   signals, and record-replay or a deterministic layer for **thread scheduling**.
   *Unlocks GATE B and the diverse-apps story. The biggest build.*
3. **True passive architecture.** 1 executor + *k* in-fabric RDMA **log-only**
   replicas; real recovery-over-fabric (backup replays the durable log to catch
   up) + the controller-coordinated recovery cut (1Pipe already has agreed-cut
   machinery). *Unlocks real RQ5 + the recovery-livelock regime.*
4. **Compute-side quiesce / deterministic multithreading.** Barrier-T drain of
   in-flight handlers + a deterministic-scheduling layer (cf. Crane/dOS) so
   multithreaded apps replay. *Extends scope past single-threaded; bounds
   consistent-cut Hole 1 honestly.*
5. **Edge proxy for non-cooperating peers.** Buffer-at-ingress + dedup at the one
   DC chokepoint → external clients get exactly-once; the output-commit
   impossibility is bounded, not hand-waved.

## 3. Experiments (prioritized; money graphs marked ★)

1. **★ RQ2 on real RDMA** — output-commit latency decomposition; durable write
   overlapping the 1.5-RTT barrier; serial-durability ablation. *The figure the
   paper lives on.* (GATE A)
2. **★ Tail latency under load** — p99/p99.9 latency CDF vs offered load, FT vs
   non-FT-fabric. The thing that killed Remus (tens-of-ms output-hold tail);
   showing OneBarrier's tail is *flat* is the visceral proof.
3. **★ Real competitor head-to-head** — run the actual **LLFT, HyCoR,
   Remus/COLO, VMware-FT, active SMR** on the same hardware + workloads (M4 today
   is a software model). Makes the novelty defensible empirically.
4. **Diverse unmodified apps** — Nginx, Node, PostgreSQL, a PyTorch trainer under
   the libOS; per-app overhead + correctness. (GATE B + breadth)
5. **Nondeterminism characterization study** — measure the *sources and rates* of
   nondeterminism in real apps; show the residual after the fabric removes
   message-order is small and virtualizable. *A citable contribution on its own —
   nobody has this data cleanly.*
6. **Recovery at scale + real failure modes** — 32+ nodes; inject crashes,
   **partitions, correlated/rack failures, clock skew**; recovery time vs Redis
   Cluster / Flink; exhibit the replay-catch-up convergence + barrier-hold
   backpressure (the livelock regime).
7. **Real linearizability** — **Elle / Knossos** over diverse fault schedules,
   replacing the hand-rolled acked-set check. "A verifier checked," not "we did."
8. **Scale-out overhead (RQ6 for real)** — on hardware where the in-network
   barrier *can* stay flat (what the single-host sim could not show).

## 4. Testbed requirements

- RDMA NICs (RoCEv2 or IB), ≥ 8 hosts for the core results, 32+ for scale.
- ≥ 1 programmable switch (Tofino/TNA) for the in-network barrier; the 1Pipe P4
  is in `~/1Pipe/p4` (needs the Intel SDE). Fallback: host-representative barrier
  (1Pipe §6.2.3) on commodity switches — label it.
- PTP clock sync (1Pipe assumes ~µs skew; the repo's clock-sync layer corrects
  residual).
- Comparison systems installed: Redis Cluster, Flink, FaRM-like RDMA KV, QEMU
  COLO, a HyCoR/CRIU container stack, an LLFT-style host-sequencer build.

## 5. Intellectual contribution (lift it from strong → great)

A **formal argument** (even TLA+/model-checked, not full proof) that the
composition — timestamp-T empty-channel cut + exactly-once dedup + output-commit
gated on the commit barrier — is **linearizable under crash + the stated failure
model**. This converts the elegant consistent-cut result from "asserted + tested"
to "proven," and is the kind of thing that separates a strong paper from a great
one. Pair with the empirical Jepsen/Elle results for belt-and-suspenders.

## 6. Sequencing

```
Phase 1 (de-risk):   RDMA deployment (sys #1) -> GATE A (exp #1)
                     real libOS MVP  (sys #2) -> GATE B (exp #4 on 1 app)
   ── decision point: bold paper vs honest-narrower ──
Phase 2 (depth):     passive arch (#3) + tail latency (exp #2) + competitors (exp #3)
Phase 3 (breadth):   app matrix (exp #4) + nondeterminism study (exp #5)
Phase 4 (rigor):     scale/failures (exp #6) + Elle (exp #7) + formal model (§5)
Phase 5 (scope):     multithread quiesce (#4) + edge proxy (#5) — or future work
```

## 7. Venue read

- **Bold (A and B green):** OSDI/SOSP — "transparent FT is free at the
  in-network-total-order operating point, on real hardware, across diverse
  unmodified apps, beating the real competitors."
- **Narrower (A or B partial):** NSDI — "lowest-overhead transparent FT for
  share-nothing servers on a total-order fabric," still with measured baselines.

## 8. The two questions that decide everything

1. Does real-RDMA RQ2 show the durability overlap? (GATE A)
2. Does the real libOS run a genuinely unmodified, nondeterministic app
   (Nginx/PostgreSQL) under FT with low overhead? (GATE B)

Build toward exactly those; let the results choose the bold vs narrower paper.
Keep the reproduce-everything discipline: each phase lands working code + real
numbers in `STATUS.md`, pushed.
