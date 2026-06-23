#!/usr/bin/env python3
"""Generate all OneBarrier paper figures (NeurIPS-style) from the measured data."""
import os
import numpy as np
import matplotlib.pyplot as plt
from matplotlib.patches import FancyBboxPatch, FancyArrowPatch, Rectangle
from style import setup, finish, PAL, SEQ
setup()
OUT = os.path.join(os.path.dirname(__file__), '..')

def P(name): return os.path.join(OUT, name)

# ---------------------------------------------------------------------------
# F3. Overlap vs stack (RQ2) — the headline. Stacked latency showing the
# durable-replica write hidden UNDER the commit barrier vs stacked ON TOP.
# ---------------------------------------------------------------------------
def _overlap_panel(ax, title, deliv, dur_fabric, dur_stack, xmax, unit):
    y = [1, 0]
    ax.barh(y, [deliv, deliv], color=PAL['blue'], height=0.52)
    # in-fabric: durable rides UNDER the barrier (overlapped)
    ax.barh([1], [dur_fabric], left=[deliv*0.82], color=PAL['green'], height=0.26)
    # serial: durable stacks on top
    ax.barh([0], [dur_stack], left=[deliv], color=PAL['red'], height=0.52)
    ax.set_yticks(y)
    ax.set_yticklabels(['overlap\n(in-fabric)', 'stack\n(serial)'], fontsize=8.2)
    ax.set_xlabel(f'critical-path latency ({unit})')
    ax.set_title(title, fontsize=9.0)
    ax.set_xlim(0, xmax)
    ax.grid(axis='y', visible=False)

def fig_overlap():
    fig, axes = plt.subplots(1, 2, figsize=(5.6, 2.45))
    # left: MEASURED reproduction (loopback-UDP, us)
    _overlap_panel(axes[0], 'measured (loopback reproduction)',
                   2014, 4.59, 2963, 7500, 'µs')
    axes[0].annotate('+0.23%', xy=(2018, 1.0), xytext=(2900, 0.62),
                     fontsize=8.0, color=PAL['green'], va='center',
                     arrowprops=dict(arrowstyle='->', color=PAL['green'], lw=0.9))
    axes[0].text(5060, 0.0, '+2963 µs\n(commit 2×)', fontsize=7.6, color=PAL['red'],
                 ha='left', va='center')
    # right: PROJECTED RDMA operating point (us): reliable delivery ~21us barrier,
    # 1-RTT replica ~1.5us hidden under it; out-of-regime serial durability ~100us.
    _overlap_panel(axes[1], 'projected (RDMA, 1–2 µs RTT)',
                   21, 1.5, 100, 158, 'µs')
    axes[1].annotate('≈0%', xy=(21, 1.0), xytext=(40, 0.62),
                     fontsize=8.0, color=PAL['green'], va='center',
                     arrowprops=dict(arrowstyle='->', color=PAL['green'], lw=0.9))
    axes[1].text(124, 0.0, '+100 µs', fontsize=7.6, color=PAL['red'], ha='left', va='center')
    # shared legend
    from matplotlib.patches import Patch
    handles = [Patch(color=PAL['blue'], label='reliable-delivery barrier'),
               Patch(color=PAL['green'], label='durable write (overlapped)'),
               Patch(color=PAL['red'], label='durable write (stacked)')]
    fig.legend(handles=handles, loc='lower center', ncol=3, fontsize=7.3,
               bbox_to_anchor=(0.5, -0.08))
    fig.subplots_adjust(wspace=0.34, bottom=0.30, top=0.86)
    finish(fig, P('fig_overlap.pdf'))

# ---------------------------------------------------------------------------
# F6. Recovery time vs log length (linear, exact reconstruction).
# ---------------------------------------------------------------------------
def fig_recovery_time():
    n = np.array([10, 50, 100, 200, 500, 1000])         # k requests
    t = np.array([35, 59, 87, 146, 269, 536])           # ms
    fig, ax = plt.subplots(figsize=(3.5, 2.5))
    ax.plot(n, t, '-o', color=PAL['blue'], label='measured replay')
    # linear fit overlay
    a, b = np.polyfit(n, t, 1)
    ax.plot(n, a*n+b, '--', color=PAL['gray'], lw=1.2, label=f'linear ({a:.2f} ms / 1k req)')
    ax.set_xlabel('captured request log (×1000 requests)')
    ax.set_ylabel('recovery time (ms)')
    ax.set_xticks([0, 250, 500, 750, 1000])
    ax.legend(loc='upper left')
    ax.annotate('exact: recovered keys = live keys', xy=(500, 269), xytext=(120, 430),
                fontsize=7.8, color=PAL['green'])
    finish(fig, P('fig_recovery_time.pdf'))

# ---------------------------------------------------------------------------
# F8. Share-nothing sharding beats multithreading (memcached).
# ---------------------------------------------------------------------------
def fig_sharding():
    labels = ['-t1', '-t2', '-t4', '-t8', '-t1\n+libOS', '4×-t1\nshards']
    vals   = [342, 575, 821, 1239, 302, 1008]           # k ops/s
    colors = [PAL['gray'], PAL['gray'], PAL['blue'], PAL['gray'], PAL['orange'], PAL['green']]
    fig, ax = plt.subplots(figsize=(3.5, 2.5))
    bars = ax.bar(labels, vals, color=colors, width=0.66)
    ax.axhline(821, color=PAL['blue'], ls=':', lw=1.0)
    ax.set_ylabel('throughput (k ops/s)')
    ax.set_ylim(0, 1400)
    for b, v in zip(bars, vals):
        ax.text(b.get_x()+b.get_width()/2, v+25, f'{v}', ha='center', fontsize=7.6)
    ax.annotate('shards beat one\n4-worker process\n(no lock contention)',
                xy=(5, 1008), xytext=(2.3, 1180), fontsize=7.5, color=PAL['green'],
                arrowprops=dict(arrowstyle='->', color=PAL['green'], lw=0.9))
    ax.grid(axis='x', visible=False)
    finish(fig, P('fig_sharding.pdf'))

# ---------------------------------------------------------------------------
# F9. Deterministic scheduler: correct but collapses throughput.
# ---------------------------------------------------------------------------
def fig_detsched():
    fig, ax = plt.subplots(figsize=(3.5, 2.5))
    labels = ['baseline', '+detsched']
    vals = [24.53, 7.61]
    bars = ax.bar(labels, vals, color=[PAL['gray'], PAL['red']], width=0.5)
    ax.set_ylabel('lock throughput (M locks/s)')
    for b, v in zip(bars, vals):
        ax.text(b.get_x()+b.get_width()/2, v+0.4, f'{v:.1f}', ha='center', fontsize=8)
    ax.annotate('3.2× slower\n(4-thread microbench)', xy=(1, 7.6), xytext=(0.55, 16),
                fontsize=8, color=PAL['red'],
                arrowprops=dict(arrowstyle='->', color=PAL['red'], lw=0.9))
    ax.text(0.5, 22.5, 'on a contended server\n(memcached -t4): >1000× slower → use shards',
            ha='center', fontsize=7.3, color=PAL['dark'],
            bbox=dict(boxstyle='round,pad=0.3', fc='#fff3cd', ec=PAL['orange'], lw=0.8))
    ax.set_ylim(0, 27)
    ax.grid(axis='x', visible=False)
    finish(fig, P('fig_detsched.pdf'))

# ---------------------------------------------------------------------------
# F10. Snapshot-interval tradeoff (RQ8), dual axis.
# ---------------------------------------------------------------------------
def fig_interval():
    interval = np.array([64, 512, 4096, 100000])
    apply_us = np.array([1.215, 0.439, 0.407, 0.407])
    recov_us = np.array([20.7, 58.6, 134.1, 5856.4])
    fig, ax1 = plt.subplots(figsize=(3.5, 2.5))
    ax1.plot(interval, apply_us, '-o', color=PAL['blue'], label='steady apply cost')
    ax1.set_xscale('log'); ax1.set_yscale('log')
    ax1.set_xlabel('snapshot interval (ops)')
    ax1.set_ylabel('apply µs/op', color=PAL['blue'])
    ax1.tick_params(axis='y', labelcolor=PAL['blue'])
    ax2 = ax1.twinx(); ax2.spines['top'].set_visible(False)
    ax2.plot(interval, recov_us, '-s', color=PAL['red'], label='recovery time')
    ax2.set_yscale('log')
    ax2.set_ylabel('recovery µs', color=PAL['red'])
    ax2.tick_params(axis='y', labelcolor=PAL['red'])
    ax2.grid(False)
    ax1.annotate('3× steady cost', xy=(64, 1.215), xytext=(120, 1.6), fontsize=7.3, color=PAL['blue'])
    ax2.annotate('283× recovery', xy=(100000, 5856), xytext=(2000, 5000), fontsize=7.3, color=PAL['red'])
    finish(fig, P('fig_interval.pdf'))

# ---------------------------------------------------------------------------
# F11. Passive vs active SMR: execution CPU.
# ---------------------------------------------------------------------------
def fig_passive():
    # RQ5 measured: execution CPU (ms) for active SMR vs OneBarrier passive
    reps   = [2, 3, 5, 7]
    active = [205.9, 309.5, 519.5, 729.7]
    passive= [105.7, 108.7, 114.5, 123.1]
    save   = [49, 65, 78, 83]
    fig, ax = plt.subplots(figsize=(3.5, 2.5))
    x = np.arange(len(reps)); w = 0.38
    ax.bar(x-w/2, active, w, color=PAL['red'], label='active SMR ($N$ execute)')
    ax.bar(x+w/2, passive, w, color=PAL['green'], label='OneBarrier (passive)')
    for i, s in enumerate(save):
        ax.text(i, active[i]+18, f'−{s}%', ha='center', fontsize=7.8, color=PAL['green'])
    ax.set_xticks(x); ax.set_xticklabels([f'{r}' for r in reps])
    ax.set_xlabel('replicas')
    ax.set_ylabel('execution CPU (ms)')
    ax.set_ylim(0, 830)
    ax.legend(loc='upper left', fontsize=7.6)
    ax.grid(axis='x', visible=False)
    finish(fig, P('fig_passive.pdf'))

def fig_competitors():
    # M4 measured: throughput vs producers, OneBarrier vs HyCoR vs LLFT (M ops/s)
    prod = [1, 2, 4, 8]
    ob   = [2.85, 4.95, 6.12, 12.11]
    hycor= [1.27, 2.87, 4.34, 7.89]
    llft = [2.06, 5.14, 5.64, 10.34]
    fig, ax = plt.subplots(figsize=(3.5, 2.5))
    x = np.arange(len(prod)); w = 0.26
    ax.bar(x-w, ob,    w, color=PAL['green'], label='OneBarrier')
    ax.bar(x,   llft,  w, color=PAL['blue'],  label='LLFT (host sequencer)')
    ax.bar(x+w, hycor, w, color=PAL['red'],   label='HyCoR (order-log)')
    ax.set_xticks(x); ax.set_xticklabels([f'{p}' for p in prod])
    ax.set_xlabel('concurrent producers')
    ax.set_ylabel('throughput (M ops/s)')
    ax.set_ylim(0, 14)
    ax.legend(loc='upper left', fontsize=7.3)
    ax.annotate('1.5–2.2× vs HyCoR\n(no order-log)', xy=(3-w, 7.89), xytext=(1.3, 12.6),
                fontsize=7.3, color=PAL['green'],
                arrowprops=dict(arrowstyle='->', color=PAL['green'], lw=0.9))
    ax.grid(axis='x', visible=False)
    finish(fig, P('fig_competitors.pdf'))

# ---------------------------------------------------------------------------
# F7. libOS steady-state overhead (redis pipelined + nginx).
# ---------------------------------------------------------------------------
def fig_overhead():
    fig, ax = plt.subplots(figsize=(3.5, 2.5))
    cfgs = ['base', '+clock', '+rng', '+capture']
    # nginx req/s normalized to baseline (realistic, non-pipelined)
    nginx = [1.00, 0.98, 1.00, 0.945]
    bars = ax.bar(cfgs, [v*100 for v in nginx], color=[PAL['gray'], PAL['blue'], PAL['teal'], PAL['orange']], width=0.6)
    ax.set_ylabel('throughput (% of native)')
    ax.set_ylim(80, 104)
    ax.axhline(100, color=PAL['gray'], ls=':', lw=0.9)
    for b, v in zip(bars, nginx):
        ax.text(b.get_x()+b.get_width()/2, v*100+0.4, f'{v*100:.0f}', ha='center', fontsize=7.8)
    ax.set_title('Nginx (ApacheBench): libOS overhead', fontsize=9.2)
    ax.grid(axis='x', visible=False)
    finish(fig, P('fig_overhead.pdf'))

# ---------------------------------------------------------------------------
# F13. Real RDMA verbs over SoftRoCE: per-op latency in the operating point.
# ---------------------------------------------------------------------------
def fig_softroce():
    fig, ax = plt.subplots(figsize=(3.5, 2.5))
    ax.axhspan(1, 2, color=PAL['lightblue'], alpha=0.5, label='1Pipe operating point (1–2 µs)')
    labels = ['RDMA\nWRITE', 'RC\npingpong\n(RTT)']
    vals = [1.56, 11.78]
    bars = ax.bar(labels, vals, color=[PAL['green'], PAL['gray']], width=0.5)
    for b, v in zip(bars, vals):
        ax.text(b.get_x()+b.get_width()/2, v+0.3, f'{v:.2f} µs', ha='center', fontsize=8)
    ax.set_ylabel('latency (µs)')
    ax.set_yscale('log'); ax.set_ylim(0.8, 20)
    ax.legend(loc='upper left', fontsize=7.6)
    ax.set_title('Real ibverbs over SoftRoCE', fontsize=9.2)
    ax.grid(axis='x', visible=False)
    finish(fig, P('fig_softroce.pdf'))

# ---------------------------------------------------------------------------
# F12. CRIU checkpoint cost + PostgreSQL multi-process result.
# ---------------------------------------------------------------------------
def fig_criu():
    fig, ax = plt.subplots(figsize=(3.5, 2.5))
    size = np.array([30, 100, 200, 300])
    t = np.array([37, 62, 95, 121])
    ax.plot(size, t, '-o', color=PAL['purple'])
    ax.set_xlabel('checkpoint image size (MB)')
    ax.set_ylabel('CRIU checkpoint time (ms)')
    ax.set_ylim(0, 140)
    ax.text(0.50, 0.16, 'PostgreSQL (multi-process):\ndump 50 images → restore →\ndata byte-identical, server live',
            transform=ax.transAxes, fontsize=7.3, ha='center',
            bbox=dict(boxstyle='round,pad=0.3', fc='#eef6f0', ec=PAL['green'], lw=0.8))
    finish(fig, P('fig_criu.pdf'))

# ---------------------------------------------------------------------------
# F5. Five-app deterministic recovery matrix (live=replay; control differs).
# ---------------------------------------------------------------------------
def fig_recovery_matrix():
    apps = ['Redis', 'Memcached', 'Nginx', 'Node.js', 'PostgreSQL']
    cols = ['time', 'RNG', 'full state', 'recovered\n= live', 'control\ndiffers']
    # 1 = yes/identical (good), for control-differs 1 = differs (good, expected)
    M = np.array([
        [1,1,1,1,1],
        [1,0.5,1,1,1],   # memcached: RNG n/a-ish
        [1,0.5,1,1,1],
        [1,1,1,1,1],
        [1,0.5,1,1,1],   # postgres via CRIU
    ])
    fig, ax = plt.subplots(figsize=(4.3, 2.4))
    for i in range(len(apps)):
        for j in range(len(cols)):
            v = M[i, j]
            yy = len(apps)-1-i
            if v == 1:        # identical / yes
                ax.scatter(j, yy, s=120, marker='o', color=PAL['green'], zorder=3)
                ax.scatter(j, yy, s=30, marker='o', color='white', zorder=4)
            elif v == 0.5:    # n/a
                ax.scatter(j, yy, s=80, marker='_', color=PAL['gray'], lw=2, zorder=3)
            else:             # no
                ax.scatter(j, yy, s=120, marker='X', color=PAL['red'], zorder=3)
    ax.set_xticks(range(len(cols))); ax.set_xticklabels(cols, fontsize=8.3)
    ax.set_yticks(range(len(apps))); ax.set_yticklabels(apps[::-1], fontsize=9)
    ax.set_xlim(-0.5, len(cols)-0.5); ax.set_ylim(-0.5, len(apps)-0.5)
    for s in ax.spines.values(): s.set_visible(False)
    ax.tick_params(length=0); ax.grid(False)
    ax.set_xticks(np.arange(-0.5, len(cols), 1), minor=True)
    ax.set_yticks(np.arange(-0.5, len(apps), 1), minor=True)
    ax.grid(which='minor', color='#e6e8eb', lw=0.8); ax.grid(which='major', visible=False)
    finish(fig, P('fig_recovery_matrix.pdf'))

if __name__ == '__main__':
    fig_overlap()
    fig_recovery_time()
    fig_sharding()
    fig_detsched()
    fig_interval()
    fig_passive()
    fig_competitors()
    fig_overhead()
    fig_softroce()
    fig_criu()
    fig_recovery_matrix()
    print('data figures done')
