# OneBarrier — interactive paper companion

A single-page React site illustrating *OneBarrier: Transparent Fault Tolerance for Free*:
how the 1Pipe total-order fabric works, how OneBarrier turns its reliable-delivery commit
barrier into the output-commit barrier (fault tolerance at 0.23% marginal cost), the
determinism libOS, the crash/replay/exactly-once life cycle, and the paper's measured
results and honest novelty reckoning.

## Structure

- `ts=001` Why transparent FT never shipped — the three coupled costs
- `ts=002` **anim 01** — the 1Pipe fabric end to end (stamp → aggregate → hold → deliver → commit)
- `ts=003` The OneBarrier stack, the three collapses, **anim 02** — the barrier coincidence (ride vs. stack)
- `ts=004` The determinism libOS (virtual clock boundary, syscall RNG trap, share-nothing sharding)
- `ts=005` **anim 03** — live · checkpoint · crash · restore · replay+suppress · resume
- `ts=006` Results (all numbers taken from the paper's figure sources under `../paper/figures/src/`)
- `ts=007` Honest reckoning — positioning, prior art, limitations

All animations are driven by a shared loop clock (`src/lib/anim.js`) — each frame is a
pure function of `t`, so the animations are deterministic, in keeping with the paper.

## Develop / build

```sh
npm install
npm run dev        # dev server
npm run build      # production build into dist/
npm run preview    # serve the build
```

Requires Node >= 20.19 (Vite 6). No backend; deploy `dist/` on any static host.
