#!/usr/bin/env python3
"""fig_loadsweep: the barrier overlap holds across the load range and into the tail.

(a) p99.9 commit latency vs offered throughput at the RDMA operating point: the
    FT-overlap tier is coincident with the reliable-1Pipe baseline across the whole
    range (FT marginal ~ 0, even at the p99.9 tail under load).
(b) Max sustainable throughput: overlap/baseline are apply-bound (1/apply), the
    serial-durability tier is bound by the out-of-regime serial storage write
    (1/write) -- a ~200x collapse at the Table-3 value of 100 us. (The 3000-us
    fsync figure belongs to the loopback calibration, ob-calibrate, not to the
    RDMA operating point plotted here.) The serial tier never enters the
    microsecond regime, so it cannot share the (a) axis.

Data: paper/figures/src/loadsweep.csv, from
    cargo run --release -p onebarrier --bin ob-sim -- csv > loadsweep.csv
Model params: apply = 0.5 us (ob-sim default); serial storage write = 100 us
(Table 3, out-of-regime tier at the RDMA operating point).
"""
import csv
import os

import matplotlib.pyplot as plt
from style import PAL, setup, finish

HERE = os.path.dirname(__file__)
OUT = os.path.join(HERE, '..')
CSV = os.path.join(HERE, 'loadsweep.csv')

APPLY_US = 0.5      # ob-sim default: state-machine apply (executor occupancy, overlap tier)
SERIAL_US = 100.0   # Table 3: serial stable-storage write (out-of-regime tier, RDMA point)


def load():
    thr, base, over = [], [], []
    with open(CSV) as f:
        for row in csv.DictReader(f):
            thr.append(float(row['throughput_ops_s']))
            base.append(float(row['baseline_p999_us']))
            over.append(float(row['overlap_p999_us']))
    return thr, base, over


def main():
    setup()
    thr, base, over = load()
    fig, (axa, axb) = plt.subplots(1, 2, figsize=(6.6, 2.7),
                                   gridspec_kw={'width_ratios': [2.05, 1.0]})

    # ---- (a) tail latency vs throughput: overlap coincides with baseline ----
    axa.plot(thr, base, color=PAL['gray'], lw=3.4, alpha=0.55,
             label='reliable-1Pipe baseline (no FT)', zorder=2)
    axa.plot(thr, over, color=PAL['green'], lw=1.7, ls=(0, (4, 2)),
             label='OneBarrier FT (rides the barrier)', zorder=3)
    axa.set_xscale('log')
    axa.set_xlabel('offered throughput (ops/s, log)')
    axa.set_ylabel(r'p99.9 commit latency ($\mu$s)')
    axa.set_ylim(0, max(over) * 1.18)
    axa.set_title('(a) FT marginal $\\approx$ 0 across load, into the tail', fontsize=9.6)
    axa.legend(loc='upper left', handlelength=2.4)
    axa.annotate('overlap curve sits exactly\non the baseline',
                 xy=(thr[len(thr) // 2], over[len(over) // 2]),
                 xytext=(2e3, max(over) * 0.72), fontsize=8.3, color=PAL['green'],
                 arrowprops=dict(arrowstyle='->', color=PAL['green'], lw=1.0))

    # ---- (b) sustainable throughput: apply-bound vs serial-storage-bound ----
    cap_overlap = 1.0e6 / APPLY_US   # ops/s
    cap_serial = 1.0e6 / SERIAL_US   # ops/s
    bars = axb.bar([0, 1], [cap_overlap, cap_serial],
                   color=[PAL['green'], PAL['red']], width=0.62, zorder=3)
    axb.set_yscale('log')
    axb.set_xticks([0, 1])
    axb.set_xticklabels(['overlap\n/ baseline', 'serial\nstorage'], fontsize=8.6)
    axb.set_ylabel('max sustainable\nthroughput (ops/s, log)')
    axb.set_ylim(1e3, cap_overlap * 6)
    axb.set_title('(b) durability tier\nthroughput ceiling', fontsize=9.6)
    for b, v in zip(bars, [cap_overlap, cap_serial]):
        if v >= 1e6:
            lbl = f'{v/1e6:.1f}M'
        elif v >= 1e3:
            lbl = f'{v/1e3:.0f}k'
        else:
            lbl = f'{v:.0f}'
        axb.text(b.get_x() + b.get_width() / 2, v * 1.35,
                 lbl, ha='center', va='bottom', fontsize=8.4)
    axb.annotate(f'{cap_overlap / cap_serial:.0f}$\\times$', xy=(0.5, cap_serial * 4),
                 ha='center', fontsize=10, color=PAL['dark'])

    fig.tight_layout(w_pad=1.6)
    finish(fig, os.path.join(OUT, 'fig_loadsweep.pdf'))


if __name__ == '__main__':
    main()
