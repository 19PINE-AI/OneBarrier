# OneBarrier

**Crash a server. Start it again. It comes back exactly as it was — and it never
had to be written to survive.**

[Paper](https://arxiv.org/abs/2608.14601) ·
[Interactive site](https://01.me/research/OneBarrier) ·
[Getting started](docs/getting-started.md) ·
[How it works](docs/how-it-works.md) ·
[Use it on your app](docs/your-app.md) ·
[Measured results](docs/research/RESULTS.md)

---

## What this is

Most programs are not written to survive being killed. Making one survive
normally means rewriting it: add a write-ahead log, add checkpoints, replicate
the state, reason about which replies you may send before which writes are safe.
That work is invasive, it is easy to get subtly wrong, and it has to be redone
for every program.

OneBarrier does it from the outside. You launch an ordinary binary — `redis-server`,
`nginx`, `memcached`, `node`, a Python service you wrote last week — through a
small user-space layer. Nothing about the program changes: no patch, no rebuild,
no library to link, no kernel module, no root. When the process dies, a fresh one
takes its place and re-derives the state the dead one had.

```bash
# record
onebarrier run --session app -- redis-server --port 6379

# ... kill -9 it, wait however long you like ...

# recover
onebarrier recover --session app --target 127.0.0.1:6379 -- redis-server --port 6379
```

The recovered server does not merely have similar state. It is **byte-identical**,
down to the values a program has no business reproducing: the timestamps it
generated, the random IDs it minted, the `Date:` header it wrote, the exact set of
keys its LRU chose to evict.

## Why that is hard, and what makes it work

Replaying a program's inputs only reproduces its state if the program is a
*function* of those inputs. Real servers are not. They read the clock, they draw
randomness, they let the OS decide which thread wins a lock — and each of those is
a private input that no request log contains. Replay the requests alone and you
get a server that agrees on the data and disagrees on everything else.

So OneBarrier virtualizes those three inputs, each with its own `LD_PRELOAD`
library you can use independently:

| Hidden input | What OneBarrier does | Library |
|---|---|---|
| **Time** | A virtual clock that advances with *input events*, not with the wall clock. Replay then ignores the real-time gap entirely. | `libobpreload.so` |
| **Randomness** | A seccomp filter traps the raw `getrandom(2)` syscall and serves a deterministic stream — catching the sources `LD_PRELOAD` alone cannot see. | `librngdet.so` |
| **Thread order** | Kendo-style logical clocks make lock acquisition order a function of the program, not of OS timing. | `libdetsched.so` |

The clock is the interesting one. A conventional record/replay layer logs each
time value and hands them back in order — which works right up until the program
reads the clock on an internal timer rather than per request, at which point the
replay's read count diverges from the recording's and every subsequent value is
wrong. A virtual clock has no cursor to lose: time is a function of how many
inputs have arrived, so it is correct no matter who reads it or how often.

Threads are handled differently, and this is a deliberate choice backed by
measurement. Forcing deterministic lock order is *expensive* — on a contended
multithreaded server it costs over 1000×. So OneBarrier does not recommend it.
It recommends **share-nothing sharding**: run N single-threaded instances, which
are deterministic by construction. That is not a consolation prize. Four
single-threaded memcached shards reach **1.0 M ops/s**, beating one four-threaded
memcached at 821 k — faster *and* deterministic, because there is no lock
contention to lose.

## Results

Fifteen unmodified applications recover byte-identically. Every number below is
reproduced by a command in [`docs/research/RESULTS.md`](docs/research/RESULTS.md).

**Recovery is essentially free at the right operating point.** This is the
paper's central measurement. A durable write placed *inside* the network's
existing commit barrier versus the same write placed *after* it:

| Durability placement | Marginal cost per request |
|---|---|
| Riding the commit barrier | **4.59 µs** — 0.23 % of delivery latency |
| Serial `fsync` after it | **2963 µs** — 645× more, and throughput collapses |

**Fault tolerance costs almost nothing in throughput.** A fault-tolerant
Redis-protocol server against stock non-fault-tolerant Redis:

| | SET | GET | INCR |
|---|---:|---:|---:|
| Redis, no persistence (not fault-tolerant) | 239 234 | 244 499 | 238 095 |
| OneBarrier, fault-tolerant | 142 248 | 188 679 | 233 100 |
| | 59 % | 77 % | **98 %** |

**Recovery is fast and scales linearly.** Rebuilding an unmodified redis from its
request stream:

| Requests replayed | Keys recovered | Time |
|---:|---:|---:|
| 10 k | 9 998 / 9 998 ✓ | 35 ms |
| 100 k | 99 957 / 99 957 ✓ | 87 ms |
| 1 M | 994 967 / 994 967 ✓ | 536 ms |

~1.9 M requests/second, exact reconstruction every time.

**The interception layer is cheap.** Under realistic (non-pipelined) load, nginx
pays **under 5 %** with p99 latency unchanged at 2 ms. Virtualizing time and
randomness is within noise.

**Fifteen applications, byte-identical across a crash**, each verified against a
control run that must *differ* — so a pass proves the determinism came from
OneBarrier and not from a trivially-identical test:

> Redis · Memcached · Nginx · Node.js · PostgreSQL · MariaDB · SQLite ·
> Redis Streams · Kafka-model partitions · Mosquitto MQTT · dnsmasq · lighttpd ·
> HAProxy · a Click network function · a Python order-service microservice

What comes back identical, app by app: Redis's `TIME` and its `SPOP` draws;
nginx's `Date:` header, formatted deep inside nginx from its own cached clock;
memcached's LRU eviction set (1416 survivors, same hash); Node's `Math.random()`;
SQLite rows whose values came from SQLite's own PRNG; a stateful load balancer's
per-flow conntrack timestamps; a microservice's random order IDs.

And the correctness is not only empirical. The core protocols are
**machine-checked in TLA+** — 3.5 × 10⁶ states explored with no violation — and
crash injection with real `kill -9` confirms linearizable, exactly-once histories.

## The claim behind the numbers

Transparent fault tolerance has been chased for forty years without reaching
production, because every attempt paid three costs on the critical path: record
message order for replay, coordinate a consistent snapshot, and hold each reply
until the state behind it is durable.

The paper's argument is that these are not intrinsic costs. **They are the price
of a network that guarantees neither order nor delivery.** Given a network that
does — one that delivers messages in a single global order, confirms delivery
with a commit barrier, and replicates to backups before that barrier completes —
all three disappear. There is no order to record, because the network *is* the
order. There is no snapshot to coordinate, because an empty channel is already a
consistent cut. And the barrier a reply was waiting on is a barrier the network
was going to cross anyway.

Three of those four conditions belong to the network; OneBarrier gets them from
[1Pipe](https://doi.org/10.1145/3452296.3472909), an in-network total-order fabric
with microsecond round trips. The fourth — determinism — belongs to the host, and
that is the `LD_PRELOAD` layer in this repository, which costs 2–10 % and needs no
special hardware at all.

**On a network that meets the conditions, fault tolerance is a property, not a tax.**

## Getting started

Requires Linux, `gcc`, and Rust 1.85+.

```bash
git clone https://github.com/19PINE-AI/OneBarrier.git
cd OneBarrier
make                 # build the shims and the engine
make doctor          # see what else is worth installing
make demo            # crash an unmodified redis; watch its state come back
make verify          # prove determinism across redis, memcached, nginx, and node
```

`make verify` records each server, kills it, waits out a real-time gap, replays,
and compares — alongside a control run without OneBarrier that must differ:

```
redis     live/replay 1782054868.424071 == 1782054868.424071  | control 1782054874.139431  ✅
memcached live/replay STAT time 1782054875 == 1782054875       | control STAT time 1782054880  ✅
nginx     live/replay Date 15:14:41 GMT == 15:14:41 GMT        | control Date 15:14:46 GMT  ✅
node      live/replay {"now":1782054887981} == ...887981       | control {"now":1782054893724}  ✅
```

Then read [Getting started](docs/getting-started.md), and
[Use it on your app](docs/your-app.md) for the fit test that says whether your
own program is a candidate.

## Scope — please read before deploying

This is a research prototype that works, not a production service.

- **Durability is in-memory.** State is replicated to *f*-of-*k* peers, which
  survives crashes, not power loss. This is the FaRM/RAMCloud tradeoff, chosen
  deliberately: it is what makes the cost disappear.
- **The free-fault-tolerance result needs the network.** The 4.59 µs figure
  assumes an in-network total-order fabric. The `LD_PRELOAD` determinism layer
  runs anywhere; the operating-point argument does not.
- **Your program must fit.** It has to be deterministic given its input order,
  share-nothing or shardable, socket-based, and bounded in output.
  [The fit test](docs/your-app.md) is three questions long.
- **Not every app needs the whole stack.** memcached needs four flags to disable
  its timer-driven maintenance threads; Node needs ASLR off; HAProxy needs an
  explicit date header. These are documented, not hidden.

## Repository layout

| Path | Contents |
|---|---|
| `bin/onebarrier` | The command-line tool |
| `interpose/` | The `LD_PRELOAD` determinism layer and per-application harnesses |
| `crates/onebarrier/` | Rust engine: protocols, recovery, benchmarks, checkers |
| `spec/` | TLA+ specifications, machine-checked |
| `docs/` | Guides, and `docs/research/` for the full experimental record |
| `paper/` | Paper source and figure generation |
| `website/` | Interactive companion site |

## Where this came from

This paper began as a draft written during the author's internship at Microsoft
Research in 2017. The idea was there; finishing it was not something the author
managed to do. It stayed a draft for nine years.

What changed was not the idea. It was that large language models made it possible
for one person to carry a systems paper all the way to completion — the planning,
the implementation, the fifteen application harnesses, the experiments, and the
writing. The work was done through voice-directed
[whisper coding](https://19pine.ai): the author specifies, discusses, and reviews
by speaking, while a coding agent carries it out.

Nine years is a long time for a draft to wait. It seems worth saying plainly that
it did not have to wait for a better idea.

## Citing

> Bojie Li. "OneBarrier: What a Network Must Provide for Transparent Fault
> Tolerance to Be Free." arXiv:2608.14601, 2026.

Machine-readable metadata is in [`CITATION.cff`](CITATION.cff).

## Contributing

Issues and pull requests are welcome — see [CONTRIBUTING.md](CONTRIBUTING.md).
The most useful contribution is a new application harness: if you get a program
recovering byte-identically that is not on the list above, that is a real result.

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
