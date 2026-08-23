# Figures

Generated, not hand-drawn. Regenerate after editing the source:

```bash
python3 docs/figures/make_figures.py
```

Each figure is written twice, `-light.svg` and `-dark.svg`, and embedded with a
`<picture>` element so GitHub serves whichever matches the reader's theme:

```html
<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/figures/architecture-dark.svg">
  <img alt="..." src="docs/figures/architecture-light.svg" width="100%">
</picture>
```

| figure | used in | shows |
|---|---|---|
| `architecture` | README | clients, the fabric's three conditions, one executing primary and two logging backups |
| `costs` | README, how-it-works | the three classical costs and the conditions that remove each |
| `barrier` | README | ride-versus-stack request timelines, drawn to one scale |
| `passive` | README | execution CPU against replica count, active SMR versus passive |
| `vclock` | how-it-works | why a record/replay cursor slips and a virtual clock doesn't |
| `recovery` | how-it-works | snapshot, log suffix, state transfer from a survivor, resume |
| `sharding` | how-it-works | memcached at one thread, four threads, and four shards |

Text is rendered as paths, so the figures look the same without DejaVu Sans installed.
Numbers come from [../research/RESULTS.md](../research/RESULTS.md); if a measurement
changes, update the generator rather than editing SVG by hand.
