# Research record

The experimental material behind the paper. Nothing was trimmed for the open-source
release: `RESULTS.md` is the same source of truth the paper was written from, caveats and
negative results included.

| file | contents |
|---|---|
| [RESULTS.md](RESULTS.md) | every measured result with the command that reproduces it, plus the claims ledger mapping each research question to its evidence |
| [PLAN.md](PLAN.md) | the research plan, including the self-critical novelty section and the prior art that anticipated the original pitch |
| [PAPER-PLAN.md](PAPER-PLAN.md) | the path from validated reproduction to paper, and what's simulated versus measured |

Also part of the record: [`paper/`](../../paper/) (LaTeX source, figures, and the scripts
that make them), [`spec/`](../../spec/) (TLA+ and TLC configs), [`website/`](../../website/),
and the [paper](https://arxiv.org/abs/2608.14601) itself.

## Reading the results

Three distinctions run through `RESULTS.md` and they're labelled there.

Measured, simulated, or inherited. There was no RDMA testbed, so results at the RDMA
operating point are simulated using the latency model from the 1Pipe paper and marked as
such. The reproduction runs on loopback UDP where the shape of the result transfers but
the absolute microseconds don't. Where a property is inherited from 1Pipe's hardware
rather than shown here, that's stated.

Durability tier. The near-zero marginal cost is in-memory f-of-k fail-stop durability,
crash-safe but not power-loss-safe. The fsync tier is measured alongside it and costs
about 3 ms per op. Both are reported and the tier is always named.

Negative results are kept. The deterministic scheduler's 1000x collapse on a contended
server, record/replay failing on timer-driven servers, the CRIU version blocker: they're
in the record because they're what shaped the design.

## Reproducing

```bash
make test                                             # Rust suite, crash and recovery tests
make verify                                           # determinism across four servers
cargo run --release -p onebarrier --bin ob-bench      # the 4.59 vs 2963 µs result
bash interpose/ob-perf.sh                             # overhead decomposition
bash interpose/ob-recovery-time.sh                    # recovery latency vs log length
```

Each section of `RESULTS.md` names its own command. Anything involving RDMA, CRIU, or KVM
needs extra system dependencies and sometimes privileges, noted per experiment.
