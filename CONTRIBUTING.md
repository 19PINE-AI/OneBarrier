# Contributing

## Best thing you can add

A new application harness. If you get a program recovering byte-identically that isn't
already on the list, that's a real result and it extends the paper's generality claim.
It also has the clearest bar for acceptance.

[docs/your-app.md](docs/your-app.md) is the porting guide. A harness needs four parts:

1. Record the app under the determinism stack, driving a workload that moves a time- or
   randomness-dependent probe.
2. Kill it with `kill -9`, then wait a few seconds of real time.
3. Replay on a fresh instance and diff against the live output.
4. A control with no OneBarrier, which has to differ.

The control isn't optional. Without it a pass can't be told apart from a test that would
have passed anyway. Every harness in `interpose/` has one and a PR without one will get
asked for one.

A passing harness prints both halves:

```
live   : <probe value>
replay : <probe value>          <- identical
control: <different value>      (real time)
RESULT: <app> DETERMINISTIC
```

Name it `interpose/ob-<app>.sh`, source `ob-common.sh` and call `ob_require_shims`, and
add a section to [docs/research/RESULTS.md](docs/research/RESULTS.md) with the real
output of your run.

## Other useful work

On the engine (`crates/onebarrier/`): automated primary promotion is the big one. It's
listed as a limitation because a view change needs consensus, and a production-grade
implementation would close the recovery window that passive replication opens. Anything
that strengthens the crash and convergence tests over the live fabric is also welcome.

Closing a residual nondeterminism source. If you find an entropy channel the shims miss,
a reproduction is worth having even without a fix.

Reducing interception overhead, particularly the capture path's synchronous `fwrite`.

Extending the TLA+ specs in `spec/`.

Documentation, especially if something in the guides was wrong or confusing when you
followed it. Say what you expected and what happened.

## Setup

```bash
git clone https://github.com/19PINE-AI/OneBarrier.git
cd OneBarrier
make
make doctor
make test
make verify
```

Linux is required for anything touching the shims. The Rust engine and its tests build
anywhere.

## Before a PR

```bash
make test
bash -n interpose/*.sh
make verify        # if you touched interpose/
```

CI runs the Rust suite, shell syntax, a warning-free shim build, the end-to-end demo,
and the website build.

On `cargo fmt` and `cargo clippy`: the codebase satisfies neither right now, and
`cargo fmt --all` would reformat 34 files. Please don't reformat the tree as part of an
unrelated PR, it buries the change and touches code the results run on. Formatting the
codebase and clearing the clippy backlog is welcome as a PR that does nothing else.

## Style

**Claims come with a command.** This is the one firm rule and it's why the results are
worth anything. If you add a number to a document, the command that produced it goes
next to it. If you can't reproduce it, don't assert it.

**Negative results stay.** The deterministic scheduler's 1000x collapse, record/replay
failing on timer-driven servers, the CRIU version blocker: those are documented on
purpose. If your change makes something measurably worse, report that too. It's more
useful than a silent regression.

**Say when something is simulated** rather than measured, or inherited from 1Pipe rather
than shown here, somewhere a reader will see it.

**Comments explain why.** There are several places where the obvious approach is wrong:
`pthread_cond_wait` has to be bound via `dlvsym` to `GLIBC_2.3.2` or threaded servers
hang, only depth-0 lock acquisitions can be gated or nested locking deadlocks. Those
comments are load-bearing. Match that: explain the non-obvious, skip the obvious.

## Bug reports

A determinism failure is the most interesting kind. Include the app and its exact launch
command with flags, the output of `onebarrier doctor`, the live/replay/control probe
values, and `uname -a`.

A divergence where the control also differs usually means a leftover nondeterminism
source. The list in
[docs/your-app.md](docs/your-app.md#4-chase-the-leftovers) covers the known ones.

## Security

Don't open a public issue for a security problem, see [SECURITY.md](SECURITY.md).

## License

Contributions are MIT or Apache-2.0, matching the project.
