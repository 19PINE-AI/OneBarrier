#!/usr/bin/env python3
"""Headline results figure (page 1, below the abstract).

(a) Marginal cost of transparent FT on the commit critical path (loopback
    reproduction, p50): the durable recovery-log write completes inside the
    fabric's reliable-delivery barrier, so FT adds 4.59 us (+0.23% of the
    2014 us delivery p50); serial durability stacks +2963 us (Table 2).
(b) Throughput at 8 concurrent producers against same-engine reimplementations
    of the mechanisms OneBarrier avoids: HyCoR-style order-log (1.5x) and an
    LLFT-style host sequencer (data of fig_competitors, 8-producer group).
"""
import os
import numpy as np
import matplotlib.pyplot as plt
from style import setup, finish, PAL
setup()
OUT = os.path.join(os.path.dirname(__file__), '..')


def main():
    fig, (axa, axb) = plt.subplots(1, 2, figsize=(5.7, 2.0),
                                   gridspec_kw={'width_ratios': [1.45, 1.0]})

    # ---- (a) commit-path p50: durability rides the barrier or stacks on it ----
    tiers = ['durable write\nafter barrier (fsync)',
             'OneBarrier: durable\nwrite inside barrier',
             'no fault\ntolerance']
    vals = [6016, 2018, 2014]
    colors = [PAL['red'], PAL['green'], PAL['gray']]
    bars = axa.barh([0, 1, 2], vals, color=colors, height=0.62)
    axa.set_yticks([0, 1, 2])
    axa.set_yticklabels(tiers, fontsize=7.6)
    axa.set_xlabel('commit p50 (µs)', fontsize=8.6)
    axa.set_xlim(0, 8600)
    axa.set_title('(a) ride vs. stack: fault-tolerance cost', fontsize=9.0)
    axa.tick_params(axis='x', labelsize=7.8)
    axa.grid(axis='y', visible=False)
    axa.text(2014 + 160, 2, '2014', va='center', fontsize=7.6, color=PAL['dark'])
    axa.text(2018 + 160, 1, '2018\n+4.6 µs', va='center', fontsize=7.4,
             color=PAL['green'])
    axa.text(6016 + 160, 0, '6016\n+2963 µs', va='center', fontsize=7.4,
             color=PAL['red'])

    # ---- (b) vs the mechanisms the conditions eliminate (8 producers) ----
    names = ['OneBarrier', 'host\nsequencer', 'order log']
    thr = [12.11, 10.34, 7.89]
    bcolors = [PAL['green'], PAL['blue'], PAL['red']]
    bars = axb.bar([0, 1, 2], thr, color=bcolors, width=0.62)
    axb.set_xticks([0, 1, 2])
    axb.set_xticklabels(names, fontsize=7.6)
    axb.set_ylabel('M ops/s', fontsize=8.6)
    axb.set_ylim(0, 14.6)
    axb.set_title('(b) vs. mechanisms eliminated', fontsize=9.0)
    axb.tick_params(axis='y', labelsize=7.8)
    axb.grid(axis='x', visible=False)
    for x, v in zip([0, 1, 2], thr):
        axb.text(x, v + 0.35, f'{v:.1f}', ha='center', va='bottom',
                 fontsize=7.6, color=PAL['dark'])
    axb.annotate('1.5×', xy=(2, 9.6), xytext=(2, 13.1), ha='center',
                 fontsize=8.0, color=PAL['dark'],
                 arrowprops=dict(arrowstyle='->', color=PAL['gray'], lw=0.9))

    fig.tight_layout(w_pad=2.2)
    finish(fig, os.path.join(OUT, 'fig_headline.pdf'))


if __name__ == '__main__':
    main()
