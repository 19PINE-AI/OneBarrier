#!/usr/bin/env python3
"""Geometry audit for the generated figures.

Checks nothing renders outside the canvas, no label is wider than the box it
sits in, and no two text elements collide. Run after editing make_figures.py.
"""
import matplotlib; matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.patches import FancyBboxPatch
import make_figures as M

def overlap(a, b):
    dx = min(a.x1, b.x1) - max(a.x0, b.x0)
    dy = min(a.y1, b.y1) - max(a.y0, b.y0)
    return dx, dy

problems = 0
for theme in ("light", "dark"):
    t = M.THEMES[theme]
    for name, fn in M.FIGURES.items():
        M.setup(t); fig = fn(t); fig.canvas.draw(); r = fig.canvas.get_renderer()
        fb = fig.bbox
        for ax in fig.axes:
            texts = [(x, x.get_window_extent(renderer=r)) for x in ax.texts]
            boxes = [(p, p.get_window_extent(r)) for p in ax.patches
                     if isinstance(p, FancyBboxPatch)]
            for txt, tb in texts:                       # outside the canvas
                out = max(fb.x0 - tb.x0, tb.x1 - fb.x1, fb.y0 - tb.y0, tb.y1 - fb.y1)
                if out > 3:
                    print(f"[{theme}/{name}] off-canvas by {out:.0f}px: {txt.get_text()[:40]!r}")
                    problems += 1
            for txt, tb in texts:                       # wider than its own box
                cx, cy = (tb.x0+tb.x1)/2, (tb.y0+tb.y1)/2
                for _, pb in boxes:
                    if pb.x0 <= cx <= pb.x1 and pb.y0 <= cy <= pb.y1:
                        o = max(pb.x0-tb.x0, tb.x1-pb.x1, pb.y0-tb.y0, tb.y1-pb.y1)
                        if o > 2:
                            print(f"[{theme}/{name}] spills {o:.0f}px out of its box: {txt.get_text()[:40]!r}")
                            problems += 1
                        break
            for i in range(len(texts)):                 # text colliding with text
                for j in range(i+1, len(texts)):
                    if texts[i][0].get_bbox_patch() or texts[j][0].get_bbox_patch():
                        continue                        # a text with its own bbox masks what's under it
                    dx, dy = overlap(texts[i][1], texts[j][1])
                    if dx > 2 and dy > 2:
                        print(f"[{theme}/{name}] text overlap {dx:.0f}x{dy:.0f}px: "
                              f"{texts[i][0].get_text()[:26]!r} / {texts[j][0].get_text()[:26]!r}")
                        problems += 1
        plt.close(fig)
print(f"\n{problems} problems")
raise SystemExit(1 if problems else 0)
