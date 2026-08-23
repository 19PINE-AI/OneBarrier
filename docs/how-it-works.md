# How it works

The paper has the rigorous version. This is the short one.

## The three costs

To bring a dead process back you need to know what it was doing. Write down every
request, snapshot memory now and then, and after a crash restore the snapshot and
re-apply what came after. That's deterministic replay, and it's decades old.

It has three costs, and all three land on the critical path of a live request:

1. Recording arrival order. Two clients send at once, whichever the server handles
   first decides the outcome, so the order has to be written down before the server
   acts on it.
2. Coordinating a snapshot. Snapshot a distributed system at the wrong moment and you
   capture a state that never existed, like a message received but never sent. Avoiding
   that takes coordination.
3. Holding replies. You can't tell a client "your money moved" and then lose the
   record, so every reply waits on a durable write. This is the output-commit problem.

Pay all three and transparent fault tolerance is slow. That's roughly the history of
the field: it works, nobody ships it.

## Two of them belong to the network

The claim in the paper is that these aren't costs of fault tolerance. They're the cost
of running on a network that promises nothing, one that can reorder your messages and
can drop them.

Suppose the network guarantees three things: every receiver sees messages in one global
order, delivery is confirmed by a commit barrier, and each message is replicated to
backups before its barrier completes. Then:

- There's nothing to record. The network *is* the order, and every replica sees the
  same one.
- There's no snapshot to coordinate. With ordered delivery there's a moment when no
  message is in flight, and an empty channel is already a consistent cut.
- The barrier the reply is waiting on is one the network was going to cross anyway, so
  the durable write rides inside it.

That last one is the result. Fault tolerance stops being extra work and becomes a
byproduct of work already happening, which is where the name comes from.

The measurement: a durable write inside the barrier costs 4.59 µs, the same write after
it costs 2963 µs. Same durability, same guarantee, 645x apart on placement alone.

This needs a network that actually offers those guarantees.
[1Pipe](https://doi.org/10.1145/3452296.3472909) is one, an in-network total-order
fabric with microsecond round trips.

## The fourth condition is the host's problem

Three conditions are the network's. The fourth is determinism, and it's why this repo
exists: feeding a program the same inputs in the same order only rebuilds its state if
the program is a function of those inputs.

Servers aren't. They read the clock (redis stamps entries, nginx formats a `Date:`
header, a microservice mints timestamped order IDs), they draw randomness (hash seeds,
session tokens, `Math.random()`, whatever `SPOP` returns), and they let the OS pick
which thread wins a lock. Replay the request log and the data matches but everything
derived from those hidden inputs doesn't. That's the wall this problem keeps hitting.

### Time

The obvious approach is record/replay: log every value the clock returned, hand them
back in order. That works if the program reads the clock once per request. It breaks as
soon as the program has an internal timer, like redis `serverCron` or nginx
`ngx_time_update`, because the timer fires a different number of times during replay,
the cursor into the log slips by one, and every value after that is wrong.

A virtual clock has no cursor. Time is `base + ticks`, ticks advance on each input
event, so time is a function of the inputs and it doesn't matter who reads it or how
often:

```
LIVE:   1782053569.269446 .270446 .271446 .272446 .273446 .274446
REPLAY: 1782053569.269446 .270446 .271446 .272446 .273446 .274446   (3s real gap)
```

The replayed process isn't told that three seconds passed. As far as it can tell, none
did.

One wrinkle: a fixed tick makes virtual time drift from the wall clock. So the live run
also logs the real interval between inputs and replay advances by those, which keeps
recovery deterministic and roughly wall-clock-faithful.

### Randomness

`LD_PRELOAD` replaces symbols, so it catches `rand()` and friends. It doesn't catch a
program issuing the `getrandom(2)` syscall directly, which is what V8, OpenSSL, and
`arc4random` do when seeding.

So `librngdet.so` installs a seccomp user-notification filter. The syscall gets trapped
in the kernel and a supervisor thread fills the caller's buffer from a deterministic
stream seeded from a saved file, so live and replay see the same bytes.

Two sources need separate handling. ASLR, because V8 folds addresses into its seed, so
addresses get pinned with `setarch -R`. And RDRAND, which is a CPU instruction with no
syscall to trap and no interposition point at all, so it has to be disabled via
`OPENSSL_ia32cap` and OpenSSL falls back to the trapped `getrandom`. The RDRAND case is
a real limitation, not an oversight. You can't virtualize a hardware instruction that
returns entropy straight to userspace from inside userspace.

### Threads

`libdetsched.so` does Kendo-style deterministic scheduling: a thread may take a
top-level lock only when its logical clock is the global minimum, so lock order is a
function of the program instead of OS timing. It works, including under a real
`memcached -t 4`.

It's also over 1000x slower on a contended server, because the spin-based turn gating
serializes every critical section. That's not specific to this implementation. Kendo,
dthreads, and CoreDet all report the same shape.

So the recommendation is not to use it. Shard instead: run N single-threaded instances,
where one thread means there's no interleaving to make deterministic. Determinism for
free.

It's also faster, which I didn't expect:

| memcached | throughput | deterministic |
|---|---:|---|
| `-t 1` | 342 k ops/s | yes, by construction |
| `-t 4` | 821 k ops/s | no, and `detsched` collapses it |
| 4 x `-t 1` shards | 1.0 M ops/s | yes, by construction |

Four single-threaded shards beat one four-threaded process because shards don't contend
for locks. Single-threaded isn't a ceiling, it's how redis, nginx workers, and sharded
memcached already scale. `detsched` stays in the tree for genuinely shared mutable
state.

## Together

```
   record                                      recover
   ------                                      -------
   requests --+                                +-- same requests, same order
   clock -----+-- virtualized -- captured -----+-- same clock base + deltas
   randomness-+                                +-- same random stream
                                                        |
                                                        v
                                            byte-identical state
```

The virtualized inputs are what make the request log sufficient. Without them the log
rebuilds the data and nothing derived from it.

## What it doesn't do

Durability is in-memory: replication to f-of-k peers survives crashes, not power loss.
That's the FaRM/RAMCloud tradeoff, taken on purpose, because it's what puts the durable
write inside the barrier instead of after it.

The 4.59 µs result needs the fabric. The determinism layer runs on any Linux box, the
operating-point argument doesn't.

Not every program qualifies. See [the fit test](your-app.md).

## More

- [Results](research/RESULTS.md), every claim with its command
- [Paper](https://arxiv.org/abs/2608.14601)
- [`spec/`](../spec/), the TLA+ specs
- [`interpose/README.md`](../interpose/README.md), implementation notes
