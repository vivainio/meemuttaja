#!/usr/bin/env python3
"""Draws the Meemuttaja icon: an original wobbly rage-comic style smiling face.

Everything is generated from the code below, so there is no third-party art.
Dedicated to the public domain under CC0 1.0.

Usage: python3 scripts/make-icon.py   (needs Pillow; writes assets/icon.png)
Then rebuild assets/icon.ico and assets/icon.icns from the PNG.
"""
import math
import random
from pathlib import Path

from PIL import Image, ImageDraw

SIZE = 1024
SS = 4  # supersampling factor
W = SIZE * SS
INK = (17, 17, 17, 255)
PAPER = (255, 255, 255, 255)

rng = random.Random(7)
img = Image.new("RGBA", (W, W), (0, 0, 0, 0))
d = ImageDraw.Draw(img)


def s(v):
    return v * SS


def wobble(points, amt):
    return [(x + rng.uniform(-amt, amt), y + rng.uniform(-amt, amt)) for x, y in points]


def densify(points, step=6, closed=False):
    out = []
    pts = points + ([points[0]] if closed else [])
    for (x0, y0), (x1, y1) in zip(pts, pts[1:]):
        n = max(1, int(math.hypot(x1 - x0, y1 - y0) / step))
        for i in range(n):
            t = i / n
            out.append((x0 + (x1 - x0) * t, y0 + (y1 - y0) * t))
    out.append(pts[-1])
    return out


def stroke(points, width, closed=False, jitter=0.7):
    pts = wobble(densify(points, closed=closed), jitter)
    r = s(width) / 2
    flat = [(s(x), s(y)) for x, y in pts]
    d.line(flat, fill=INK, width=int(s(width)), joint="curve")
    for x, y in (flat[:1] + flat[-1:]) if not closed else []:
        d.ellipse([x - r, y - r, x + r, y + r], fill=INK)


def ellipse_pts(cx, cy, rx, ry, n=72, a0=0.0, a1=2 * math.pi):
    return [
        (cx + rx * math.cos(a0 + (a1 - a0) * i / n), cy + ry * math.sin(a0 + (a1 - a0) * i / n))
        for i in range(n + (0 if a1 - a0 >= 2 * math.pi else 1))
    ]


def fill(points, color):
    d.polygon([(s(x), s(y)) for x, y in points], fill=color)


# Head: slightly lopsided blob.
head = wobble(ellipse_pts(512, 520, 440, 450, n=48), 9)
head_dense = densify(head, closed=True)
fill(head_dense, PAPER)
stroke(head, 26, closed=True, jitter=1.0)

# Relaxed, slightly raised brows.
stroke([(235, 380), (320, 345), (420, 355)], 26)
stroke([(789, 380), (704, 345), (604, 355)], 26)

# Eyes: wide, with tiny pupils looking straight at you.
for cx in (350, 674):
    eye = wobble(ellipse_pts(cx, 490, 80, 80, n=36), 3)
    fill(densify(eye, closed=True), PAPER)
    stroke(eye, 20, closed=True, jitter=0.6)
    px = cx + (14 if cx < 512 else -14)
    pupil = ellipse_pts(px, 498, 17, 17, n=20)
    fill(pupil, INK)

# Nose: a short hooked line.
stroke([(498, 520), (486, 590), (520, 608)], 18)

# Big open smile with a row of teeth.
smile = ellipse_pts(512, 650, 250, 190, n=40, a0=0.12 * math.pi, a1=0.88 * math.pi)
mouth_w = wobble(smile, 3)
fill(densify(mouth_w, closed=True), PAPER)
stroke(mouth_w + [mouth_w[0]], 22, jitter=0.6)

# Cheek creases framing the smile.
stroke([(238, 640), (222, 690), (240, 735)], 14)
stroke([(786, 640), (802, 690), (784, 735)], 14)

# Sparkle on the forehead: four arcs around a centre point.
vx, vy, vr = 700, 215, 44
for k in range(4):
    cx = vx + (vr if k in (0, 3) else -vr)
    cy = vy + (vr if k in (0, 1) else -vr)
    a0 = math.pi * (1 + k / 2) % (2 * math.pi)
    arc = ellipse_pts(cx, cy, vr, vr, n=16, a0=a0, a1=a0 + math.pi / 2)
    stroke(arc, 15)

out = img.resize((SIZE, SIZE), Image.LANCZOS)
dest = Path(__file__).resolve().parent.parent / "assets" / "icon.png"
out.save(dest)
print(f"wrote {dest}")
