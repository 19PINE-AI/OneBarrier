# How it works

No jargon required. The paper is the rigorous version; this is the intuition.

## The problem, stated carefully

To bring a dead program back, you need to know what it was doing. The obvious
answer is to write everything down — every request, every so often a snapshot of
memory — and after a crash, restore the snapshot and re-apply what came after.

That is deterministic replay, and it is decades old. It has three costs, and
every one of them lands on the critical path of a live request:

1. **Recording arrival order.** Two clients send at once. Whichever the server
   handles first determines the outcome, so the order has to be written down
   before the server acts on it.
2. **Coordinating a snapshot.** Take a snapshot of a distributed system at the
   wrong moment and you capture a state that never existed — a message received
   but not sent. Avoiding that takes coordination.
3. **Holding replies until the state is durable.** You must not tell a client
   "your money moved" and then lose the record. This is the *output-commit*
   problem, and it means every reply waits for a durable write.

Pay those and transparent fault tolerance is slow. That is roughly the history of
the field: it works, nobody deploys it.

## The claim: two of the three costs belong to the network

OneBarrier's argument is that these are not costs of fault tolerance. They are
costs of running on a network that promises nothing — one that may reorder your
messages and may drop them.

Suppose the network guarantees three things:

- **Order.** Every receiver sees messages in one global sequence.
- **Barrier.** Delivery is confirmed by a commit barrier — the network already
  performs a two-phase handshake to know a message landed.
- **Durability.** A message is replicated to backups before its barrier completes.

Then the three costs evaporate, one by one:

| Cost | Why it disappears |
|---|---|
| Recording order | There is nothing to record. The network *is* the order, and every replica sees the same one. |
| Coordinating a snapshot | With ordered delivery there is a moment when no message is in flight. An empty channel is already a consistent cut — no coordination needed. |
| Holding replies | The barrier the reply is waiting for is a barrier the network was going to cross anyway. The durable write rides along inside it. |

That last row is the whole result. Fault tolerance stops being extra work and
becomes a byproduct of work already happening. Hence the name: **one barrier**,
serving both purposes.

The measurement that settles it: a durable write placed *inside* the barrier
costs **4.59 µs**. The same write placed *after* it costs **2963 µs** — 645×
more. Same durability, same guarantee. Only the placement differs.

This requires a network that actually offers those guarantees.
[1Pipe](https://doi.org/10.1145/3452296.3472909) is one, an in-network total-order
fabric with microsecond round trips.

## The fourth condition, and why this repo exists

Three conditions belong to the network. The fourth belongs to the machine:
**determinism**. Feeding a program the same inputs in the same order only rebuilds
its state if the program is a function of those inputs.

Real servers are not. They have private inputs no request log records:

- They **read the clock**. Redis stamps entries with it, nginx formats a `Date:`
  header from it, a microservice mints timestamped order IDs.
- They **draw randomness**. Hash seeds, session tokens, `Math.random()`, the
  order `SPOP` returns.
- They **let the OS schedule threads**. Whichever thread wins a lock changes what
  the state becomes.

Replay a request log and none of these line up. The data matches; everything
derived from the hidden inputs does not. This is the wall transparent fault
tolerance has kept hitting.

OneBarrier virtualizes all three in user space.

### Time — a virtual clock

The natural approach is record/replay: log every value the clock returned, hand
them back in order. It works if the program reads the clock once per request. It
breaks the moment the program has an internal timer — Redis's `serverCron`,
nginx's `ngx_time_update` — because the timer fires a different number of times
during replay, the cursor into the log slips by one, and every value after that
is wrong.

The virtual clock has no cursor to slip. Time is defined as `base + ticks`, where
ticks advance on each **input event**. Time becomes a function of the inputs, so
it does not matter who reads it, how often, or in what order:

```
LIVE:   1782053569.269446 .270446 .271446 .272446 .273446 .274446
REPLAY: 1782053569.269446 .270446 .271446 .272446 .273446 .274446   (3 s real gap)
```

The replayed process is not told that ten seconds passed. As far as it can tell,
none did.

One refinement matters in practice: a fixed tick makes virtual time drift from
the wall clock. So the live run also logs the *real* interval between inputs, and
replay advances by those recorded deltas. Recovery is then both deterministic and
wall-clock-faithful.

### Randomness — trapping the syscall

`LD_PRELOAD` replaces symbols, so it catches `rand()` and friends. It does not
catch a program that issues the `getrandom(2)` syscall directly — which is
exactly what V8, OpenSSL, and `arc4random` do when seeding.

So `librngdet.so` installs a **seccomp user-notification** filter: the syscall is
trapped in the kernel, and a supervisor thread fills the caller's buffer from a
deterministic stream seeded from a saved file. Live and replay observe identical
randomness.

Two entropy sources need separate handling, and it is worth knowing why:

- **ASLR.** V8 folds addresses into its seed, so addresses must be pinned —
  `setarch -R`.
- **RDRAND.** A CPU instruction. No syscall trap can see it; there is no
  interposition point. It has to be *disabled*, via `OPENSSL_ia32cap`, so OpenSSL
  falls back to the trapped `getrandom`.

The RDRAND case is a genuine limitation, not an oversight: a hardware instruction
that returns entropy directly to userspace cannot be virtualized from userspace.

### Threads — and the measurement that changed the recommendation

`libdetsched.so` implements Kendo-style deterministic scheduling: a thread may
take a top-level lock only when its logical clock is the global minimum. Lock
order becomes a function of the program, not of OS timing. It works, including
under a real `memcached -t 4`.

It is also **over 1000× slower** on a contended multithreaded server. Its
spin-based turn gating serializes every critical section. That is not a bug in
this implementation — Kendo, dthreads, and CoreDet all report the same shape.

So the honest recommendation is not to use it. Use **share-nothing sharding**:
run N single-threaded instances. One thread means no interleaving to make
deterministic — determinism by construction, at zero cost.

The pleasant surprise is that this is *also faster*:

| memcached configuration | Throughput | Deterministic? |
|---|---:|---|
| `-t 1` (single thread) | 342 k ops/s | ✅ by construction |
| `-t 4` (four threads) | 821 k ops/s | ✗ needs `detsched`, which collapses it |
| **4 × `-t 1` shards** | **1.0 M ops/s** | ✅ by construction |

Four single-threaded shards beat one four-threaded process, because shards do not
contend for locks. Single-threaded is not a throughput ceiling — it is how Redis,
nginx workers, and sharded memcached already scale. `detsched` stays in the tree
as the fallback for genuinely shared mutable state.

## Putting it together

```
   record                                      recover
   ──────                                      ───────
   requests ──┐                                ┌── same requests, same order
   clock   ───┼── virtualized ── captured ─────┼── same clock base + deltas
   randomness ┘                                └── same random stream
                                                        │
                                                        ▼
                                            byte-identical state
```

The virtualized inputs are what make the request log sufficient. Without them a
request log rebuilds the data and nothing derived from it.

## What this does not do

- **Durability is in-memory.** Replication to *f*-of-*k* peers survives crashes,
  not power loss. That is the FaRM/RAMCloud tradeoff, taken deliberately: it is
  what puts the durable write inside the barrier instead of after it.
- **The free-fault-tolerance result needs the fabric.** The determinism layer
  runs on any Linux box. The 4.59 µs figure does not.
- **Not every program qualifies.** See [the fit test](your-app.md).
- **RDRAND cannot be virtualized**, only disabled.

## Going deeper

- [Measured results](research/RESULTS.md) — every claim with its command
- [Paper](https://arxiv.org/abs/2608.14601) — the formal argument
- [`spec/`](../spec/) — TLA+ specifications, machine-checked
- [`interpose/README.md`](../interpose/README.md) — implementation notes
