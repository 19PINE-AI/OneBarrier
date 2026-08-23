# Changelog

All notable changes to this project are documented here. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); this project uses
[semantic versioning](https://semver.org/).

## [0.2.0] — first public release

The research artifact becomes a usable project. No experimental result changed;
the paper's numbers and the full experimental record are unchanged in
[`docs/research/RESULTS.md`](docs/research/RESULTS.md).

### Added

- **`bin/onebarrier`** — a command-line tool covering the whole workflow:
  `doctor`, `build`, `run`, `recover`, `replay`, `sessions`, `verify`, `demo`.
  Previously the determinism stack had to be assembled by hand from six
  environment variables, an order-sensitive two-library `LD_PRELOAD`, and
  `setarch -R`.
- **`onebarrier doctor`** — reports every dependency and what each one unlocks,
  so an incomplete environment produces a diagnosis instead of a confusing
  failure.
- **Makefiles** — `make` builds shims and engine; `make demo`, `make verify`,
  `make test`, `make doctor`, `make clean`. `interpose/Makefile` builds the three
  shims with a clean, warning-free compile.
- **Guides** — [getting started](docs/getting-started.md),
  [how it works](docs/how-it-works.md), and
  [use it on your app](docs/your-app.md), which includes the fit test and the
  residual-nondeterminism hunting list.
- **Project files** — `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, `SECURITY.md`,
  issue and pull request templates.
- **CI** — the shims are now built and the end-to-end demo is run on every push,
  so the path a new user takes is covered by tests.

### Fixed

- **The flagship demo silently failed on a fresh clone.** The shims are build
  artifacts and correctly gitignored, but no script built them — and the dynamic
  loader ignores a missing `LD_PRELOAD` library without an error. A new clone
  therefore ran every harness with no interposition at all and reported
  `RESULT: redis NOT byte-identical` — a real-looking failure caused entirely by
  a missing build step. All 18 harnesses now call `ob_require_shims`, which
  builds what is missing or fails loudly if it cannot.

### Changed

- Research documents moved to `docs/research/`: `STATUS.md` → `RESULTS.md`, with
  `PLAN.md` and `PAPER-PLAN.md` alongside it. Contents unchanged; all internal
  references updated.
- README rewritten for readers who have not read the paper.

## [0.1.0] — research artifact

The state of the repository when
[arXiv:2608.14601](https://arxiv.org/abs/2608.14601) was published: engine,
determinism layer, 15 application harnesses, TLA+ specifications, paper source,
and the interactive companion site.
