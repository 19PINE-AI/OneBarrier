# OneBarrier

Kill a server process, start it again, and it comes back byte-identical: same data,
same timestamps, same random IDs it had already handed out. The server is unmodified.
No patch, no rebuild, no library to link, no kernel module, no root.

[Paper](https://arxiv.org/abs/2608.14601) ·
[Site](https://01.me/research/OneBarrier) ·
[Getting started](docs/getting-started.md) ·
[How it works](docs/how-it-works.md) ·
[Your app](docs/your-app.md) ·
[Results](docs/research/RESULTS.md)

## What it does

Making a program survive a crash normally means rewriting it: add a write-ahead log,
add checkpoints, replicate the state, then reason carefully about which replies you're
allowed to send before which writes are durable. It's invasive, easy to get subtly
wrong, and you do it again for every program.

OneBarrier does it from outside the process.

```bash
onebarrier run --session app -- redis-server --port 6379
# kill -9 it, wait as long as you want
onebarrier recover --session app --target 127.0.0.1:6379 -- redis-server --port 6379
```

Redis is never told any of this happened. After recovery, `GET` returns what it
returned before, `INCR` counters aren't double-applied, and `TIME` reports the instant
of the crash instead of now.

## Why replaying requests isn't enough

Replaying inputs only rebuilds state if the program is a function of its inputs, and
real servers aren't. They read the clock, they draw randomness, and they let the OS
decide which thread gets a lock first. None of that is in a request log, so a naive
replay gives you a server that agrees on the data and disagrees on everything derived
from it.

OneBarrier virtualizes those three, each as a separate `LD_PRELOAD` library you can use
on its own:

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
deterministic lock order costs over 1000x on a contended server. So don't do that. Run
N single-threaded instances instead, which are deterministic because there's only one
thread. That's also faster: 4 single-threaded memcached shards do 1.0 M ops/s against
821 k for one `memcached -t 4`, because shards don't fight over locks.

## Numbers

All of these are reproduced by a command in [docs/research/RESULTS.md](docs/research/RESULTS.md).

The paper's main result is that where you put the durable write is what costs you. Same
write, same guarantee, two placements:

| durable write | cost per request |
|---|---|
| inside the network's commit barrier | 4.59 µs (0.23% of delivery latency) |
| serial `fsync` after it | 2963 µs, and throughput collapses |

A fault-tolerant Redis-protocol server against stock Redis with no persistence and no
fault tolerance at all:

| | SET | GET | INCR |
|---|---:|---:|---:|
| Redis, not fault-tolerant | 239 234 | 244 499 | 238 095 |
| OneBarrier, fault-tolerant | 142 248 | 188 679 | 233 100 |
| | 59% | 77% | 98% |

Recovery is linear in log length, about 1.9 M requests/s, and reconstructs the key set
exactly:

| requests | keys recovered | time |
|---:|---:|---:|
| 10 k | 9 998 / 9 998 | 35 ms |
| 100 k | 99 957 / 99 957 | 87 ms |
| 1 M | 994 967 / 994 967 | 536 ms |

Interception overhead is under 5% for nginx under realistic (non-pipelined) load, with
p99 unchanged at 2 ms. Time and RNG virtualization are within noise.

Fifteen unmodified applications recover byte-identically: redis, memcached, nginx,
Node.js, PostgreSQL, MariaDB, SQLite, Redis Streams, a Kafka partition model, Mosquitto,
dnsmasq, lighttpd, HAProxy, a Click network function, and a Python microservice. Each is
checked against a control run without OneBarrier that has to differ, so a pass isn't
just a test that would have passed anyway.

Some of what survives a crash unchanged: nginx's `Date:` header, formatted deep inside
nginx from its own cached clock. memcached's LRU eviction set, all 1416 survivors.
Node's `Math.random()`. SQLite rows whose values came from SQLite's own PRNG. A load
balancer's per-flow conntrack timestamps.

The engine protocols are model-checked in TLA+ (3.5M states, no violation), and crash
injection with real `kill -9` gives linearizable exactly-once histories.

## The argument

Transparent fault tolerance has been worked on for forty years without reaching
production. Every attempt paid three costs on the critical path: record message arrival
order so you can replay it, coordinate a consistent snapshot, and hold each reply until
the state behind it is durable.

The paper's claim is that these aren't costs of fault tolerance. They're the price of a
network that guarantees neither order nor delivery. Give the network three properties
(messages delivered in one global order, delivery confirmed by a commit barrier, each
message replicated to backups before its barrier completes) and all three costs go away.
There's no order to record because the network is the order. There's no snapshot to
coordinate because an empty channel is already a consistent cut. And the barrier the
reply was waiting for is one the network was going to cross anyway.

Three of those conditions are the network's job, and OneBarrier gets them from
[1Pipe](https://doi.org/10.1145/3452296.3472909). The fourth, determinism, is the host's
job, and that's the `LD_PRELOAD` layer here. It costs 2-10% and needs no special
hardware.

## Try it

Linux, `gcc`, Rust 1.85+.

```bash
git clone https://github.com/19PINE-AI/OneBarrier.git
cd OneBarrier
make
make doctor    # tells you what else is worth installing
make demo      # kills an unmodified redis and brings its state back
make verify    # redis, memcached, nginx, node, each with a control
```

`make verify` records each server, kills it, waits out a real-time gap, replays, and
compares against a control run with no OneBarrier:

```
redis     live/replay 1782054868.424071 == 1782054868.424071  | control 1782054874.139431  OK
memcached live/replay STAT time 1782054875 == 1782054875      | control STAT time 1782054880  OK
nginx     live/replay Date 15:14:41 GMT == 15:14:41 GMT       | control Date 15:14:46 GMT  OK
node      live/replay {"now":1782054887981} == ...887981      | control {"now":1782054893724}  OK
```

## Limitations

Durability is in-memory. State goes to f-of-k peers, which survives crashes but not
power loss. Same tradeoff as FaRM and RAMCloud, and it's what makes the cost disappear.

The 4.59 µs number needs the fabric. The determinism shims run on any Linux box; the
operating-point result doesn't.

Your program has to fit: deterministic given input order, share-nothing or shardable,
socket-based, bounded output. The fit test is in [docs/your-app.md](docs/your-app.md).

Some apps need flags. memcached needs four to turn off its timer-driven maintenance
threads, Node needs ASLR off, HAProxy needs an explicit date header. They're documented,
not hidden.

RDRAND can't be virtualized from userspace, only disabled.

This is a research prototype. It works, but it isn't a production service.

## Layout

| path | contents |
|---|---|
| `bin/onebarrier` | the CLI |
| `interpose/` | the determinism shims and the per-app harnesses |
| `crates/onebarrier/` | Rust engine, protocols, recovery, benchmarks, checkers |
| `spec/` | TLA+ specs |
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
