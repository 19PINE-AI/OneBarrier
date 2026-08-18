# OneBarrier

**Transparent, low-overhead fault tolerance for unmodified share-nothing
servers—as a byproduct of total-order communication.**

[Paper](https://arxiv.org/abs/2608.14601) ·
[Interactive website](https://01.me/research/OneBarrier) ·
[Results and reproduction commands](STATUS.md) ·
[1Pipe](https://doi.org/10.1145/3452296.3472909)

OneBarrier routes the IPC of unmodified servers through the 1Pipe in-network
total-order reliable fabric. The fabric supplies the message order, an
empty-channel uncoordinated timestamp-*T* snapshot, and a reliable-delivery 2PC
commit barrier. OneBarrier makes that commit barrier coincide with the
output-commit barrier, so fault tolerance adds almost no marginal cost at
1Pipe's microsecond operating point.

> **Scope.** This is a research prototype and a co-design, operating-point, and
> measurement result—not a new fault-tolerance primitive or production-ready
> service. Durability is in-memory *f*-of-*k* fail-stop durability, not
> power-loss-safe storage. The [paper](https://arxiv.org/abs/2608.14601) and
> [`docs/PLAN.md`](docs/PLAN.md) give the full assumptions and related-work
> analysis.

## Repository layout

| Path | Contents |
|---|---|
| `crates/onebarrier/` | Rust engine, protocols, recovery, benchmarks, and checkers |
| `interpose/` | Determinism libOS and unmodified-application harnesses |
| `spec/` | TLA+ specifications for the engine and total-order fabric |
| `paper/` | Paper source and figure-generation inputs |
| `website/` | Interactive React paper companion |
| `STATUS.md` | Measured results and their reproduction commands |

## Quick start

The core artifact requires Git, Rust 1.85 or newer, and a Linux or macOS host.
Cargo fetches the pinned public 1Pipe dependency automatically.

```bash
git clone https://github.com/19PINE-AI/OneBarrier.git
cd OneBarrier
cargo test --workspace --all-targets
cargo run -p onebarrier --bin ob-demo
```

To build the website, use Node.js `^20.19.0` or `>=22.12.0`:

```bash
cd website
npm ci
npm run lint
npm run build
```

The transparent application, RDMA, and checkpoint/restore experiments have
additional system dependencies and, in some cases, require Linux privileges or
specific hardware. Start with [`interpose/README.md`](interpose/README.md) and
use [`STATUS.md`](STATUS.md) as the source of truth for commands, environments,
and measured outputs.

## Citation

If you use OneBarrier, please cite:

> Bojie Li. “OneBarrier: What a Network Must Provide for Transparent Fault
> Tolerance to Be Free.” arXiv:2608.14601, 2026.

Machine-readable citation metadata is available in [`CITATION.cff`](CITATION.cff).

## License

The code is dual-licensed under the [MIT](LICENSE-MIT) or
[Apache-2.0](LICENSE-APACHE) license, at your option.
