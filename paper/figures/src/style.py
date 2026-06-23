"""Shared NeurIPS-style matplotlib config for OneBarrier figures."""
import matplotlib as mpl
import matplotlib.pyplot as plt

# muted, distinct, print-friendly palette
PAL = {
    'blue':   '#3b6fb6',
    'orange': '#e08214',
    'green':  '#4a9b6e',
    'red':    '#c0392b',
    'purple': '#8064a2',
    'teal':   '#2a9d8f',
    'gray':   '#6c757d',
    'lightblue': '#a6cee3',
    'lightgray': '#d9dde1',
    'dark':   '#222831',
}
SEQ = [PAL['blue'], PAL['orange'], PAL['green'], PAL['red'], PAL['purple'], PAL['gray']]

def setup():
    mpl.rcParams.update({
        'figure.dpi': 150,
        'savefig.dpi': 300,
        'savefig.bbox': 'tight',
        'savefig.pad_inches': 0.02,
        'pdf.fonttype': 42,          # editable text in PDF
        'ps.fonttype': 42,
        'font.family': 'serif',
        'font.serif': ['Nimbus Roman', 'Times New Roman', 'DejaVu Serif'],
        'mathtext.fontset': 'cm',
        'font.size': 10,
        'axes.titlesize': 10.5,
        'axes.labelsize': 10,
        'xtick.labelsize': 9,
        'ytick.labelsize': 9,
        'legend.fontsize': 8.7,
        'axes.spines.top': False,
        'axes.spines.right': False,
        'axes.linewidth': 0.8,
        'axes.grid': True,
        'grid.color': '#e6e8eb',
        'grid.linewidth': 0.7,
        'axes.axisbelow': True,
        'xtick.direction': 'out',
        'ytick.direction': 'out',
        'xtick.major.size': 3,
        'ytick.major.size': 3,
        'legend.frameon': False,
        'lines.linewidth': 1.8,
        'lines.markersize': 5,
        'figure.autolayout': False,
    })

def finish(fig, path):
    fig.savefig(path)
    plt.close(fig)
    print('wrote', path)
