# Getting started

This walks through making an unmodified `redis-server` survive `kill -9`, then
generalizes. Budget ten minutes.

## Requirements

OneBarrier's determinism layer is Linux-only — it relies on `LD_PRELOAD` and
seccomp. No RDMA, no kernel module, no root.

| | |
|---|---|
| Required | Linux, `gcc`, Rust 1.85+ |
| Recommended | `setarch` (util-linux), `ss` (iproute2) |
| For the demos | `redis-server`, `redis-cli`; then `memcached`, `nginx`, `node`, `curl` |

```bash
git clone https://github.com/19PINE-AI/OneBarrier.git
cd OneBarrier
make
make doctor
```

`make doctor` names every tool it looks for and what you lose without it, so you
can install only what you care about.

Optionally put the tool on your `PATH`:

```bash
export PATH="$PWD/bin:$PATH"
```

## The two-minute version

```bash
make demo
```

An unmodified `redis-server` is started under the shim, populated with real
`redis-cli` commands, killed with `kill -9`, restarted empty, and rebuilt from
the captured request stream. Redis is never told any of this is happening.

## Doing it yourself

### 1. Record

```bash
onebarrier run --session app -- redis-server --port 6379 --save '' --appendonly no
```

The server runs normally. In another shell, give it some state:

```bash
redis-cli -p 6379 SET name OneBarrier
redis-cli -p 6379 INCR hits
redis-cli -p 6379 INCR hits
redis-cli -p 6379 TIME        # note this value
```

Session files land in `~/.onebarrier/app/`: the clock base, the per-input time
deltas, the random seed, and `capture.log`. `onebarrier sessions` lists them.

### 2. Crash it

```bash
kill -9 $(pgrep -f 'redis-server \*:6379')
sleep 10        # let real time move on, so recovery has something to disagree with
```

### 3. Recover

```bash
onebarrier recover --session app --target 127.0.0.1:6379 -- \
  redis-server --port 6379 --save '' --appendonly no
```

This starts a fresh server *in the same session* — same clock base, same random
stream — and pushes the captured requests back into it.

```bash
redis-cli -p 6379 GET name    # OneBarrier
redis-cli -p 6379 GET hits    # 2, not 4 — replay is exactly-once
redis-cli -p 6379 TIME        # the value from step 1, despite the ten-second gap
```

`TIME` is the part worth pausing on. The recovered Redis is not approximately
where it was; it is at the same instant. That is what makes the state
byte-identical rather than merely similar.

## Proving it, with a control

Byte-identical output is only evidence if a run *without* OneBarrier would have
differed. `make verify` checks both directions for four servers:

```bash
make verify
# or one at a time, with a chosen real-time gap:
onebarrier verify redis 5
onebarrier verify nginx 3
```

Each app is run three times — live under the shim, replayed after a crash and a
gap, and a control with no shim at all. A pass requires `replay == live` **and**
`control != live`.

## Command reference

| Command | Purpose |
|---|---|
| `onebarrier doctor` | What is installed, what each thing unlocks |
| `onebarrier build` | Build the shims and the Rust tools |
| `onebarrier run --session N -- CMD` | Run under the determinism stack, capturing input |
| `onebarrier recover --session N --target H:P -- CMD` | Fresh instance in the same session, then replay into it |
| `onebarrier replay --session N --target H:P` | Replay a capture into an already-running instance |
| `onebarrier sessions` | List recorded sessions |
| `onebarrier verify [app] [gap]` | Determinism harness with a control |
| `onebarrier demo` | The scripted redis crash-and-recover |

Useful flags: `--no-capture` (virtualize time and randomness but record nothing),
`--no-rng` (skip seccomp and `setarch`, for apps that draw no randomness).
`OB_HOME` relocates the session directory.

## Under the hood

`onebarrier run` is a wrapper around an environment, and you can set it yourself:

```bash
OB_VCLOCK=~/.onebarrier/app/vclock.base \
OB_VCLOCK_DELTAS=~/.onebarrier/app/vclock.deltas \
OB_VRAND=~/.onebarrier/app/vrand.seed \
OB_CAPTURE=~/.onebarrier/app/capture.log \
OPENSSL_ia32cap='~0x4000000000000000:~0x0' \
LD_PRELOAD='interpose/librngdet.so interpose/libobpreload.so' \
setarch -R redis-server --port 6379
```

The wrapper exists because two details here fail *silently*. `librngdet.so` must
come first in `LD_PRELOAD`, and a library named in `LD_PRELOAD` that does not
exist is ignored by the loader without a word — so a missing shim does not raise
an error, it just produces a run with no determinism and a confusing failure at
the end. The harnesses now build the shims on demand for the same reason.

## Where to go next

- [How it works](how-it-works.md) — the mechanism, in plain terms
- [Use it on your app](your-app.md) — the fit test and the porting recipe
- [Measured results](research/RESULTS.md) — every number, with its command
- [`interpose/README.md`](../interpose/README.md) — the determinism layer in detail
