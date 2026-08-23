# Getting started

Making an unmodified `redis-server` survive `kill -9` on one machine.

That's the determinism layer, which is one of OneBarrier's four conditions and the only
one that needs no special network. The replicated engine (k replicas over a total-order
fabric, one executing and the rest logging) lives in `crates/onebarrier/` and is
exercised by `make test`, not by the commands below. [How it works](how-it-works.md) has
the whole picture.

## Requirements

Linux only for anything involving the shims, since they use `LD_PRELOAD` and seccomp.
No RDMA, no kernel module, no root.

You need `gcc` and Rust 1.85+. `setarch` and `ss` are worth having. For the demos,
`redis-server` and `redis-cli`, then `memcached`, `nginx`, `node`, and `curl` if you
want the rest.

```bash
git clone https://github.com/19PINE-AI/OneBarrier.git
cd OneBarrier
make
make doctor
```

`make doctor` lists every tool it looks for and what you lose without it, so you can
install only what you care about.

Optionally:

```bash
export PATH="$PWD/bin:$PATH"
```

## Quick version

```bash
make demo
```

Starts an unmodified `redis-server` under the shim, fills it using real `redis-cli`,
kills it with `kill -9`, starts an empty one, and rebuilds it from the captured request
stream.

## Doing it by hand

Record:

```bash
onebarrier run --session app -- redis-server --port 6379 --save '' --appendonly no
```

In another shell, give it some state:

```bash
redis-cli -p 6379 SET name OneBarrier
redis-cli -p 6379 INCR hits
redis-cli -p 6379 INCR hits
redis-cli -p 6379 TIME        # note this
```

Session files go to `~/.onebarrier/app/`: clock base, per-input time deltas, random
seed, and `capture.log`. `onebarrier sessions` lists them.

Crash it:

```bash
kill -9 $(pgrep -f 'redis-server \*:6379')
sleep 10
```

The sleep matters. Without a real-time gap you can't tell a working virtual clock from
a lucky one.

Recover:

```bash
onebarrier recover --session app --target 127.0.0.1:6379 -- \
  redis-server --port 6379 --save '' --appendonly no
```

That starts a fresh server in the same session (same clock base, same random stream) and
pushes the captured requests into it.

```bash
redis-cli -p 6379 GET name    # OneBarrier
redis-cli -p 6379 GET hits    # 2, not 4: replay is exactly-once
redis-cli -p 6379 TIME        # the value from before, despite the 10s gap
```

`TIME` is the one to look at. The recovered redis isn't approximately where it was, it's
at the same instant, which is what makes the rest of the state identical rather than
just similar.

## Proving it

Identical output only means something if a run without OneBarrier would have differed.
`make verify` checks both directions on four servers:

```bash
make verify
onebarrier verify redis 5      # one app, 5 second gap
```

Each app runs three times: live under the shim, replayed after a crash and a gap, and a
control with no shim at all. A pass needs `replay == live` and `control != live`.

## Commands

| command | what it does |
|---|---|
| `onebarrier doctor` | what's installed and what each thing unlocks |
| `onebarrier build` | build shims and Rust tools |
| `onebarrier run --session N -- CMD` | run under the stack, capturing input |
| `onebarrier recover --session N --target H:P -- CMD` | fresh instance in the same session, then replay into it |
| `onebarrier replay --session N --target H:P` | replay a capture into a running instance |
| `onebarrier sessions` | list sessions |
| `onebarrier verify [app] [gap]` | determinism harness with a control |
| `onebarrier demo` | the scripted redis crash and recover |

`--no-capture` virtualizes time and randomness without recording anything. `--no-rng`
skips seccomp and `setarch`. `OB_HOME` moves the session directory.

## What the CLI is doing

Just setting up an environment, which you can do yourself:

```bash
OB_VCLOCK=~/.onebarrier/app/vclock.base \
OB_VCLOCK_DELTAS=~/.onebarrier/app/vclock.deltas \
OB_VRAND=~/.onebarrier/app/vrand.seed \
OB_CAPTURE=~/.onebarrier/app/capture.log \
OPENSSL_ia32cap='~0x4000000000000000:~0x0' \
LD_PRELOAD='interpose/librngdet.so interpose/libobpreload.so' \
setarch -R redis-server --port 6379
```

The wrapper exists because two things here fail silently. `librngdet.so` has to come
first in `LD_PRELOAD`, and a library in `LD_PRELOAD` that doesn't exist is ignored by
the loader without any error at all, so a missing shim doesn't fail, it just gives you a
run with no determinism and a confusing result at the end. The harnesses build the shims
on demand for the same reason.

## The replicated engine

Everything above is single-machine. The engine that implements the paper's protocol is
in `crates/onebarrier/`, and `make test` runs it over a live loopback-UDP fabric:

```bash
cargo test -p onebarrier cluster::          # 3 replicas converge; survivors stay correct
cargo run --release -p onebarrier --bin ob-bench    # the 4.59 vs 2963 µs result
cargo run --release -p onebarrier --bin ob-cpu      # passive vs active SMR CPU
```

There's no turnkey way to run a real replicated deployment here. Order, Barrier, and
Durability come from the fabric, and OneBarrier consumes them rather than providing them.

## Next

- [How it works](how-it-works.md)
- [Your app](your-app.md), the fit test and the porting recipe
- [Results](research/RESULTS.md)
- [`interpose/README.md`](../interpose/README.md)
