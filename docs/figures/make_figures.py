#!/usr/bin/env python3
"""Figures for the README and docs/how-it-works.md.

Rendered as SVG in light and dark variants, embedded with <picture> so they
follow the reader's GitHub theme. Content follows the paper's figures; the
styling is different because a README is read on a screen at arm's length, not
in a two-column PDF.

    python3 docs/figures/make_figures.py
"""
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.patches import FancyBboxPatch, FancyArrowPatch
import pathlib

OUT = pathlib.Path(__file__).parent

THEMES = {
    "light": dict(
        bg="#ffffff", fg="#1f2328", muted="#656d76", faint="#d0d7de",
        blue="#0969da", green="#1a7f37", orange="#bc4c00", red="#cf222e",
        purple="#8250df", teal="#0f7c86",
        blue_fill="#ddf4ff", green_fill="#dafbe1", orange_fill="#fff1e5",
        red_fill="#ffebe9", purple_fill="#fbefff", neutral_fill="#f6f8fa",
    ),
    "dark": dict(
        bg="#0d1117", fg="#e6edf3", muted="#9198a1", faint="#30363d",
        blue="#4493f8", green="#3fb950", orange="#db6d28", red="#f85149",
        purple="#ab7df8", teal="#39c5cf",
        blue_fill="#0c2d6b", green_fill="#0f5323", orange_fill="#5a1e02",
        red_fill="#67060c", purple_fill="#3c1e70", neutral_fill="#161b22",
    ),
}


def setup(t):
    plt.rcParams.update({
        "svg.fonttype": "path",       # render identically without the font installed
        # Reproducible output: without a fixed salt matplotlib randomises the
        # generated element ids on every run, so CI would see a diff each time.
        "svg.hashsalt": "onebarrier",
        "figure.dpi": 110,
        "savefig.bbox": "tight",
        "savefig.pad_inches": 0.06,
        "font.family": "sans-serif",
        "font.sans-serif": ["DejaVu Sans"],
        "font.size": 9,
        "text.color": t["fg"],
        "axes.labelcolor": t["fg"],
        "xtick.color": t["muted"],
        "ytick.color": t["muted"],
        "axes.edgecolor": t["faint"],
        "figure.facecolor": t["bg"],
        "axes.facecolor": t["bg"],
        "savefig.facecolor": t["bg"],
    })


def box(ax, x, y, w, h, label, face, edge, fs=8.5, bold=False, lw=1.2, tcolor=None):
    ax.add_patch(FancyBboxPatch(
        (x, y), w, h, boxstyle="round,pad=0.02,rounding_size=0.14",
        facecolor=face, edgecolor=edge, linewidth=lw, zorder=2))
    if label:
        ax.text(x + w / 2, y + h / 2, label, ha="center", va="center",
                fontsize=fs, color=tcolor or edge, zorder=3,
                fontweight="bold" if bold else "normal", linespacing=1.45)


def titled_box(ax, x, y, w, h, title, body, face, edge, tcolor,
               tfs=8.2, bfs=7.0, lw=1.2):
    """A box with a bold heading and a lighter body, without mathtext."""
    ax.add_patch(FancyBboxPatch(
        (x, y), w, h, boxstyle="round,pad=0.02,rounding_size=0.14",
        facecolor=face, edgecolor=edge, linewidth=lw, zorder=2))
    ax.text(x + w / 2, y + h * 0.70, title, ha="center", va="center",
            fontsize=tfs, color=edge, fontweight="bold", zorder=3)
    ax.text(x + w / 2, y + h * 0.30, body, ha="center", va="center",
            fontsize=bfs, color=tcolor, zorder=3, linespacing=1.5)


def arrow(ax, a, b, color, lw=1.3, style="-|>"):
    ax.add_patch(FancyArrowPatch(a, b, arrowstyle=style, mutation_scale=11,
                                 color=color, linewidth=lw, zorder=4,
                                 shrinkA=0, shrinkB=0))


def canvas(w, h, t):
    fig, ax = plt.subplots(figsize=(w, h))
    ax.set_xlim(0, 100); ax.set_ylim(0, 100); ax.axis("off")
    fig.patch.set_facecolor(t["bg"])
    return fig, ax


def save(fig, name, theme):
    p = OUT / f"{name}-{theme}.svg"
    # metadata Date=None drops the generation timestamp, the other source of churn
    fig.savefig(p, format="svg", metadata={"Date": None})
    plt.close(fig)
    print("wrote", p.name)


# ---------------------------------------------------------------- 1. system --
def fig_architecture(t):
    fig, ax = canvas(9.2, 4.6, t)
    ax.text(50, 97, "One replica executes. The rest only log.",
            ha="center", fontsize=10.5, color=t["fg"], fontweight="bold")

    # clients
    box(ax, 38, 85, 24, 9, "clients", t["neutral_fill"], t["muted"], fs=9,
        bold=True, tcolor=t["fg"])
    arrow(ax, (50, 85), (50, 80), t["muted"])

    # the fabric spans every replica it delivers to
    box(ax, 2, 54, 96, 26, "", t["green_fill"], t["green"], lw=1.7)
    ax.text(50, 75.5, "total-order fabric  (1Pipe)", ha="center", fontsize=10,
            color=t["green"], fontweight="bold")
    conds = [("Order", "one global\ndelivery order"),
             ("Barrier", "commit barrier\nconfirms delivery"),
             ("Durability", "input copied to backups\nbefore its barrier completes")]
    for i, (nm, sub) in enumerate(conds):
        titled_box(ax, 5 + i * 31, 56.5, 28, 15, nm, sub, t["bg"], t["green"],
                   t["fg"], tfs=8.4, bfs=6.8)

    # delivery, in timestamp order, to each replica
    for x in (19.5, 55, 84):
        arrow(ax, (x, 54), (x, 48), t["muted"], lw=1.2)
    ax.text(23, 51, "deliver in timestamp order", fontsize=6.8, color=t["muted"],
            va="center", ha="left")

    # primary
    box(ax, 3, 12, 33, 36, "", t["blue_fill"], t["blue"], lw=1.7)
    ax.text(19.5, 44.5, "primary replica", ha="center", fontsize=9.5,
            color=t["blue"], fontweight="bold")
    box(ax, 5, 34.5, 29, 7.5, "unmodified server binary", t["bg"], t["blue"],
        fs=7.8, tcolor=t["fg"])
    box(ax, 5, 25.5, 29, 7.5, "determinism shim", t["bg"], t["purple"], fs=7.8,
        tcolor=t["fg"])
    box(ax, 5, 15.5, 29, 7.5, "durable ordered log + snapshot", t["orange_fill"],
        t["orange"], fs=7.2, tcolor=t["fg"])
    ax.text(19.5, 7.5, "executes the state machine", ha="center", fontsize=7.6,
            color=t["blue"], fontweight="bold")

    # backups
    for i, x0 in enumerate((42, 71)):
        box(ax, x0, 12, 26, 36, "", t["neutral_fill"], t["faint"], lw=1.4)
        ax.text(x0 + 13, 44.5, f"backup {i+1}", ha="center", fontsize=9.5,
                color=t["muted"], fontweight="bold")
        ax.text(x0 + 13, 31, "no execution,\nno state machine", ha="center",
                fontsize=7.4, color=t["muted"], linespacing=1.5)
        box(ax, x0 + 2, 15.5, 22, 7.5, "durable ordered log", t["orange_fill"],
            t["orange"], fs=7.2, tcolor=t["fg"])
        ax.text(x0 + 13, 7.5, "logs only", ha="center", fontsize=7.6, color=t["muted"])

    # in-barrier replication
    arrow(ax, (36, 19.2), (42, 19.2), t["red"], lw=1.5)
    arrow(ax, (68, 19.2), (71, 19.2), t["red"], lw=1.5)
    ax.text(63, 2.5, "every input scattered to the backups in one round trip, inside the\n"
                     "barrier the fabric already crosses.  Tolerates f < k crashes.",
            ha="center", fontsize=7.6, color=t["red"], linespacing=1.6)
    return fig


# ------------------------------------------------- 2. ride vs stack timeline --
def fig_barrier(t):
    fig, ax = canvas(9.2, 3.6, t)
    ax.text(50, 96, "Where you put the replica write is what costs you",
            ha="center", fontsize=10.5, color=t["fg"], fontweight="bold")

    SCALE = 62 / 6016.0     # µs -> x units, both rows to the same scale
    X0 = 8

    # --- riding the barrier
    y = 62
    ax.text(X0 - 1.5, y + 11, "riding the commit barrier", fontsize=9,
            color=t["green"], fontweight="bold", ha="left")
    w = 2014 * SCALE
    box(ax, X0, y, w, 8.5, "delivery + barrier", t["green_fill"],
        t["green"], fs=6.8, tcolor=t["fg"])
    # the 4.59 µs replica write, nested inside: too small to see, so call it out
    ax.plot([X0 + w * 0.62, X0 + w * 0.62], [y, y + 8.5], color=t["red"], lw=2.2,
            zorder=5, solid_capstyle="butt")
    ax.annotate("replica write, 4.59 µs\n(inside the barrier)",
                xy=(X0 + w * 0.62, y), xytext=(X0 + w * 0.62, y - 15),
                fontsize=7.4, color=t["red"], ha="center", linespacing=1.5,
                arrowprops=dict(arrowstyle="-|>", color=t["red"], lw=1.1))
    ax.plot([X0 + w, X0 + w], [y - 2, y + 12], color=t["fg"], lw=1.1, ls=":")
    ax.text(X0 + w + 1.5, y + 4.2, "reply released\n2018 µs", fontsize=8,
            color=t["fg"], va="center", linespacing=1.5)

    # --- stacking after it
    y = 26
    ax.text(X0 - 1.5, y + 11, "serial fsync after it", fontsize=9,
            color=t["red"], fontweight="bold", ha="left")
    wd = 3042 * SCALE
    wf = 2963 * SCALE
    box(ax, X0, y, wd, 8.5, "delivery + barrier", t["green_fill"],
        t["green"], fs=6.8, tcolor=t["fg"])
    box(ax, X0 + wd, y, wf, 8.5, "fsync  2963 µs", t["red_fill"], t["red"], fs=7.6,
        tcolor=t["fg"])
    ax.plot([X0 + wd + wf, X0 + wd + wf], [y - 2, y + 12], color=t["fg"], lw=1.1, ls=":")
    ax.text(X0 + wd + wf + 1.5, y + 4.2, "reply released\n6016 µs", fontsize=8,
            color=t["fg"], va="center", linespacing=1.5)

    ax.annotate("", xy=(X0, 12), xytext=(X0 + 62, 12),
                arrowprops=dict(arrowstyle="<->", color=t["faint"], lw=1))
    ax.text(50, 7.5, "same durability guarantee, 645x apart on placement alone",
            ha="center", fontsize=8.4, color=t["muted"])
    return fig


# ------------------------------------------------- 3. passive vs active CPU ---
def fig_passive(t):
    setup(t)
    fig, ax = plt.subplots(figsize=(5.6, 3.1))
    reps = [2, 3, 5, 7]
    active = [205.9, 309.5, 519.5, 729.7]
    passive = [105.7, 108.7, 114.5, 123.1]
    xs = range(len(reps))
    ax.bar([x - 0.19 for x in xs], active, 0.36, label="active SMR (every replica executes)",
           color=t["red"], edgecolor="none")
    ax.bar([x + 0.19 for x in xs], passive, 0.36, label="OneBarrier (one executes)",
           color=t["green"], edgecolor="none")
    for x, a, p in zip(xs, active, passive):
        ax.text(x + 0.19, p + 18, f"-{round((1-p/a)*100)}%", ha="center", fontsize=7.8,
                color=t["green"], fontweight="bold")
    ax.set_xticks(list(xs)); ax.set_xticklabels([f"{r} replicas" for r in reps])
    ax.set_ylabel("execution CPU (ms)")
    ax.set_title("Passive replication keeps execution CPU flat", fontsize=9.6,
                 color=t["fg"], pad=9)
    ax.legend(frameon=False, fontsize=7.6, loc="upper left")
    ax.spines[["top", "right"]].set_visible(False)
    ax.grid(axis="y", color=t["faint"], lw=0.7)
    ax.set_axisbelow(True)
    return fig


# -------------------------------------------------- 4. three costs, removed ---
def fig_costs(t):
    fig, ax = canvas(9.2, 3.4, t)
    ax.text(50, 95, "Three costs on every request, and what removes each",
            ha="center", fontsize=10.5, color=t["fg"], fontweight="bold")
    rows = [
        ("the order log", "record every message's\narrival order before acting",
         "Order", "the network is the order,\nso there is nothing to record"),
        ("the coordinated snapshot", "marker messages, in-flight\nchannel capture",
         "Order + Barrier", "every node cuts at the same\ntimestamp, independently"),
        ("the output hold", "every reply waits for a\ndurable write",
         "Barrier + Durability", "the wait ends at a barrier the\nreply already waits for"),
    ]
    for i, (name, cost, cond, why) in enumerate(rows):
        y = 62 - i * 26
        box(ax, 1.5, y, 25, 20, "", t["red_fill"], t["red"], lw=1.1)
        ax.text(14, y + 15, name, ha="center", fontsize=8.4, color=t["red"],
                fontweight="bold")
        ax.text(14, y + 6.5, cost, ha="center", fontsize=7.2, color=t["fg"],
                linespacing=1.5)
        arrow(ax, (27.5, y + 10), (35.5, y + 10), t["muted"], lw=1.2)
        box(ax, 36, y + 4, 24, 12, cond, t["neutral_fill"], t["blue"], fs=8,
            bold=True, tcolor=t["blue"])
        arrow(ax, (61, y + 10), (69, y + 10), t["muted"], lw=1.2)
        box(ax, 69.5, y, 29, 20, "", t["green_fill"], t["green"], lw=1.1)
        ax.text(84, y + 10, why, ha="center", fontsize=7.4, color=t["fg"],
                linespacing=1.6)
    ax.text(50, 2, "the fourth condition, Determinism, is the host's job",
            ha="center", fontsize=8.2, color=t["muted"])
    return fig


# --------------------------------------------------------- 5. virtual clock ---
def fig_vclock(t):
    fig, ax = canvas(9.2, 3.9, t)
    ax.text(50, 96, "Why record/replay breaks, and the virtual clock doesn't",
            ha="center", fontsize=10.5, color=t["fg"], fontweight="bold")

    # ---- left: record/replay cursor slip
    ax.text(2, 84, "record/replay: hand back logged values in order", fontsize=8.6,
            color=t["red"], fontweight="bold", ha="left")
    logged = ["t0", "t1", "t2", "t3"]
    for i, v in enumerate(logged):
        box(ax, 3 + i * 9.5, 68, 8, 8, v, t["neutral_fill"], t["muted"], fs=8,
            tcolor=t["fg"])
    ax.text(2, 63, "live: one clock read per request", fontsize=7.2, color=t["muted"],
            ha="left")

    ax.text(2, 53, "replay: an internal timer fires an extra read", fontsize=7.2,
            color=t["muted"], ha="left")
    reads = [("req", "t0", True), ("timer", "t1", False), ("req", "t2", True),
             ("req", "t3", True)]
    for i, (kind, v, ok) in enumerate(reads):
        c = t["red"] if not ok else t["muted"]
        box(ax, 3 + i * 9.5, 38, 8, 8, v, t["red_fill"] if not ok else t["neutral_fill"],
            c, fs=8, tcolor=t["fg"])
        ax.text(7 + i * 9.5, 34.5, kind, ha="center", fontsize=6.4, color=c)
    ax.annotate("cursor slips by one;\nevery value after is wrong",
                xy=(16, 38), xytext=(20, 20), fontsize=7.4, color=t["red"],
                ha="center", linespacing=1.5,
                arrowprops=dict(arrowstyle="-|>", color=t["red"], lw=1.1))

    ax.plot([48, 48], [8, 88], color=t["faint"], lw=1.1)

    # ---- right: virtual clock
    ax.text(52, 84, "virtual clock: time is a function of the inputs", fontsize=8.6,
            color=t["green"], fontweight="bold", ha="left")
    box(ax, 53, 62, 44, 14, "time  =  base  +  ticks", t["green_fill"], t["green"],
        fs=10.5, tcolor=t["fg"])
    ax.text(75, 57, "ticks advance on each input event, never on a clock read",
            ha="center", fontsize=7.4, color=t["muted"])

    for i, (lbl, y) in enumerate([("live", 40), ("replay", 26)]):
        ax.text(52, y + 3.5, lbl, fontsize=7.6, color=t["muted"], ha="left")
        for j, v in enumerate(["t0", "t1", "t2", "t3"]):
            box(ax, 60 + j * 9.2, y, 8, 8, v, t["green_fill"], t["green"], fs=8,
                tcolor=t["fg"])
    ax.text(75, 17, "identical, no matter who reads the clock or how often",
            ha="center", fontsize=7.6, color=t["green"])
    ax.text(75, 9, "replay ignores the real-time gap entirely", ha="center",
            fontsize=7.4, color=t["muted"])
    return fig


# -------------------------------------------------------------- 6. recovery ---
def fig_recovery(t):
    fig, ax = canvas(9.2, 3.0, t)
    ax.text(50, 93, "Recovering a replica: no order log anywhere in the path",
            ha="center", fontsize=10.5, color=t["fg"], fontweight="bold")
    steps = [
        ("replica\ncrashes", t["red"], t["red_fill"]),
        ("load latest\nsnapshot", t["blue"], t["blue_fill"]),
        ("replay log suffix\nin timestamp order", t["orange"], t["orange_fill"]),
        ("state transfer:\nfetch missing prefix\nfrom a survivor", t["purple"], t["purple_fill"]),
        ("resume live\ndelivery", t["green"], t["green_fill"]),
    ]
    w, gap = 15.5, 4.0
    for i, (label, c, fill) in enumerate(steps):
        x = 3 + i * (w + gap)
        box(ax, x, 40, w, 26, label, fill, c, fs=7.6, tcolor=t["fg"])
        if i < len(steps) - 1:
            arrow(ax, (x + w, 53), (x + w + gap, 53), t["muted"], lw=1.2)
    ax.text(50, 27, "external effects are disabled during replay, and per-client "
                    "high-water marks\nlive inside the snapshot, so any output the "
                    "suffix re-derives is dropped rather than re-sent",
            ha="center", fontsize=7.8, color=t["muted"], linespacing=1.6)
    ax.text(50, 8, "that is what makes recovery exactly-once, not merely correct",
            ha="center", fontsize=8.2, color=t["fg"])
    return fig


# -------------------------------------------------------------- 7. sharding ---
def fig_sharding(t):
    setup(t)
    fig, ax = plt.subplots(figsize=(5.8, 3.0))
    labels = ["memcached\n-t 1", "memcached\n-t 4", "4 x -t 1\nshards"]
    vals = [342, 821, 1000]
    det = [True, False, True]
    colors = [t["green"] if d else t["red"] for d in det]
    ax.bar(range(3), vals, 0.5, color=colors, edgecolor="none")
    for i, (v, d) in enumerate(zip(vals, det)):
        ax.text(i, v + 28, f"{v}k ops/s", ha="center", fontsize=8.2, color=t["fg"],
                fontweight="bold")
        ax.text(i, v * 0.5, "deterministic" if d else "not\ndeterministic",
                ha="center", va="center", fontsize=7.4,
                color="#ffffff" if d else "#ffffff", linespacing=1.5)
    ax.set_xticks(range(3)); ax.set_xticklabels(labels, fontsize=8)
    ax.set_ylabel("throughput (k ops/s)")
    ax.set_ylim(0, 1180)
    ax.set_title("Sharding beats threading, and stays deterministic", fontsize=9.6,
                 color=t["fg"], pad=9)
    ax.spines[["top", "right"]].set_visible(False)
    ax.grid(axis="y", color=t["faint"], lw=0.7)
    ax.set_axisbelow(True)
    return fig


FIGURES = {
    "architecture": fig_architecture,
    "barrier": fig_barrier,
    "passive": fig_passive,
    "costs": fig_costs,
    "vclock": fig_vclock,
    "recovery": fig_recovery,
    "sharding": fig_sharding,
}

if __name__ == "__main__":
    for theme, t in THEMES.items():
        for name, fn in FIGURES.items():
            setup(t)
            save(fn(t), name, theme)
    print(f"\n{len(FIGURES)} figures x {len(THEMES)} themes")
