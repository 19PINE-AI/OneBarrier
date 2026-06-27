#!/usr/bin/env python3
"""fig_recovery_load: recovery time vs sustained live load (recovery model,
`ob-recovery`). A recovering replica replays the durable log forward to the live
cut; while replay outruns the live stream (s = R_replay/R_live > 1) recovery is
sub-millisecond, but plain replay LIVELOCKS once the live rate reaches replay
capacity. The fabric's barrier-hold backpressure (throttle senders to the
recovering node) doubles the sustainable live rate and keeps recovery bounded.

This is the closed-form catch-up model (clearly labelled, like the RDMA overlap
model); the real engine's recovery under concurrent client load is measured
separately by the crash-injection test (ob-app-jepsen).

Data: paper/figures/src/recovery_load.csv, from
    cargo run --release -p onebarrier --bin ob-recovery -- csv > recovery_load.csv
"""
import csv
import os

import matplotlib.pyplot as plt
from style import PAL, setup, finish

HERE = os.path.dirname(__file__)
OUT = os.path.join(HERE, '..')
CSV = os.path.join(HERE, 'recovery_load.csv')


def load():
    lr, plain, bp = [], [], []
    rate = 4.0
    with open(CSV) as f:
        for row in csv.DictReader(f):
            lr.append(float(row['live_rate_ops_us']))
            plain.append(float(row['plain_us']) if row['plain_us'] else None)
            bp.append(float(row['backpressure_us']) if row['backpressure_us'] else None)
            rate = float(row['replay_rate'])
    return lr, plain, bp, rate


def seg(xs, ys):
    return [(x, y) for x, y in zip(xs, ys) if y is not None]


def main():
    setup()
    lr, plain, bp, rate = load()
    fig, ax = plt.subplots(figsize=(4.7, 2.9))

    # livelock region for PLAIN replay: live_rate >= replay capacity
    ax.axvspan(rate, max(lr), color=PAL['red'], alpha=0.06, zorder=0)
    ax.axvline(rate, color=PAL['gray'], lw=0.9, ls=':', zorder=1)
    ax.text(rate * 1.02, 4.2e4, 'plain livelocks\n($R_{live}\\geq R_{replay}$)',
            fontsize=8.0, color=PAL['red'], va='top')

    px = seg(lr, plain)
    ax.plot([x for x, _ in px], [y for _, y in px], color=PAL['red'], lw=1.9,
            marker='o', ms=3, label='plain replay')
    bx = seg(lr, bp)
    ax.plot([x for x, _ in bx], [y for _, y in bx], color=PAL['green'], lw=1.9,
            marker='s', ms=3, label='+ barrier-hold backpressure')

    ax.set_yscale('log')
    ax.set_xlabel(r'sustained live load (ops/$\mu$s)')
    ax.set_ylabel(r'recovery time ($\mu$s, log)')
    ax.set_ylim(5e2, 1e5)
    ax.set_xlim(0, max(lr))
    ax.set_title('Backpressure doubles the load a recovery can outrun', fontsize=9.6)
    ax.legend(loc='lower right')

    finish(fig, os.path.join(OUT, 'fig_recovery_load.pdf'))


if __name__ == '__main__':
    main()
