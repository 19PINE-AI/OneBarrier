# obpreload — transparent interception (Track B)

OneBarrier's libOS-style interception point, in user space, with **no kernel and
no application changes** — the SocksDirect lineage. `obpreload.c` is an
`LD_PRELOAD` shim that interposes libc socket I/O (`accept`/`accept4`,
`read`/`recv`, `close`) on an **unmodified binary** and tees the inbound request
bytes of every accepted connection into a OneBarrier capture log. `ob-replay`
groups the captured stream by connection and replays it against a fresh instance,
rebuilding state after a crash.

## Demo: transparent FT for unmodified `redis-server`

```bash
cargo build --release -p onebarrier --bin ob-replay
bash interpose/demo.sh
```

Output (reproduced 2026-06-21) — redis-server has **zero knowledge** of OneBarrier:

```
before crash: DBSIZE=7 name=OneBarrier hits=2
intercepted:  413 bytes captured transparently
== CRASH (kill -9) ==
fresh:        DBSIZE=0 name=        <- state lost
== replay the intercepted request stream ==
after replay: DBSIZE=7 name=OneBarrier hits=2
keys: hits key1 key2 key3 key4 key5 name
```

## What this is, and isn't (honest scope)

- **Is:** genuine transparent interception — the unmodified server is recorded and
  recovered without a single line of change, demonstrating the core of the
  transparent vision. Works for deterministic request/response state (KV ops).
- **Isn't (yet):** it captures the network input stream but not *all*
  non-determinism — `gettimeofday`/`rand`/thread scheduling are not yet
  virtualized, and replay is single-stream per connection, so a multi-threaded
  app with internal races or time/RNG-dependent state would not replay bit-identically.
  Closing that gap (intercepting the time/RNG syscalls, integrating with the
  fabric so replay rides the total order) is the libOS's remaining work — see
  `docs/PLAN.md` §4–5. The native servers (`ob-kv`, `ob-mc`) already get the full
  in-engine treatment (durable log + snapshot + exactly-once); this shim brings
  the *unmodified*-binary case as close to that vision as user-space interposition
  allows.

## Build

```bash
gcc -shared -fPIC -O2 -o interpose/libobpreload.so interpose/obpreload.c -ldl -lpthread
OB_CAPTURE=/tmp/cap.log LD_PRELOAD=$PWD/interpose/libobpreload.so <server> ...
```
