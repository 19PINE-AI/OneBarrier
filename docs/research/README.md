# Research record

The experimental material behind the paper. Nothing here has been trimmed for the
open-source release: the results file is the same source of truth the paper was
written from, including the caveats and the negative results.

| File | Contents |
|---|---|
| [`RESULTS.md`](RESULTS.md) | Every measured result with the command that reproduces it, plus the claims ledger mapping each research question to its evidence |
| [`PLAN.md`](PLAN.md) | The research plan, including the self-critical novelty analysis and the prior art that anticipated the original pitch |
| [`PAPER-PLAN.md`](PAPER-PLAN.md) | The path from validated reproduction to paper, and what is simulated versus measured |

Also part of the record:

- [`../../paper/`](../../paper/) — LaTeX source, figures, and the scripts that generate them
- [`../../spec/`](../../spec/) — TLA+ specifications and TLC configurations
- [`../../website/`](../../website/) — the interactive companion site
- [Paper](https://arxiv.org/abs/2608.14601) — arXiv:2608.14601

## Reading the results honestly

Three distinctions run through `RESULTS.md`, and they are labelled there:

**Measured, simulated, or inherited.** No RDMA testbed was available. Results at
the RDMA operating point are simulated using the latency model from the 1Pipe
paper and are marked as such; the reproduction runs on loopback UDP, where the
*shape* of the result transfers but the absolute microseconds do not. Where a
property is inherited from 1Pipe's hardware rather than demonstrated here, that
is stated.

**Durability tier.** The near-zero marginal cost is in-memory *f*-of-*k*
fail-stop durability — crash-safe, not power-loss-safe. The `fsync` tier is
measured alongside it and costs ~3 ms per operation. Both numbers are reported;
the tier is always named.

**Negative results are kept.** The deterministic scheduler's >1000× collapse on a
contended multithreaded server, the record/replay strategy's failure on
timer-driven servers, the CRIU version blocker — these are in the record because
they are what shaped the design.

## Reproducing

```bash
make test                       # Rust suite, including crash and recovery tests
make verify                     # determinism across four unmodified servers
cargo run --release -p onebarrier --bin ob-bench      # the 4.59 µs vs 2963 µs result
bash interpose/ob-perf.sh       # overhead decomposition
bash interpose/ob-recovery-time.sh                    # recovery latency vs log length
```

Each section of `RESULTS.md` names its own command. Experiments involving RDMA,
CRIU, or KVM need extra system dependencies and sometimes privileges; those
prerequisites are noted per experiment.
