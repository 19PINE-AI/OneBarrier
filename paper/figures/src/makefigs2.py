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
    box(ax, 0.6, 8.48, 8.8, 1.12,
        'unmodified server\n(Redis · Memcached · Nginx · Node · PostgreSQL)',
        '#eaf1fb', PAL['blue'], fs=7.4, bold=True)
    ax.text(5.0, 8.14, 'POSIX syscalls: sockets · time · randomness · threads',
            ha='center', fontsize=7.0, color=PAL['gray'])
    # determinism-shim box with sub-components
    box(ax, 0.6, 3.5, 8.8, 4.0, '', '#f5f8fc', PAL['blue'], lw=1.4)
    ax.text(5.0, 7.05, 'OneBarrier determinism shim  (Determinism)', ha='center', fontsize=9.0,
            color=PAL['blue'], fontweight='bold')
    ax.text(5.0, 6.68, 'LD_PRELOAD · no kernel · no app change', ha='center',
            fontsize=7.0, color=PAL['gray'])
    sub = [('virtual clock\n(time)', PAL['teal']),
           ('rng trap\n(getrand/urnd)', PAL['orange']),
           ('share-nothing\nshard', PAL['purple']),
           ('replay + output\nsuppress', PAL['green'])]
    for i, (t, c) in enumerate(sub):
        box(ax, 0.78+i*2.13, 5.0, 2.02, 1.1, t, 'white', c, fs=6.7)
    box(ax, 1.4, 3.78, 7.2, 0.9, 'durable ordered log  +  timestamp-$T$ snapshot',
        '#fbf3e6', PAL['orange'], fs=8.0)
    # fabric (narrower, leaves room for the replica box to its right)
    box(ax, 0.6, 0.55, 6.7, 1.7,
        '1Pipe fabric:\nglobal delivery order (Order) +\ncommit barrier (Barrier)',
        '#eef6f0', PAL['green'], fs=8.0, bold=True)
    ax.text(3.95, 0.38, 'uncoordinated recovery cut', ha='center', va='top',
            fontsize=7.0, color=PAL['gray'])
    arrow(ax, (5.0, 7.84), (5.0, 7.52), PAL['blue'])
    arrow(ax, (5.0, 3.5), (5.0, 2.32), PAL['green'])
    # in-fabric replica callout, to the RIGHT of the fabric (no overlap)
    box(ax, 7.5, 0.85, 2.35, 1.4, 'in-fabric\nreplicas\n(Durability)', '#fdecea', PAL['red'], fs=7.0)
    arrow(ax, (7.3, 1.55), (7.5, 1.55), PAL['red'], lw=1.1)
    ax.text(8.67, 0.62, '1-RTT copy, in barrier', ha='center', va='top',
            fontsize=6.4, color=PAL['red'])
    fig.savefig(P('fig_arch.pdf')); plt.close(fig); print('wrote fig_arch.pdf')

# ---------------------------------------------------------------------------
# F2. The three classical costs, and the condition that eliminates each.
# ---------------------------------------------------------------------------
def fig_conditions():
    fig, ax = plt.subplots(figsize=(5.7, 2.9))
    ax.set_xlim(0, 10); ax.set_ylim(0, 6.3); ax.axis('off')
    rows = [('order log\non every message',
             'Order: one global delivery order\n→ replay without a log',
             PAL['green'], '#eef6f0'),
            ('coordinated snapshot\n(markers + channel state)',
             'Order + Barrier: uncoordinated\ncut at timestamp $T$, channels empty',
             PAL['green'], '#eef6f0'),
            ('output held for extra\ndurability round trips',
             'Barrier + Durability: durable inside\nthe barrier → no added round trip',
             PAL['green'], '#eef6f0'),
            ('local non-determinism\n(time · randomness · threads)',
             'Determinism: user-space shim\n→ byte-identical replay',
             PAL['blue'], '#eaf1fb')]
    ax.text(1.85, 5.85, 'classical critical-path cost', fontsize=8.0,
            color=PAL['gray'], ha='center')
    ax.text(7.87, 5.85, 'condition that eliminates it', fontsize=8.0,
            color=PAL['gray'], ha='center')
    for i, (c, k, col, fc) in enumerate(rows):
        y = 4.2 - i*1.34
        cost_fc = '#fdecea' if i < 3 else '#fdf3e6'
        cost_ec = PAL['red'] if i < 3 else PAL['orange']
        ax.add_patch(FancyBboxPatch((0.15, y), 3.35, 1.12, boxstyle='round,pad=0.02,rounding_size=0.04',
                     fc=cost_fc, ec=cost_ec, lw=1.0))
        ax.text(1.83, y+0.56, c, ha='center', va='center', fontsize=7.3)
        ax.add_patch(FancyArrowPatch((3.6, y+0.56), (5.85, y+0.56), arrowstyle='-|>',
                     mutation_scale=11, color=col, lw=1.4))
        ax.add_patch(FancyBboxPatch((5.9, y), 3.95, 1.12, boxstyle='round,pad=0.02,rounding_size=0.04',
                     fc=fc, ec=col, lw=1.0))
        ax.text(7.87, y+0.56, k, ha='center', va='center', fontsize=6.9)
    ax.text(4.7, 4.98, 'network', fontsize=7.4, color=PAL['green'], ha='center')
    ax.text(4.7, 0.44, 'host', fontsize=7.4, color=PAL['blue'], ha='center')
    fig.savefig(P('fig_conditions.pdf')); plt.close(fig); print('wrote fig_conditions.pdf')

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
    ax.text(0.4, 9.2, 'live timer (5×)', color=PAL['blue'], fontsize=7.0, zorder=5,
            bbox=dict(boxstyle='square,pad=0.15', fc='white', ec='none'))
    ax.text(0.4, 8.2, 'replay timer (4×)', color=PAL['red'], fontsize=7.0, zorder=5,
            bbox=dict(boxstyle='square,pad=0.15', fc='white', ec='none'))
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
    fig_conditions()
    fig_boundary()
    print('schematic figures done')
