# Changelog

## 0.2.0 — first public release

The research artifact becomes a usable project. No experimental result changed; the
numbers and the full record are unchanged in
[docs/research/RESULTS.md](docs/research/RESULTS.md).

### Fixed

The demo silently failed on a fresh clone. The shims are build artifacts and correctly
gitignored, but nothing built them, and the dynamic loader ignores a missing
`LD_PRELOAD` library without an error. So a new clone ran every harness with no
interposition at all and printed `RESULT: redis NOT byte-identical`, which looks like a
failed research claim and was a missing build step. All 18 harnesses now call
`ob_require_shims`, which builds what's missing or fails loudly.

### Added

- `bin/onebarrier`, covering the whole workflow: `doctor`, `build`, `run`, `recover`,
  `replay`, `sessions`, `verify`, `demo`. The stack used to be six environment
  variables, an order-sensitive two-library preload, and `setarch -R`, assembled by hand.
- `onebarrier doctor`, which reports every dependency and what it unlocks, so an
  incomplete environment gives you a diagnosis instead of a confusing result.
- Makefiles. `make` builds shims and engine; also `demo`, `verify`, `test`, `doctor`,
  `clean`. The shims compile warning-free.
- Guides: [getting started](docs/getting-started.md),
  [how it works](docs/how-it-works.md), and [your app](docs/your-app.md) with the fit
  test and the leftover-nondeterminism list.
- `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, `SECURITY.md`, issue and PR templates.
- CI now builds the shims and runs the demo end to end on every push, so the path a new
  user takes is covered.

### Changed

- Research docs moved to `docs/research/`: `STATUS.md` became `RESULTS.md`, with
  `PLAN.md` and `PAPER-PLAN.md` beside it. Contents unchanged, references updated.
- README rewritten for people who haven't read the paper.

## 0.1.0 — research artifact

The repository as it stood when [arXiv:2608.14601](https://arxiv.org/abs/2608.14601) was
published: engine, determinism layer, 15 application harnesses, TLA+ specs, paper
source, companion site.
