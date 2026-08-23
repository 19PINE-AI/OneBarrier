# Your app

Whether OneBarrier can make your program crash-recoverable, and what to do about it.

## Fit test

Four questions, all have to be yes.

**Is it deterministic given its input order?** Same requests in the same order produce
the same state, assuming time and randomness are handled, which OneBarrier handles. What
breaks this is reaching outside your own inputs: reading a file another process is
writing, calling an external API whose answer varies, depending on `/proc`. Reading the
clock and drawing randomness are fine. That's the whole point.

**Is it share-nothing, or can you shard it?** One thread per unit of state. Redis is
single-threaded, nginx uses share-nothing worker processes, memcached is happy at `-t 1`.
If you have genuinely shared mutable state across threads you're in the expensive case:
`libdetsched.so` will make it deterministic and cost you 1000x on a contended workload.
Shard instead. Four single-threaded memcached shards beat one four-threaded process
anyway.

**Does its state arrive over sockets?** The capture layer hooks `accept`, `read`,
`recv`, and for UDP `recvfrom` and `recvmsg`. State arriving another way (a config file
rewritten at runtime, shared memory from a sibling, a mounted volume) isn't captured and
won't be replayed.

**Is its output bounded?** Recovery replays the whole request stream, so it has to be a
stream worth keeping. Checkpointing bounds this, see below.

If one of these is a no, the whole-process checkpoint path may still work. It assumes
nothing about determinism because it doesn't replay anything: CRIU dumps the process and
restores it. That's how PostgreSQL (multi-process, SysV shm) and MariaDB
(multi-threaded, shares everything) are covered here. Coarser and heavier, but it works
on anything.

## Porting

### 1. Find the time-dependent output

You need a probe: something observable whose value comes from the clock. It's how you
prove recovery worked.

| app | probe |
|---|---|
| redis | `TIME` |
| memcached | `stats time` |
| nginx, lighttpd, HAProxy | HTTP `Date:` header |
| Node.js | `Date.now()` |
| Redis Streams | auto-generated entry IDs (`<ms>-<seq>`) |
| dnsmasq | remaining TTL on a cached record |
| Mosquitto | `$SYS/broker/uptime` |
| SQLite | `strftime('now')` in a row |
| yours | a created-at column, a generated ID, a log line |

### 2. Make it single-threaded

Find the flag. redis already is, nginx wants `worker_processes 1`, memcached wants `-t 1`
plus four more (below). Scale with shards later, not threads.

### 3. Record, crash, replay, control

```bash
onebarrier run --session myapp -- ./my-server --port 8080
# drive a workload that moves the probe, save the output
kill -9 <pid>; sleep 5
onebarrier recover --session myapp --target 127.0.0.1:8080 -- ./my-server --port 8080
# read the probe again, it should match
```

Then run the control: the same thing with no OneBarrier. It has to differ. Without that
half you can't tell determinism from a test that would have passed anyway. Every harness
in `interpose/` has one.

### 4. Chase the leftovers

If replay doesn't match, what differs tells you where to look.

**Timer-driven maintenance threads.** A background thread mutating shared state on a
real-time schedule. memcached does this even at `-t 1` (LRU maintainer, LRU crawler,
hash expander, slab reassigner) and the bookkeeping diverges run to run:
`lru_maintainer_juggles` was 233 one run and 187 the next. Turn them off:

```
memcached -t 1 -o no_lru_crawler,no_lru_maintainer,no_hashexpand,no_slab_reassign
```

Symptom is eviction, expiry, or stats differing while the data matches.

**Randomness going around the shim.** Two known routes. The raw `getrandom(2)` syscall,
which `librngdet.so`'s seccomp filter catches and `onebarrier run` enables by default.
And `/dev/urandom` opened through `fopen`, which is glibc-internal and misses both symbol
interposition and the syscall trap. Redis 6 seeds its SipHash dict that way, which made
`SPOP` nondeterministic. Fix it with a private mount namespace and a deterministic file
bind-mounted over `/dev/urandom` (`interpose/ob-redis-rng.sh`), or set `OB_VRAND_OPENAT=1`
for the syscall-level redirect.

**ASLR and RDRAND.** V8 folds addresses into its seed and OpenSSL will use RDRAND, which
no syscall trap can intercept. `onebarrier run` handles both with `setarch -R` and
`OPENSSL_ia32cap` unless you passed `--no-rng`.

**A runtime with its own entropy path.** Some runtimes seed a PRNG from something that's
neither a libc symbol nor `getrandom(2)`, so both layers miss it. V8 is the example:
`Math.random()` comes from V8's own system-random path, which on x86 goes through
RDRAND. There's no interposition point, so you fix it at the runtime's layer with
`node --random-seed=<fixed>`, a launch flag in the same spirit as `setarch -R`. Save the
seed with the session so live and replay build the same stream.

**Fork per request.** The tick counter is per process, so forking per connection resets
it. dnsmasq forks per TCP query but handles UDP in its main process, which is why its
harness uses UDP.

**Output the app doesn't emit.** HAProxy won't send a `Date:` header unless told to; its
harness adds `hdr date "%[date(0),http_date]"`. If your probe isn't in the output, add
it.

### 5. Bound recovery with checkpoints

Replaying from process start costs O(everything). A checkpoint bounds it to O(tail).
Replay runs around 1.9 M requests/s, so a 100k-request tail is about 87 ms, which gives
you a downtime target to pick a checkpoint interval against.

App-native: redis RDB plus tail replay, with the virtual clock resuming via
`OB_VCLOCK_TICKS` (`interpose/ob-checkpoint-replay.sh`). Any binary: CRIU dumps the whole
process including the in-memory virtual clock, so restore needs no replay at all
(`interpose/ob-criu-kvm.sh`, needs CRIU 3.19+, the harness builds it).

## Examples to copy

Every harness in `interpose/` is a complete example with a control.

| your case | start from |
|---|---|
| HTTP service | `ob-microservice.sh`, Python `http.server`, exactly-once orders, random IDs |
| web server | `ob-webdate.sh`, lighttpd and HAProxy on the `Date:` axis |
| message broker | `ob-redis-streams.sh`, `ob-kafka-partitions.sh`, `ob-mqtt.sh` |
| embedded DB | `ob-sqlite.sh`, SQLite's own clock and PRNG |
| shared-everything DB | `ob-criu-mariadb.sh`, `ob-criu-postgres-kvm.sh` |
| network function | `ob-clicknf.sh`, stateful L4 LB with conntrack timestamps |
| sharded scale-out | `ob-kafka-partitions.sh`, 7.9x at 8 partitions |

## Contributing one

A program recovering byte-identically that isn't on the list is a real result and the
most useful thing you can contribute. A harness needs four parts: record under
`onebarrier run` driving a workload that moves the probe, `kill -9` and a few seconds of
real gap, replay and diff against live, and a control with no OneBarrier that differs.

Pass means `replay == live` and `control != live`. See [CONTRIBUTING.md](../CONTRIBUTING.md).
