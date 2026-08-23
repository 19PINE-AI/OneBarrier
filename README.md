# OneBarrier

A fault-tolerant service keeps running when a machine crashes. Most services get there
by being written for it: state externalized into a replicated store, or the whole
application restructured around a replay-friendly framework.

OneBarrier makes unmodified server binaries fault-tolerant instead. Stock redis, nginx,
PostgreSQL: no patch, no rebuild, no library to link, no kernel module. Clients lose no
acknowledged work and never see an effect applied twice.

[Paper](https://arxiv.org/abs/2608.14601) ·
[Site](https://01.me/research/OneBarrier) ·
[Getting started](docs/getting-started.md) ·
[How it works](docs/how-it-works.md) ·
[Your app](docs/your-app.md) ·
[Results](docs/research/RESULTS.md)

## The system

OneBarrier runs `k` replicas over a network that delivers messages in one global order
and confirms delivery with a commit barrier.

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/figures/architecture-dark.svg">
    <img alt="Clients send into a total-order fabric providing Order, Barrier and Durability; one primary replica executes the unmodified binary over the determinism shim while two backups only append to a durable log" src="docs/figures/architecture-light.svg" width="100%">
  </picture>
</p>

One replica executes. The others only log. Every input is scattered to the backups in a
single round trip *inside* the barrier the network was already crossing to confirm
delivery, so an input is durable by the time delivery is confirmed, and the reply that
was waiting on durability was waiting on that barrier anyway. Tolerates `f < k`
simultaneous fail-stop crashes.

When a replica dies, the fabric's failure detector excises it within tens of
microseconds and the survivors' barrier resumes over the reduced group, holding the
total order across the crash. The dead replica rejoins by loading its last snapshot,
replaying its log suffix in timestamp order, and fetching whatever prefix it missed from
a survivor.

Only one replica executes, so N replicas cost about 1x the execution CPU instead of Nx.
That's the argument against active state-machine replication, and it's also why there's
no automatic failover: promoting a new primary is a view change, which needs consensus.
See [limitations](#limitations).

## Why this has been expensive for forty years

Transparent fault tolerance has been chased since the 1980s and has never reached
production. The obstacle was never checkpointing. It was three costs that every
transparent system paid on the critical path of every request:

**The order log.** Replay only works if inputs are re-applied in their original order,
and on an ordinary network that order is an accident of timing. So every message's
arrival order gets recorded first, which is the dominant overhead in deterministic-replay
systems.

**The coordinated snapshot.** A distributed checkpoint has to be globally consistent: no
node may record receiving a message that no node records sending. Chandy-Lamport
coordinates the cut with marker messages and records in-flight channels.

**The output hold.** A reply that reached a client can't be un-sent, so every visible
output waits until the state behind it is durable. That's the output-commit problem, and
it's what sank Remus: tens of milliseconds on every reply.

Industry took the other road and rewrote the applications, which is what Temporal, DBOS,
Restate, and Flink are. That works one rewrite at a time, and abandons the installed base
whose rewrite was the cost transparency existed to avoid.

## Four conditions

The paper's thesis is that these three costs aren't intrinsic to fault tolerance.
They're the price of running over a network that promises nothing. Four conditions make
them disappear. Three are the network's:

| condition | meaning |
|---|---|
| **Order** | all messages delivered at all receivers in one global order, identified by a timestamp |
| **Barrier** | delivery confirmed by a commit barrier: a point at which a host knows everything ordered up to `T` has landed and nothing can be lost |
| **Durability** | each message copied to backups no later than its commit barrier |

Under Order the order log vanishes, because the network remembers the order. Under Order
and Barrier a snapshot needs no coordination, because every node independently cuts at
the same timestamp and an empty channel is already a consistent cut. Under Barrier and
Durability the output hold is free, because the durability wait ends at the same barrier
the reply was already waiting for.

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/figures/costs-dark.svg">
    <img alt="Each of the three classical costs, paired with the conditions that remove it" src="docs/figures/costs-light.svg" width="100%">
  </picture>
</p>

OneBarrier gets those three from [1Pipe](https://doi.org/10.1145/3452296.3472909), an
in-network total-order fabric with microsecond round trips.

The fourth condition, **Determinism**, is the host's job, and it's most of the code here.

## Determinism, the fourth condition

Replaying inputs only rebuilds state if the program is a function of its inputs, and
real servers aren't. They read the clock, they draw randomness, and they let the OS
decide which thread gets a lock first. None of that is in a request log, so replay gives
you a server that agrees on the data and disagrees on everything derived from it.

Three `LD_PRELOAD` libraries close that, each usable on its own, on commodity hardware
with no fabric and no root:

| library | closes | how |
|---|---|---|
| `libobpreload.so` | time | a virtual clock that advances on input events, not on the wall clock |
| `librngdet.so` | randomness | a seccomp filter traps raw `getrandom(2)` and serves a fixed stream |
| `libdetsched.so` | threads | Kendo-style logical clocks make lock order deterministic |

The clock is the interesting one. Logging clock values and handing them back in order
works until the program reads the clock on an internal timer (redis `serverCron`, nginx
`ngx_time_update`) rather than per request. The timer fires a different number of times
during replay, the cursor into the log slips, and everything after it is wrong. A
virtual clock has no cursor: time is `base + ticks` where ticks come from input events,
so it doesn't matter who reads it or how often.

Threads are the ugly case, and the measurement changed my recommendation. Forcing
deterministic lock order costs over 1000x on a contended server. So don't do that. Run N
single-threaded instances instead, which are deterministic because there's only one
thread. That's also faster: 4 single-threaded memcached shards do 1.0 M ops/s against
821 k for one `memcached -t 4`, because shards don't fight over locks.

## Numbers

All reproduced by a command in [docs/research/RESULTS.md](docs/research/RESULTS.md).

**Replication rides the barrier.** The main result: where you put the replica write is
what costs you. Same durability, same guarantee, two placements:

| replica write | cost per request |
|---|---|
| inside the commit barrier | 4.59 µs (0.23% of delivery latency) |
| serial `fsync` after it | 2963 µs, and throughput collapses |

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/figures/barrier-dark.svg">
    <img alt="Two request timelines to the same scale: riding the barrier releases the reply at 2018 microseconds, stacking an fsync after it releases at 6016" src="docs/figures/barrier-light.svg" width="100%">
  </picture>
</p>

The shape is measured on the real engine over a live fabric. The magnitude at the
microsecond operating point rests on a calibrated model plus published 1Pipe numbers,
since there was no RDMA testbed. It's the one major claim not measured on real hardware.

**Passive costs 1x execution CPU, active costs Nx.** Only one replica executes:

| replicas | active SMR | OneBarrier (passive) | saved |
|---:|---:|---:|---:|
| 3 | 309.5 ms | 108.7 ms | 65% |
| 5 | 519.5 ms | 114.5 ms | 78% |
| 7 | 729.7 ms | 123.1 ms | 83% |

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/figures/passive-dark.svg">
    <img alt="Execution CPU against replica count: active SMR grows linearly, OneBarrier stays flat" src="docs/figures/passive-light.svg" width="100%">
  </picture>
</p>

**Correct under crash, on a live fabric.** Three replicas and two clients over the real
`ReliableHost` path converge to the exact expected state with no message-order log. Kill
a replica mid-stream and the survivors still reach it, while the victim recovers its
durable prefix and state-transfers to catch up. Holds at 3, 5, 7, and 9 replicas. Crash
injection with real `kill -9` gives linearizable exactly-once histories, and the engine
protocols are model-checked in TLA+ (3.5M states, no violation).

**Fault tolerance is nearly free in throughput.** A fault-tolerant Redis-protocol server
against stock Redis with no persistence and no fault tolerance at all:

| | SET | GET | INCR |
|---|---:|---:|---:|
| Redis, not fault-tolerant | 239 234 | 244 499 | 238 095 |
| OneBarrier, fault-tolerant | 142 248 | 188 679 | 233 100 |
| | 59% | 77% | 98% |

**Recovery is linear in log length**, about 1.9 M requests/s, reconstructing the key set
exactly: 100 k requests in 87 ms, 1 M in 536 ms.

**Fifteen unmodified applications recover byte-identically**: redis, memcached, nginx,
Node.js, PostgreSQL, MariaDB, SQLite, Redis Streams, a Kafka partition model, Mosquitto,
dnsmasq, lighttpd, HAProxy, a Click network function, and a Python microservice. Each is
checked against a control run without OneBarrier that has to differ, so a pass isn't a
test that would have passed anyway. What survives unchanged: nginx's `Date:` header,
formatted deep inside nginx from its own cached clock; memcached's LRU eviction set, all
1416 survivors; Node's `Math.random()`; SQLite rows built from SQLite's own PRNG; a load
balancer's per-flow conntrack timestamps.

Interception overhead is under 5% for nginx under realistic load, p99 unchanged at 2 ms.
Time and RNG virtualization are within noise.

## Try it

The determinism layer runs on one machine, no fabric and no special hardware. That's
what the demo exercises: the shims plus record/replay recovery of one unmodified binary.
It isn't the replicated system, it's the part you can run on a laptop right now.

Linux, `gcc`, Rust 1.85+.

```bash
git clone https://github.com/19PINE-AI/OneBarrier.git
cd OneBarrier
make
make doctor    # tells you what else is worth installing
make demo      # kills an unmodified redis and brings its state back
make verify    # redis, memcached, nginx, node, each with a control
make test      # includes the multi-replica fabric tests
```

`make verify` records each server, kills it, waits out a real-time gap, replays, and
compares against a control run with no OneBarrier:

```
redis     live/replay 1782054868.424071 == 1782054868.424071  | control 1782054874.139431  OK
memcached live/replay STAT time 1782054875 == 1782054875      | control STAT time 1782054880  OK
nginx     live/replay Date 15:14:41 GMT == 15:14:41 GMT       | control Date 15:14:46 GMT  OK
node      live/replay {"now":1782054887981} == ...887981      | control {"now":1782054893724}  OK
```

The replicated engine is in `crates/onebarrier/`. `make test` runs it over a live
loopback-UDP fabric, including the convergence and replica-crash tests above.

## Limitations

**No automated failover.** The recovery mechanism is validated and failure detection is
inherited from the fabric, but primary promotion is a view change, which needs consensus,
and it's left to a production layer. A primary failure opens a recovery window of
unavailability. That window is the price of passive replication's CPU savings.

**No switch or RDMA testbed.** The ride-versus-stack structure is measured on the real
engine; its magnitude at the microsecond operating point rests on a calibrated model and
published 1Pipe numbers. It's the one major claim not measured on real artifacts.

**Durability is in-memory.** Replication tolerating `f < k` fail-stop crashes, not
persistence across correlated power loss. Same tradeoff as FaRM and RAMCloud, and it's
what puts the replica write inside the barrier instead of after it.

**Share-nothing only.** Order-log-free replay covers share-nothing servers. Arbitrary
shared-memory multithreading falls back to the checkpoint-only CRIU path, which is how
PostgreSQL and MariaDB are covered. The fit test is in [docs/your-app.md](docs/your-app.md).

**Residual nondeterminism.** Two CPU instructions take no syscall and export no symbol:
`RDRAND`, pinned per-consumer here, and `RDTSC`, unused for externally visible state by
the fifteen applications. A fully general guarantee would trap both.

**The boundary of transparency** is an impossibility, not an engineering gap. No
transparent system can un-send an effect already delivered to an external party that
won't cooperate in deduplication; that's the two-generals obstacle. Output commit bounds
the inconsistency window, it can't close it. The clean wins are fabric-internal services,
idempotent interfaces, and self-contained leaf services.

This is a research prototype. It works, but it isn't a production service.

## Layout

| path | contents |
|---|---|
| `crates/onebarrier/` | the replicated engine: protocol, durable log, snapshots, recovery, benchmarks, checkers |
| `interpose/` | the determinism shims and the per-app harnesses |
| `bin/onebarrier` | the CLI |
| `spec/` | TLA+ specs for the engine and the fabric's total order |
| `docs/` | guides; `docs/research/` has the full experimental record |
| `paper/` | paper source and figures |
| `website/` | companion site |

## History

I wrote the first draft of this at Microsoft Research in 2017 and never finished it. The
idea was there, but the implementation and measurement it needed was more than I got
through on my own, and it sat as a draft for nine years.

I finished it in 2026 with LLMs doing most of the work: the code, the fifteen
application harnesses, the experiments, and much of the writing, directed by voice
([whisper coding](https://19pine.ai)) while I specified and reviewed.

## Citing

> Bojie Li. "OneBarrier: What a Network Must Provide for Transparent Fault Tolerance to
> Be Free." arXiv:2608.14601, 2026.

Metadata in [CITATION.cff](CITATION.cff).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). The most useful thing you can add is a new
application harness. If you get something recovering byte-identically that isn't on the
list above, that's a real result.

## License

MIT or Apache-2.0, your choice.
