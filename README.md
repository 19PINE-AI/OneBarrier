# OneBarrier

**Transparent, low-overhead fault tolerance for unmodified share-nothing
servers — as a byproduct of total-order communication.**

OneBarrier routes the IPC of unmodified servers through an in-network
total-order *reliable* fabric ([1Pipe](https://doi.org/10.1145/3452296.3472909),
SIGCOMM '21). The fabric supplies, for free, the three things fault tolerance
historically paid for: the message order (so deterministic replay needs **no
order-log**), an empty-channel **uncoordinated timestamp-T snapshot** (replacing
Chandy–Lamport), and a reliable-delivery **2PC commit barrier** that **coincides
with the output-commit barrier** — so FT's marginal cost over the fabric baseline
is ≈ 0 at 1Pipe's µs operating point (RDMA RTT 1–2 µs, 1-RTT replication).

> **Thesis & honest scope.** This is a *co-design + operating-point +
> measurement* result, not a new primitive: transparent passive FT, long
> dismissed as too expensive at the millisecond scale (Remus, LLFT, HyCoR),
> becomes essentially free at the in-network-total-order + RDMA operating point.
> Durability is in-memory **f-of-k fail-stop** (FaRM/RAMCloud tradeoff), not
> power-loss-safe. See [`docs/PLAN.md`](docs/PLAN.md) for the full related-work
> analysis, narrative, and experiment plan, and [`STATUS.md`](STATUS.md) for
> what is built and measured so far.

## Layout

```
crates/onebarrier/   The OneBarrier engine: deterministic-replay state machine,
                     durable ordered log, timestamp-T snapshot, exactly-once
                     output suppression, crash recovery.
docs/PLAN.md         Research plan: related work, framing, experiments (RQ1–RQ8).
STATUS.md            Milestone tracker + reproducible results.
```

## Build & test

OneBarrier builds on the 1Pipe reproduction, expected as a sibling checkout at
`../1Pipe` (i.e. `~/1Pipe`).

```bash
cargo test -p onebarrier          # core correctness (incl. exactly-once recovery)
cargo run  -p onebarrier --bin ob-demo   # end-to-end snapshot+replay demo
```

## Status

Early. M0 (the replication core) is implemented and tested; the networked node
over the live 1Pipe fabric, the application suite (Redis/Memcached/Nginx/Node/
SQLite-class), and the RQ1–RQ8 evaluation are in progress. Numbers reported in
`STATUS.md` come from actually running the code — none are asserted.

## License

Dual-licensed under MIT or Apache-2.0.
