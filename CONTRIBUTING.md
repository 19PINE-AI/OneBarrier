# Contributing to OneBarrier

## The most valuable contribution

**A new application harness.** If you get a program recovering byte-identically
that is not already on the list, that is a real result, and it extends the paper's
generality claim. It is also the contribution with the clearest bar for
acceptance.

[`docs/your-app.md`](docs/your-app.md) is the porting guide. A harness must have
four parts:

1. **Record** the app under the determinism stack, driving a workload that moves
   a time- or randomness-dependent probe.
2. **Crash** it with `kill -9`, then wait a real-time gap of a few seconds.
3. **Replay** on a fresh instance and `diff` against the live output.
4. **Control** — the same run with no OneBarrier, which *must* differ.

The control is not optional. Without it, a pass cannot be distinguished from a
test that would have passed anyway. Every harness in `interpose/` has one, and a
PR without one will be asked for one.

A passing harness prints both halves of the verdict:

```
live   : <probe value>
replay : <probe value>          <- byte-identical
control: <different value>      (real time)
RESULT: <app> DETERMINISTIC ✅
```

Name it `interpose/ob-<app>.sh`, source `ob-common.sh` and call
`ob_require_shims`, and add a section to
[`docs/research/RESULTS.md`](docs/research/RESULTS.md) with the actual output of
your run.

## Other useful work

- **Closing a residual nondeterminism source.** If you find an entropy channel
  the shims miss, a reproduction is valuable even without a fix.
- **Reducing the interception overhead**, particularly the capture path's
  synchronous `fwrite`.
- **Extending the TLA+ specs** in `spec/` to cover more of the protocol.
- **Documentation** — especially if something in the guides was wrong or
  confusing when you followed it. Say what you expected and what happened.

## Getting set up

```bash
git clone https://github.com/19PINE-AI/OneBarrier.git
cd OneBarrier
make            # shims + engine
make doctor     # what else is worth installing
make test       # Rust suite
make verify     # determinism across four unmodified servers
```

Linux is required for anything involving the shims — they use `LD_PRELOAD` and
seccomp. The Rust engine and its tests build anywhere.

## Before you open a PR

```bash
make test                       # must pass
bash -n interpose/*.sh          # shell syntax
make verify                     # if you touched interpose/
```

CI runs the Rust suite, shell syntax, a warning-free shim build, the end-to-end
demo, and the website build. Those are the gates.

A note on `cargo fmt` and `cargo clippy`: the codebase does not currently satisfy
either, and running `cargo fmt --all` would reformat 34 files. **Please do not
reformat the tree as part of an unrelated PR** — it buries the change under noise
and touches code the paper's results depend on. Formatting the codebase and
clearing the clippy backlog is a welcome contribution *on its own*, as a PR that
does nothing else.

## House style

**Claims are backed by a command.** This is the repository's one firm rule and
the reason the results are trustworthy. If you add a number to any document, the
command that produced it goes next to it. If you cannot reproduce it, do not
assert it.

**Negative results stay in.** The deterministic scheduler's 1000× collapse, the
record/replay strategy's failure on timer-driven servers, the CRIU version
blocker — these are documented on purpose. If your change makes something worse
in a measurable way, report that too; it is more useful than a silent regression.

**Scope is stated, not implied.** Where a result is simulated rather than
measured, or inherited from 1Pipe rather than demonstrated here, say so where a
reader will see it.

**Comments explain why, not what.** The code has several places where the obvious
approach is wrong — `pthread_cond_wait` must be bound via `dlvsym` to
`GLIBC_2.3.2` or threaded servers hang; only depth-0 lock acquisitions may be
gated or nested locking deadlocks. Those comments are load-bearing. Match that
density: explain the non-obvious, skip the obvious.

## Reporting bugs

A determinism failure is the most interesting kind of bug report. Please include:

- The application and its exact launch command, flags included
- Output of `onebarrier doctor`
- The live, replay, and control probe values
- Kernel version and distribution (`uname -a`)

A divergence with a control that also differs usually means a residual
nondeterminism source; the hunting list in
[`docs/your-app.md`](docs/your-app.md#step-4--hunt-the-residual-nondeterminism)
covers the known ones.

## Security

Do not open a public issue for a security problem. See [SECURITY.md](SECURITY.md).

## License

Contributions are dual-licensed under MIT or Apache-2.0, matching the project.
