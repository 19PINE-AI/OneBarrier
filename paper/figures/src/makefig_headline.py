#!/usr/bin/env python3
"""Headline figure (page 1, below the abstract).

One request under transparent fault tolerance, drawn twice on a shared
(schematic) time axis. Top: every prior transparent system stacks the
durable write after the delivery-confirmation wait, holding the reply.
Bottom: OneBarrier scatters the copy to backups inside the commit barrier
the network crosses anyway, so the reply leaves at the barrier. The two
annotated deltas are the measured p50 marginal durability costs of Table 2.
"""
import os
import matplotlib.pyplot as plt
from matplotlib.patches import FancyBboxPatch
from style import setup, finish, PAL
setup()
OUT = os.path.join(os.path.dirname(__file__), '..')


def bar(ax, x0, x1, y, h, text, edge, face, fs=7.6, lw=1.2):
    ax.add_patch(FancyBboxPatch((x0, y), x1 - x0, h,
                 boxstyle='round,pad=0.02,rounding_size=0.06',
                 linewidth=lw, edgecolor=edge, facecolor=face, zorder=2))
    ax.text((x0 + x1) / 2, y + h / 2, text, ha='center', va='center',
            fontsize=fs, color=edge, zorder=3)


def io_arrows(ax, yc, reply_x0, color):
    ax.annotate('', xy=(1.55, yc), xytext=(0.8, yc),
                arrowprops=dict(arrowstyle='-|>', color=PAL['gray'], lw=1.1))
    ax.text(1.14, yc + 0.42, 'input', fontsize=7.0, color=PAL['gray'],
            ha='center', va='center')
    ax.annotate('', xy=(9.7, yc), xytext=(reply_x0, yc),
                arrowprops=dict(arrowstyle='-|>', color=color, lw=1.4))
    ax.text(9.8, yc, 'reply', fontsize=7.4, color=color,
            ha='left', va='center')


def main():
    fig, ax = plt.subplots(figsize=(5.7, 2.0))
    ax.set_xlim(0, 10.8); ax.set_ylim(-0.95, 5.75); ax.axis('off')
    B = 5.6   # the barrier instant

    ax.text(0.06, 5.42, 'transparent fault tolerance: where does the durable write go?',
            fontsize=9.4, fontweight='bold', color=PAL['dark'], ha='left', va='center')

    # row 1: every prior transparent system stacks the write after the wait
    ax.text(1.6, 4.62, 'prior transparent systems: the write stacks after the wait',
            fontsize=7.6, color=PAL['red'], ha='left', va='center')
    bar(ax, 1.6, B, 3.72, 0.66, 'wait for delivery confirmation',
        PAL['blue'], '#eaf1fb')
    bar(ax, B, 8.75, 3.72, 0.66, 'durable write', PAL['red'], '#fdecea')
    io_arrows(ax, 4.05, 8.87, PAL['red'])
    ax.text(7.35, 3.3, 'reply held: $+2963\\,\\mu s$ measured', fontsize=7.6,
            color=PAL['red'], ha='center', va='center')

    # row 2: OneBarrier rides the copy inside the same wait
    ax.text(1.6, 2.55, 'OneBarrier: the write rides inside the same wait',
            fontsize=7.6, color=PAL['green'], ha='left', va='center')
    bar(ax, 1.6, B, 1.65, 0.66, 'the same wait  (commit barrier)',
        PAL['blue'], '#eaf1fb', fs=7.1)
    bar(ax, 1.6, 4.45, 0.82, 0.6, 'copy to $k{-}1$ backups', PAL['green'],
        '#eef6f0', fs=7.2)
    io_arrows(ax, 1.98, B + 0.12, PAL['green'])
    ax.text(7.6, 1.44, 'already durable: $+4.6\\,\\mu s$ measured',
            fontsize=7.6, color=PAL['green'], ha='center', va='center')

    # the one barrier, shared by both rows
    ax.plot([B, B], [-0.18, 4.38], ls='--', lw=1.0, color=PAL['blue'], zorder=1)
    ax.text(B, -0.62,
            'commit barrier: the network confirms delivery here, '
            'with or without fault tolerance  (time not to scale)',
            fontsize=7.0, color=PAL['blue'], ha='center', va='center')

    finish(fig, os.path.join(OUT, 'fig_headline.pdf'))


if __name__ == '__main__':
    main()
