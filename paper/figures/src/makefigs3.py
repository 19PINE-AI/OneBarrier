#!/usr/bin/env python3
"""Extension/generality figures (§sec:ext) — NeurIPS-style, from measured data.
Designed to absorb the number-dense prose: the per-app recovery matrix, the
partition scaling, the two-path taxonomy, and the live/replay/control determinism."""
import os
import numpy as np
import matplotlib.pyplot as plt
from matplotlib.patches import FancyBboxPatch, FancyArrowPatch
from style import setup, finish, PAL, SEQ
setup()
OUT = os.path.join(os.path.dirname(__file__), '..')
def P(name): return os.path.join(OUT, name)


# ---------------------------------------------------------------------------
# Extension recovery matrix: every extension app x {recovered=live, control
# differs, exactly-once/state}. Green = holds; dash = n/a for the path.
# ---------------------------------------------------------------------------
def fig_ext_matrix():
    apps = ['Redis Streams', 'Kafka partitions', 'Mosquitto MQTT', 'SQLite',
            'MySQL/MariaDB', 'Click NF', 'microservice', 'dnsmasq',
            'lighttpd', 'HAProxy']
    cols = ['recovered\n= live', 'control\ndiffers', 'exactly-once\n/ state']
    # 1 = holds; 0.5 = n/a (CRIU checkpoint path has no replay-vs-control axis)
    M = np.array([
        [1, 1, 1],   # redis streams
        [1, 1, 1],   # kafka partitions
        [1, 1, 1],   # mosquitto
        [1, 1, 1],   # sqlite
        [1, 0.5, 1], # mariadb (CRIU)
        [1, 1, 1],   # click nf
        [1, 1, 1],   # microservice
        [1, 1, 1],   # dnsmasq
        [1, 1, 1],   # lighttpd
        [1, 1, 1],   # haproxy
    ])
    fig, ax = plt.subplots(figsize=(3.5, 3.3))
    for i in range(len(apps)):
        for j in range(len(cols)):
            v = M[i, j]; yy = len(apps)-1-i
            if v == 1:
                ax.scatter(j, yy, s=130, marker='o', color=PAL['green'], zorder=3)
                ax.scatter(j, yy, s=28, marker='o', color='white', zorder=4)
            else:
                ax.scatter(j, yy, s=90, marker='_', color=PAL['gray'], lw=2.2, zorder=3)
    ax.set_xticks(range(len(cols))); ax.set_xticklabels(cols, fontsize=8.2)
    ax.set_yticks(range(len(apps))); ax.set_yticklabels(apps[::-1], fontsize=8.6)
    ax.set_xlim(-0.5, len(cols)-0.5); ax.set_ylim(-0.5, len(apps)-0.5)
    for s in ax.spines.values(): s.set_visible(False)
    ax.tick_params(length=0); ax.grid(False)
    ax.set_xticks(np.arange(-0.5, len(cols), 1), minor=True)
    ax.set_yticks(np.arange(-0.5, len(apps), 1), minor=True)
    ax.grid(which='minor', color='#e6e8eb', lw=0.8)
    ax.xaxis.tick_top(); ax.xaxis.set_label_position('top')
    finish(fig, P('fig_ext_matrix.pdf'))


# ---------------------------------------------------------------------------
# Kafka partition model: publish throughput scales near-linearly with
# share-nothing partitions (measured 1/2/4/8 vs ideal-linear).
# ---------------------------------------------------------------------------
def fig_partition_scale():
    parts = np.array([1, 2, 4, 8])
    tput = np.array([20157, 38417, 82637, 158427]) / 1000.0  # k msg/s
    ideal = tput[0] * parts
    fig, ax = plt.subplots(figsize=(3.5, 2.5))
    ax.plot(parts, ideal, '--', color=PAL['gray'], lw=1.4, label='ideal linear', zorder=2)
    ax.plot(parts, tput, '-o', color=PAL['blue'], label='measured', zorder=3)
    for x, y, m in zip(parts, tput, [1.0, 1.9, 4.1, 7.9]):
        ax.annotate(f'{m:.1f}\\u00d7'.replace('\\u00d7', '×'), (x, y),
                    textcoords='offset points', xytext=(4, -10), fontsize=8.2,
                    color=PAL['blue'])
    ax.set_xscale('log', base=2); ax.set_xticks(parts); ax.set_xticklabels(parts)
    ax.set_xlabel('share-nothing partitions'); ax.set_ylabel('publish throughput (k msg/s)')
    ax.legend(loc='upper left')
    ax.set_ylim(0, 175)
    finish(fig, P('fig_partition_scale.pdf'))


# ---------------------------------------------------------------------------
# Two-path taxonomy: the fit-test routes each app to deterministic replay
# (order-log-free, the novel path) or the general CRIU checkpoint path.
# ---------------------------------------------------------------------------
def fig_taxonomy():
    fig, ax = plt.subplots(figsize=(7.2, 3.0))
    ax.set_xlim(0, 10.2); ax.set_ylim(0, 5.5); ax.axis('off')

    def box(x, y, w, h, color, lw=1.4, fc='white'):
        ax.add_patch(FancyBboxPatch((x, y), w, h, boxstyle='round,pad=0.02,rounding_size=0.08',
                     ec=color, fc=fc, lw=lw, zorder=2))
    def txt(x, y, s, **kw):
        ax.text(x, y, s, ha=kw.pop('ha', 'center'), va=kw.pop('va', 'center'), zorder=3, **kw)

    # decision node
    box(0.15, 2.25, 2.0, 1.05, PAL['dark'], fc='#f3f4f6')
    txt(1.15, 2.78, 'unmodified\nserver', fontsize=9.0, weight='bold')

    # path A: deterministic replay (top)
    box(3.0, 2.95, 7.0, 2.4, PAL['green'], fc='#f1f8f4')
    txt(6.5, 5.02, 'deterministic-replay path  (order-log-free, $\\approx$0 over the fabric)',
        fontsize=8.6, weight='bold', color=PAL['green'])
    txt(6.5, 4.62, 'share-nothing $\\cdot$ single-thread $\\cdot$ socket-based',
        fontsize=7.6, color=PAL['gray'])
    a_apps = ['Redis', 'Memcached', 'Nginx', 'Node.js', 'Redis Streams', 'Mosquitto',
              'SQLite', 'Click NF', 'microservice', 'dnsmasq', 'lighttpd', 'HAProxy']
    for k, name in enumerate(a_apps):
        cx = 3.35 + (k % 4) * 1.62; cy = 4.12 - (k // 4) * 0.42
        txt(cx, cy, name, fontsize=7.5, ha='left', color=PAL['dark'])

    # path B: CRIU checkpoint (bottom)
    box(3.0, 0.2, 7.0, 1.95, PAL['orange'], fc='#fdf5ec')
    txt(6.5, 1.78, 'CRIU checkpoint path  (any binary, checkpoint-only)',
        fontsize=8.6, weight='bold', color=PAL['orange'])
    txt(6.5, 1.38, 'shared-everything $\\cdot$ multi-thread / multi-process',
        fontsize=7.6, color=PAL['gray'])
    b_apps = ['PostgreSQL', 'MySQL / MariaDB', 'other shared-everything']
    for k, name in enumerate(b_apps):
        txt(3.6 + k * 2.15, 0.72, name, fontsize=7.5, ha='left', color=PAL['dark'])

    ax.add_patch(FancyArrowPatch((2.15, 3.0), (3.0, 3.9), arrowstyle='-|>',
                 mutation_scale=11, color=PAL['green'], lw=1.5, zorder=1))
    ax.add_patch(FancyArrowPatch((2.15, 2.55), (3.0, 1.35), arrowstyle='-|>',
                 mutation_scale=11, color=PAL['orange'], lw=1.5, zorder=1))
    txt(2.45, 3.62, 'deterministic', fontsize=7.0, color=PAL['green'], rotation=30)
    txt(2.4, 1.75, 'shared-everything', fontsize=7.0, color=PAL['orange'], rotation=-32)
    finish(fig, P('fig_taxonomy.pdf'))


# ---------------------------------------------------------------------------
# Determinism across apps: each app's time/value-derived probe — recovered
# replay lands EXACTLY on live (overlapping), the no-libOS control drifts.
# Shown as deviation from live (live=replay=0; control offset to the right).
# ---------------------------------------------------------------------------
def fig_ext_determinism():
    # (label, control deviation in its own unit, unit string)
    rows = [
        ('Redis Streams (entry id ms)', 4752, 'ms'),
        ('SQLite (row timestamp s)',    6.0,  's'),
        ('Mosquitto (broker uptime s)', 4,    's'),
        ('dnsmasq (cached TTL s)',      3,    's'),
        ('Click NF (conntrack ns)',     5.24, 's'),
        ('lighttpd (Date header s)',    5,    's'),
        ('HAProxy (Date header s)',     6,    's'),
        ('microservice (order ts s)',   6.0,  's'),
    ]
    labels = [r[0] for r in rows]
    n = len(rows)
    fig, ax = plt.subplots(figsize=(3.6, 3.1))
    for i, (lab, dev, unit) in enumerate(rows):
        yy = n - 1 - i
        ax.plot([0, 1], [yy, yy], color='#e6e8eb', lw=6, solid_capstyle='round', zorder=1)
        # live & replay: identical, at left
        ax.scatter(0, yy, s=120, marker='o', color=PAL['green'], zorder=3)
        ax.scatter(0, yy, s=26, marker='o', color='white', zorder=4)
        # control: drifts (position is schematic-normalized; magnitude in label)
        ax.scatter(1, yy, s=95, marker='X', color=PAL['red'], zorder=3)
        ax.text(1.07, yy, f'+{dev:g}{unit}', va='center', fontsize=7.6, color=PAL['red'])
    ax.set_yticks(range(n)); ax.set_yticklabels(labels[::-1], fontsize=8.0)
    ax.set_xticks([0, 1]); ax.set_xticklabels(['live = replay', 'control'], fontsize=8.6)
    ax.set_xlim(-0.18, 1.5); ax.set_ylim(-0.6, n-0.4)
    ax.grid(False)
    for s in ax.spines.values(): s.set_visible(False)
    ax.tick_params(length=0)
    finish(fig, P('fig_ext_determinism.pdf'))


# ---------------------------------------------------------------------------
# Barrier-coincidence timeline: the durable-replica write (1-RTT replication)
# completes INSIDE the reliable-delivery commit barrier the fabric crosses anyway,
# so FT adds no round trip -- the "by construction" thesis, as a timeline.
# ---------------------------------------------------------------------------
def fig_barrier_timeline():
    fig, ax = plt.subplots(figsize=(6.6, 2.3))
    ax.set_xlim(0, 10.4); ax.set_ylim(0, 3.2); ax.axis('off')
    def bar(x0, x1, y, h, color, fc=None, lw=1.4):
        from matplotlib.patches import FancyBboxPatch
        ax.add_patch(FancyBboxPatch((x0, y), x1-x0, h, boxstyle='round,pad=0.0,rounding_size=0.05',
                     ec=color, fc=fc if fc else color, lw=lw, zorder=2))
    # time axis
    ax.annotate('', xy=(10.2, 0.25), xytext=(0.2, 0.25),
                arrowprops=dict(arrowstyle='-|>', color=PAL['gray'], lw=1.2))
    ax.text(10.2, 0.46, 'time', fontsize=8, color=PAL['gray'], ha='right')
    for x, lab in [(0.6, '0'), (4.0, '0.5 RTT'), (7.4, '1 RTT'), (9.6, '1.5 RTT')]:
        ax.plot([x, x], [0.18, 0.32], color=PAL['gray'], lw=0.9)
        ax.text(x, -0.05, lab, fontsize=7.4, color=PAL['gray'], ha='center')
    # reliable-delivery commit barrier (what the fabric pays anyway)
    bar(0.6, 9.6, 2.35, 0.5, PAL['blue'], fc='#dbe6f3')
    ax.text(5.1, 2.6, 'reliable-delivery commit barrier  ·  1.5 RTT',
            fontsize=8.2, ha='center', va='center', color=PAL['blue'], weight='bold')
    ax.text(5.1, 2.18, 'the fabric crosses this for communication correctness, with or without FT',
            fontsize=7.0, ha='center', color=PAL['gray'])
    # 1-RTT replication, hidden UNDER the barrier
    bar(0.6, 7.4, 1.5, 0.5, PAL['green'], fc='#e3f1ea')
    ax.text(4.0, 1.75, 'recovery-log replication  ·  1 RTT',
            fontsize=8.2, ha='center', va='center', color=PAL['green'])
    # output release marker at the barrier
    ax.scatter([9.6], [1.0], s=70, marker='D', color=PAL['dark'], zorder=4)
    ax.text(9.55, 0.62, 'output released', fontsize=7.4, ha='center', color=PAL['dark'])
    ax.annotate('durable here', xy=(7.4, 1.55), xytext=(8.0, 1.1), fontsize=7.4,
                color=PAL['green'], arrowprops=dict(arrowstyle='->', color=PAL['green'], lw=1))
    ax.text(5.1, 3.05, 'output-commit hold $=$ reliable-delivery barrier  $\\Rightarrow$  FT adds no round trip',
            fontsize=8.8, ha='center', color=PAL['dark'], weight='bold')
    finish(fig, P('fig_barrier_timeline.pdf'))


# ---------------------------------------------------------------------------
# Prior-art positioning: operating point (ms -> us) x approach (rewrite vs
# transparent). OneBarrier owns transparent + microsecond + passive.
# ---------------------------------------------------------------------------
def fig_positioning():
    fig, ax = plt.subplots(figsize=(5.2, 3.0))
    pts = [
        ('Remus / VMware-FT', 0.7, 0.78, PAL['gray']),
        ('LLFT', 1.2, 0.86, PAL['gray']),
        ('HyCoR / RRC', 1.0, 0.70, PAL['gray']),
        ('NOPaxos / Eris', 2.7, 0.45, PAL['purple']),
        ('Temporal / Flink', 0.9, 0.16, PAL['orange']),
        ('OneBarrier', 2.9, 0.88, PAL['green']),
    ]
    for name, x, y, c in pts:
        big = name == 'OneBarrier'
        ax.scatter(x, y, s=190 if big else 80, color=c, zorder=3,
                   edgecolor='white', linewidth=1.2 if big else 0.6)
        dy = 0.075 if not big else 0.085
        ax.annotate(name, (x, y), textcoords='offset points',
                    xytext=(0, 9 if big else 7), ha='center',
                    fontsize=8.6 if big else 7.8,
                    weight='bold' if big else 'normal',
                    color=c if big else PAL['dark'])
    ax.axhline(0.33, color='#d9dde1', lw=1.0, ls='--', zorder=1)
    ax.text(0.18, 0.27, 'rewrite the app', fontsize=7.6, color=PAL['orange'])
    ax.text(0.18, 0.94, 'transparent (unmodified)', fontsize=7.6, color=PAL['green'])
    ax.set_xlim(0.2, 3.4); ax.set_ylim(0.05, 1.05)
    ax.set_xticks([0.7, 2.9]); ax.set_xticklabels(['millisecond\n(software-ordered)', 'microsecond\n(in-network order)'])
    ax.set_yticks([])
    ax.set_xlabel('operating point'); ax.grid(False)
    for s in ['left', 'right', 'top']: ax.spines[s].set_visible(False)
    finish(fig, P('fig_positioning.pdf'))


if __name__ == '__main__':
    fig_ext_matrix()
    fig_partition_scale()
    fig_taxonomy()
    fig_ext_determinism()
    fig_barrier_timeline()
    fig_positioning()
    print('extension figures done')
