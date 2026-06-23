#!/usr/bin/env python3
"""Schematic figures: architecture, three-costs-collapse, determinism boundary."""
import os
import numpy as np
import matplotlib.pyplot as plt
from matplotlib.patches import FancyBboxPatch, FancyArrowPatch
from style import setup, PAL
setup()
OUT = os.path.join(os.path.dirname(__file__), '..')
def P(n): return os.path.join(OUT, n)

def box(ax, x, y, w, h, text, fc, ec=None, fs=8.5, tc='black', lw=1.1, bold=False):
    ec = ec or fc
    ax.add_patch(FancyBboxPatch((x, y), w, h, boxstyle='round,pad=0.012,rounding_size=0.02',
                 linewidth=lw, edgecolor=ec, facecolor=fc, zorder=2))
    ax.text(x+w/2, y+h/2, text, ha='center', va='center', fontsize=fs, color=tc,
            zorder=3, fontweight=('bold' if bold else 'normal'))

def arrow(ax, p0, p1, color=PAL['dark'], lw=1.3, style='-|>'):
    ax.add_patch(FancyArrowPatch(p0, p1, arrowstyle=style, mutation_scale=11,
                 color=color, lw=lw, zorder=4, shrinkA=2, shrinkB=2))

# ---------------------------------------------------------------------------
# F1. System architecture.
# ---------------------------------------------------------------------------
def fig_arch():
    fig, ax = plt.subplots(figsize=(5.0, 3.2))
    ax.set_xlim(0, 10); ax.set_ylim(0, 10); ax.axis('off')
    box(ax, 0.5, 8.45, 9.0, 1.05,
        'unmodified server  (Redis · Memcached · Nginx · Node · PostgreSQL)',
        '#eaf1fb', PAL['blue'], fs=8.0, bold=True)
    ax.text(5.0, 8.18, 'POSIX syscalls: sockets · time · randomness · threads',
            ha='center', fontsize=7.0, color=PAL['gray'])
    # libOS box with sub-components
    box(ax, 0.6, 3.5, 8.8, 4.0, '', '#f5f8fc', PAL['blue'], lw=1.4)
    ax.text(5.0, 7.05, 'OneBarrier determinism libOS', ha='center', fontsize=9.2,
            color=PAL['blue'], fontweight='bold')
    ax.text(5.0, 6.68, 'LD_PRELOAD · no kernel · no app change', ha='center',
            fontsize=7.0, color=PAL['gray'])
    sub = [('virtual clock\n(time)', PAL['teal']),
           ('rng trap\n(getrand/urnd)', PAL['orange']),
           ('share-nothing\nshard', PAL['purple']),
           ('replay + output\nsuppress', PAL['green'])]
    for i, (t, c) in enumerate(sub):
        box(ax, 0.95+i*2.08, 5.0, 1.9, 1.1, t, 'white', c, fs=7.0)
    box(ax, 2.0, 3.78, 6.0, 0.9, 'durable ordered log  +  timestamp-$T$ snapshot',
        '#fbf3e6', PAL['orange'], fs=8.0)
    # fabric (narrower, leaves room for the replica box to its right)
    box(ax, 0.6, 0.7, 6.7, 1.5,
        '1Pipe fabric:\nin-network total order +\nreliable 2PC commit barrier',
        '#eef6f0', PAL['green'], fs=8.0, bold=True)
    ax.text(3.95, 0.45, 'decentralized recovery cut', ha='center', fontsize=7.0, color=PAL['gray'])
    arrow(ax, (5.0, 8.45), (5.0, 7.5), PAL['blue'])
    arrow(ax, (5.0, 3.5), (5.0, 2.2), PAL['green'])
    # in-fabric replica callout, to the RIGHT of the fabric (no overlap)
    box(ax, 7.7, 0.95, 1.8, 1.0, 'RDMA replica\n(durability)', '#fdecea', PAL['red'], fs=7.2)
    arrow(ax, (7.3, 1.45), (7.7, 1.45), PAL['red'], lw=1.1)
    ax.text(8.6, 0.62, 'durability = 2PC phase-1', ha='center', fontsize=6.6, color=PAL['red'])
    fig.savefig(P('fig_arch.pdf')); plt.close(fig); print('wrote fig_arch.pdf')

# ---------------------------------------------------------------------------
# F2. The three coupled costs collapse (each killed by libOS or fabric).
# ---------------------------------------------------------------------------
def fig_threecosts():
    fig, ax = plt.subplots(figsize=(5.2, 2.5))
    ax.set_xlim(0, 10); ax.set_ylim(0, 6); ax.axis('off')
    costs = [('Non-determinism\norder-log', 'fabric supplies the order\n→ order-log vanishes', PAL['green']),
             ('Distributed\nconsistent cut', 'timestamp-$T$ snapshot\n→ empty-channel cut', PAL['green']),
             ('Output-commit\nhold', 'coincides with the 2PC\ncommit barrier → ≈0', PAL['green'])]
    local = ('Local non-determinism\n(time · rng · threads)', 'determinism libOS\n→ byte-identical replay', PAL['blue'])
    ax.text(2.3, 5.6, 'three costs that doomed transparent FT at ms scale', fontsize=8.0,
            color=PAL['gray'], ha='left')
    for i, (c, k, col) in enumerate(costs):
        y = 3.7 - i*1.15
        from matplotlib.patches import FancyBboxPatch, FancyArrowPatch
        ax.add_patch(FancyBboxPatch((0.3, y), 3.0, 0.95, boxstyle='round,pad=0.02,rounding_size=0.04',
                     fc='#fdecea', ec=PAL['red'], lw=1.0))
        ax.text(1.8, y+0.48, c, ha='center', va='center', fontsize=7.6)
        ax.add_patch(FancyArrowPatch((3.4, y+0.48), (5.9, y+0.48), arrowstyle='-|>',
                     mutation_scale=11, color=col, lw=1.4))
        ax.add_patch(FancyBboxPatch((5.95, y), 3.8, 0.95, boxstyle='round,pad=0.02,rounding_size=0.04',
                     fc='#eef6f0', ec=col, lw=1.0))
        ax.text(7.85, y+0.48, k, ha='center', va='center', fontsize=7.4)
    # local non-determinism row
    y = 0.25
    from matplotlib.patches import FancyBboxPatch, FancyArrowPatch
    ax.add_patch(FancyBboxPatch((0.3, y), 3.0, 0.95, boxstyle='round,pad=0.02,rounding_size=0.04',
                 fc='#fdf3e6', ec=PAL['orange'], lw=1.0))
    ax.text(1.8, y+0.48, local[0], ha='center', va='center', fontsize=7.6)
    ax.add_patch(FancyArrowPatch((3.4, y+0.48), (5.9, y+0.48), arrowstyle='-|>',
                 mutation_scale=11, color=PAL['blue'], lw=1.4))
    ax.add_patch(FancyBboxPatch((5.95, y), 3.8, 0.95, boxstyle='round,pad=0.02,rounding_size=0.04',
                 fc='#eaf1fb', ec=PAL['blue'], lw=1.0))
    ax.text(7.85, y+0.48, local[1], ha='center', va='center', fontsize=7.4)
    ax.text(4.65, 4.95, 'fabric', fontsize=7.6, color=PAL['green'], ha='center', rotation=0)
    fig.savefig(P('fig_threecosts.pdf')); plt.close(fig); print('wrote fig_threecosts.pdf')

# ---------------------------------------------------------------------------
# F4. Determinism boundary + virtual clock.
# ---------------------------------------------------------------------------
def fig_boundary():
    fig, axes = plt.subplots(1, 2, figsize=(5.4, 2.3))
    # left: wall-clock time -> timer fires a variable number of times -> diverge
    ax = axes[0]
    ax.set_title('wall-clock time', fontsize=9)
    t = np.linspace(0, 10, 200)
    ax.plot(t, t, color=PAL['gray'], lw=1.2)
    for k in [2, 4, 5.5, 7, 9]:        # live timer fires
        ax.axvline(k, color=PAL['blue'], lw=0.8, alpha=0.7)
    for k in [2.2, 4.5, 6.8, 9.2]:     # replay timer fires (different count)
        ax.axvline(k, color=PAL['red'], lw=0.8, ls='--', alpha=0.7)
    ax.text(0.4, 9.2, 'live timer (5×)', color=PAL['blue'], fontsize=7.0)
    ax.text(0.4, 8.2, 'replay timer (4×)', color=PAL['red'], fontsize=7.0)
    ax.set_xticks([]); ax.set_yticks([]); ax.grid(False)
    ax.set_xlabel('real time', fontsize=8.5)
    ax.set_ylabel('clock value', fontsize=8.5)
    # right: virtual clock -> tick per input event -> identical
    ax = axes[1]
    ax.set_title('virtual clock', fontsize=9)
    ev = np.arange(0, 11)
    ax.step(ev, ev, where='post', color=PAL['green'], lw=1.6)
    ax.scatter(ev, ev, s=14, color=PAL['green'], zorder=3)
    ax.set_xticks([]); ax.set_yticks([]); ax.grid(False)
    ax.set_xlabel('input events', fontsize=8.5)
    ax.set_ylabel('virtual time', fontsize=8.5)
    # captions placed BELOW everything via fig.text (no axis overlap)
    fig.text(0.28, -0.04, 'timer-driven reads desync\n→ replay diverges', ha='center',
             fontsize=7.3, color=PAL['red'])
    fig.text(0.78, -0.04, 'time = base + ticks/input\n→ count-independent, identical', ha='center',
             fontsize=7.3, color=PAL['green'])
    fig.subplots_adjust(bottom=0.22, wspace=0.28)
    fig.savefig(P('fig_boundary.pdf')); plt.close(fig); print('wrote fig_boundary.pdf')

if __name__ == '__main__':
    fig_arch()
    fig_threecosts()
    fig_boundary()
    print('schematic figures done')
