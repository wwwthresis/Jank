#!/usr/bin/env python3
"""Builds the two plush bunnies of the starter scene (demo/bunnies.jank) from two photos.

    python3 tools/bunnies/make_bunnies.py white_bunny.png cream_bunny.jpg out_dir

The first photo shows the white two-headed fleece bunny from above, the second the cream bunny lying on a pink rug.
For each bunny the script cuts the toy out of the photo, triangulates its outline, "inflates" the outline into a stuffed
3D shape (round edges like a sewn plush), and projects the photo onto the front as the texture; the back gets the same
fleece with the faces painted out. It also places bones and skin weights (ears, heads, arms, legs) and writes for each
bunny a JSON mesh and a PNG texture atlas, plus rug and floor textures cut from the second photo.
`core/examples/make_bunny_demo.rs` assembles them into the project. Needs numpy and OpenCV (opencv-python-headless).
"""
import json
import os
import sys

import cv2
import numpy as np

TEX = 384  # side of the front and of the back half of each texture atlas (about the resolution of the photos)


# ----------------------------------------------------------------------------------------------- image helpers

def keep_largest(mask):
    n, lab, st, _ = cv2.connectedComponentsWithStats(mask)
    if n <= 1:
        return mask
    big = 1 + int(np.argmax(st[1:, cv2.CC_STAT_AREA]))
    return np.where(lab == big, 255, 0).astype(np.uint8)


def fill_holes(mask):
    h, w = mask.shape
    ff = mask.copy()
    cv2.floodFill(ff, np.zeros((h + 2, w + 2), np.uint8), (0, 0), 255)
    return mask | (255 - ff)


def disc(r):
    return cv2.getStructuringElement(cv2.MORPH_ELLIPSE, (2 * r + 1, 2 * r + 1))


def push_pull(img, mask):
    """Fills the pixels outside `mask` with the colours of the nearest inside pixels (so texture filtering at the rim
    never picks up the background)."""
    img = img.astype(np.float32)
    w = (mask > 0).astype(np.float32)
    levels = []
    c, a = img * w[..., None], w
    while min(a.shape) > 2:
        levels.append((c, a))
        c = cv2.resize(c, (max(1, c.shape[1] // 2), max(1, c.shape[0] // 2)), interpolation=cv2.INTER_AREA)
        a = cv2.resize(a, (max(1, a.shape[1] // 2), max(1, a.shape[0] // 2)), interpolation=cv2.INTER_AREA)
    filled = c / np.maximum(a, 1e-6)[..., None]
    for c, a in reversed(levels):
        up = cv2.resize(filled, (c.shape[1], c.shape[0]), interpolation=cv2.INTER_LINEAR)
        known = c / np.maximum(a, 1e-6)[..., None]
        k = np.clip(a * 4.0, 0, 1)[..., None]
        filled = known * k + up * (1 - k)
    out = img.copy()
    out[w == 0] = filled[w == 0]
    return np.clip(out, 0, 255).astype(np.uint8)


def flatten(img, sigma):
    """Removes the light falloff of the photo (keeps the detail, evens out the brightness)."""
    f = img.astype(np.float32)
    low = cv2.GaussianBlur(f, (0, 0), sigma)
    return np.clip(f / np.maximum(low, 1) * f.reshape(-1, 3).mean(0), 0, 255).astype(np.uint8)


def seamless(tex):
    """Blends a picture with its half-shifted copy so that it tiles without visible edges."""
    h, w = tex.shape[:2]
    rolled = np.roll(np.roll(tex, h // 2, 0), w // 2, 1).astype(np.float32)
    y, x = np.mgrid[0:h, 0:w]
    wx = 1 - np.abs((x + 0.5) / w * 2 - 1)
    wy = 1 - np.abs((y + 0.5) / h * 2 - 1)
    m = np.clip(np.minimum(wx, wy) * 2.2, 0, 1)
    m = m * m * (3 - 2 * m)
    return np.clip(tex.astype(np.float32) * m[..., None] + rolled * (1 - m[..., None]), 0, 255).astype(np.uint8)


# ----------------------------------------------------------------------------------------------- the two photos

def white_bunny(path):
    """The white two-headed bunny on a beige table: a white, unsaturated toy on a saturated background."""
    img = cv2.imread(path)
    h, w = img.shape[:2]
    hsv = cv2.cvtColor(img, cv2.COLOR_BGR2HSV)
    s = cv2.GaussianBlur(hsv[:, :, 1].astype(np.float32), (0, 0), 1.5)
    v = hsv[:, :, 2].astype(np.float32)
    rough = ((s < 32) & (v > 95)).astype(np.uint8) * 255
    gc = np.full((h, w), cv2.GC_PR_BGD, np.uint8)
    gc[rough > 0] = cv2.GC_PR_FGD
    gc[cv2.erode(rough, disc(4)) > 0] = cv2.GC_FGD
    gc[cv2.dilate(rough, disc(12)) == 0] = cv2.GC_BGD
    cv2.grabCut(img, gc, None, np.zeros((1, 65)), np.zeros((1, 65)), 8, cv2.GC_INIT_WITH_MASK)
    m = np.where((gc == 1) | (gc == 3), 255, 0).astype(np.uint8)
    m = cv2.morphologyEx(m, cv2.MORPH_OPEN, disc(4))  # drops the thread hanging from a foot
    m = cv2.morphologyEx(m, cv2.MORPH_CLOSE, disc(3))
    m = fill_holes(keep_largest(m))
    # the shadow on the right side is grey, not white: trim it from the rim
    rim = (m > 0) & (cv2.erode(m, disc(5)) == 0) & (v < 118)
    m[rim] = 0
    m = fill_holes(keep_largest(cv2.morphologyEx(m, cv2.MORPH_OPEN, disc(2))))
    # colours: lift the dim flash photo to clean white fleece, keep the navy stitches
    f = img.astype(np.float32) / 255.0
    f = np.clip(f ** 0.8 * 1.24, 0, 1)
    col = (f * 255).astype(np.uint8)
    # the back: the same fleece with the stitched faces painted out
    bi = img.astype(np.int32)
    blue = (bi[:, :, 0] - bi[:, :, 2] > 6) | (hsv[:, :, 1] > 36)
    stitches = (blue & (m > 0) & (cv2.erode(m, disc(6)) > 0)).astype(np.uint8) * 255
    stitches = cv2.dilate(cv2.morphologyEx(stitches, cv2.MORPH_OPEN, disc(1)), disc(5))
    back = cv2.inpaint(col, stitches, 9, cv2.INPAINT_TELEA)
    # the painted-out spots are smooth: give them the fleece grain of their surroundings
    grain = col.astype(np.float32) - cv2.GaussianBlur(col, (0, 0), 3).astype(np.float32)
    shifted = np.roll(grain, (17, 23), (0, 1))
    soft = cv2.GaussianBlur(stitches, (0, 0), 3).astype(np.float32)[..., None] / 255.0
    back = np.clip(back.astype(np.float32) + shifted * soft, 0, 255).astype(np.uint8)
    return col, back, m, m


def cream_bunny(path):
    """The cream bunny on the pink rug in a dark room: a traced outline refined by GrabCut. The lower ear runs out of the
    picture, so the photo is extended downwards and the ear gets a rounded tip."""
    img = cv2.imread(path)
    h, w = img.shape[:2]
    outer = np.array([(212, 552), (212, 470), (232, 420), (238, 380), (236, 300), (243, 262), (290, 252), (328, 228), (336, 183),
                      (382, 176), (402, 196), (420, 166), (503, 128), (518, 92), (562, 86), (602, 102), (612, 138), (632, 146),
                      (658, 172), (664, 232), (658, 292), (622, 312), (592, 322), (588, 362), (578, 412), (530, 418), (440, 412),
                      (420, 402), (380, 412), (334, 422), (324, 470), (314, 552)], np.int32)
    gc = np.full((h, w), cv2.GC_BGD, np.uint8)
    cv2.fillPoly(gc, [outer], cv2.GC_PR_BGD)
    inside = np.zeros((h, w), np.uint8)
    cv2.fillPoly(inside, [outer], 1)
    lum = cv2.GaussianBlur(cv2.cvtColor(img, cv2.COLOR_BGR2LAB)[:, :, 0], (0, 0), 2)
    gc[(inside > 0) & (lum > 72)] = cv2.GC_PR_FGD
    seed = np.zeros((h, w), np.uint8)
    cv2.circle(seed, (335, 330), 48, 1, -1)
    cv2.circle(seed, (490, 270), 55, 1, -1)
    cv2.circle(seed, (365, 225), 11, 1, -1)
    for p, q, r in [((540, 130), (570, 190), 14), ((612, 190), (622, 262), 14), ((445, 377), (548, 380), 11),
                    ((300, 425), (258, 528), 13), ((272, 322), (268, 300), 6), ((450, 230), (560, 300), 30)]:
        cv2.line(seed, p, q, 1, r * 2)
    gc[seed > 0] = cv2.GC_FGD
    cv2.grabCut(img, gc, None, np.zeros((1, 65)), np.zeros((1, 65)), 10, cv2.GC_INIT_WITH_MASK)
    m = np.where((gc == 1) | (gc == 3), 255, 0).astype(np.uint8)
    m = cv2.morphologyEx(m, cv2.MORPH_OPEN, disc(2))
    m = cv2.morphologyEx(m, cv2.MORPH_CLOSE, disc(4))
    m = fill_holes(keep_largest(m))
    # extend the picture by 60 rows (mirrored) and close the ear with a rounded tip
    ext = 60
    img2 = cv2.copyMakeBorder(img, 0, ext, 0, 0, cv2.BORDER_REFLECT)
    m2 = cv2.copyMakeBorder(m, 0, ext, 0, 0, cv2.BORDER_CONSTANT, value=0)
    cols = np.where(m[h - 3] > 0)[0]
    if len(cols):
        x0, x1 = int(cols.min()), int(cols.max())
        cx, rx = (x0 + x1) // 2, max(8, (x1 - x0) // 2)
        cv2.rectangle(m2, (x0, h - 3), (x1, h + 4), 255, -1)
        cv2.ellipse(m2, (cx - 3, h + 2), (int(rx * 0.92), int(rx * 0.9)), 8, 0, 360, 255, -1)
    m2 = cv2.morphologyEx(m2, cv2.MORPH_CLOSE, disc(5))
    # colours: denoise the dark JPEG, lift the exposure and balance the warm lamp light towards cream fleece
    den = cv2.fastNlMeansDenoisingColored(img2, None, 5, 5, 7, 21)
    f = den.astype(np.float32) / 255.0
    f = f ** 0.62
    mean = f[m2 > 0].mean(axis=0)  # BGR
    target = np.array([0.74, 0.79, 0.83])  # cream (BGR)
    f = np.clip(f * (target / mean), 0, 1)
    col = (f * 255).astype(np.uint8)
    # gentle local contrast so the fleece keeps its texture
    blur = cv2.GaussianBlur(col, (0, 0), 6)
    col = cv2.addWeighted(col, 1.35, blur, -0.35, 0)
    # the back lies on the rug: fleece without the eye and nose
    lab = cv2.cvtColor(col, cv2.COLOR_BGR2LAB)
    dark = ((lab[:, :, 0] < 80) & (m2 > 0) & (cv2.erode(m2, disc(8)) > 0)).astype(np.uint8) * 255
    back = cv2.inpaint(col, cv2.dilate(dark, disc(4)), 7, cv2.INPAINT_TELEA)
    # rug and floor textures from the same photo, corrected the same way
    full = (np.clip((cv2.fastNlMeansDenoisingColored(img, None, 5, 5, 7, 21).astype(np.float32) / 255.0) ** 0.62 * (target / mean), 0, 1) * 255).astype(np.uint8)
    rug = seamless(flatten(cv2.resize(full[200:440, 10:250], (256, 256), interpolation=cv2.INTER_AREA), 20))
    tile = flatten(full[15:95, 300:430], 12)
    valid = m2.copy()
    valid[h - 2:] = 0  # the rounded ear tip below the photo takes its colour from the ear above
    return col, back, m2, valid, rug, tile


# ----------------------------------------------------------------------------------------------- geometry

def resample(contour, step, offset=0.0):
    p = contour.reshape(-1, 2).astype(np.float64)
    seg = np.linalg.norm(np.roll(p, -1, 0) - p, axis=1)
    cum = np.concatenate([[0], np.cumsum(seg)])
    total = cum[-1]
    n = max(8, int(round(total / step)))
    out = []
    for i in range(n):
        s = (i + offset) / n * total
        k = int(np.searchsorted(cum, s, side='right') - 1)
        k = min(k, len(p) - 1)
        t = (s - cum[k]) / max(seg[k], 1e-9)
        out.append(p[k] + (p[(k + 1) % len(p)] - p[k]) * t)
    return np.array(out)


def outline(mask):
    cs, _ = cv2.findContours(mask, cv2.RETR_EXTERNAL, cv2.CHAIN_APPROX_NONE)
    return max(cs, key=cv2.contourArea)


def height_field(mask, cap, k, dome):
    """Stuffed-toy profile: a quarter circle over the rim (radius = local half-width, at most `cap` px), then a soft dome."""
    dt = cv2.distanceTransform(mask, cv2.DIST_L2, 5)
    r = cv2.dilate(dt, disc(int(cap)))
    r = np.clip(r, 1.0, cap)
    t = np.clip(dt / r, 0, 1)
    z = k * r * np.sqrt(1 - (1 - t) ** 2) + dome * np.sqrt(np.maximum(dt - r, 0))
    m = (mask > 0).astype(np.float32)
    zb = cv2.GaussianBlur(z * m, (0, 0), 2.5) / np.maximum(cv2.GaussianBlur(m, (0, 0), 2.5), 1e-4)
    return zb * m, dt


def height_parts(mask, bones, parts, overlap):
    """A stuffed toy sewn from separate pieces (head, body, paws...): every piece is inflated on its own and the toy is
    their union, so a crease forms where the head meets the body. `parts` maps bone names to (k, cap)."""
    h, w = mask.shape
    ys, xs = np.nonzero(mask)
    joints, weights = skin(np.stack([xs, ys], 1).astype(np.float64), bones)
    owner = np.full((h, w), -1, np.int32)
    owner[ys, xs] = joints[:, 0]
    z = np.zeros((h, w), np.float32)
    for i, (name, *_rest) in enumerate(bones):
        if name not in parts:
            continue
        k, cap = parts[name]
        region = ((owner == i).astype(np.uint8) * 255)
        if not region.any():
            continue
        region = cv2.dilate(region, disc(overlap)) & mask
        dt = cv2.distanceTransform(region, cv2.DIST_L2, 5)
        r = np.clip(cv2.dilate(dt, disc(int(cap))), 1.0, cap)
        t = np.clip(dt / r, 0, 1)
        z = np.maximum(z, k * r * np.sqrt(1 - (1 - t) ** 2))
    m = (mask > 0).astype(np.float32)
    zb = cv2.GaussianBlur(z * m, (0, 0), 3.0) / np.maximum(cv2.GaussianBlur(m, (0, 0), 3.0), 1e-4)
    return zb * m


def bilinear(f, x, y):
    h, w = f.shape
    x = np.clip(x, 0, w - 1.001)
    y = np.clip(y, 0, h - 1.001)
    x0, y0 = np.floor(x).astype(int), np.floor(y).astype(int)
    fx, fy = x - x0, y - y0
    return (f[y0, x0] * (1 - fx) * (1 - fy) + f[y0, x0 + 1] * fx * (1 - fy) + f[y0 + 1, x0] * (1 - fx) * fy + f[y0 + 1, x0 + 1] * fx * fy)


def triangulate(mask, step_rim, step_in, ring):
    h, w = mask.shape
    dt = cv2.distanceTransform(mask, cv2.DIST_L2, 5)
    pts = [resample(outline(mask), step_rim)]
    inner = (dt > ring).astype(np.uint8) * 255
    pts.append(resample(outline(keep_largest(inner)), step_rim * 1.15, 0.5))
    # interior: a hexagonal grid
    ys = np.arange(step_in * 0.5, h, step_in * 0.866)
    grid = []
    for j, y in enumerate(ys):
        xs = np.arange(step_in * (0.5 if j % 2 else 0.0), w, step_in)
        for x in xs:
            if bilinear(dt, np.array([x]), np.array([y]))[0] > ring + step_in * 0.55:
                grid.append((x, y))
    if grid:
        pts.append(np.array(grid))
    p = np.concatenate(pts)
    # Delaunay
    sub = cv2.Subdiv2D((-2, -2, w + 4, h + 4))
    index = {}
    for i, (x, y) in enumerate(p):
        key = (round(float(x), 3), round(float(y), 3))
        if key in index:
            continue
        index[key] = i
        sub.insert((float(x), float(y)))
    tris = []
    near = cv2.dilate(mask, disc(1))
    for t in sub.getTriangleList():
        ids = []
        for k in range(3):
            key = (round(float(t[2 * k]), 3), round(float(t[2 * k + 1]), 3))
            ids.append(index.get(key))
        if None in ids:
            continue
        a, b, c = (p[i] for i in ids)
        if abs((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])) < 1.0:
            continue
        probes = [(a + b + c) / 3, (a + b) / 2, (b + c) / 2, (c + a) / 2]
        if all(near[int(min(h - 1, max(0, q[1]))), int(min(w - 1, max(0, q[0])))] > 0 for q in probes):
            tris.append(ids)
    tris = np.array(tris)
    used = np.unique(tris)
    remap = -np.ones(len(p), int)
    remap[used] = np.arange(len(used))
    return p[used], remap[tris]


def split_rim_chords(pts, tris):
    """An inner edge whose both ends lie on the outline would pinch the front and back together (both ends have zero
    thickness); split such edges at their midpoint until none is left."""
    from collections import defaultdict
    pts, tris = list(map(tuple, pts)), [list(t) for t in tris]
    while True:
        count = defaultdict(list)
        for k, t in enumerate(tris):
            for e in ((t[0], t[1]), (t[1], t[2]), (t[2], t[0])):
                count[tuple(sorted(e))].append(k)
        rim = set()
        for e, ks in count.items():
            if len(ks) == 1:
                rim.update(e)
        chords = [e for e, ks in count.items() if len(ks) == 2 and e[0] in rim and e[1] in rim]
        if not chords:
            return np.array(pts), np.array(tris)
        done = set()
        for a, b in chords:
            ks = count[(a, b)]
            if any(k in done for k in ks):
                continue
            m = len(pts)
            pts.append(((pts[a][0] + pts[b][0]) / 2, (pts[a][1] + pts[b][1]) / 2))
            for k in ks:
                t = tris[k]
                c = [v for v in t if v != a and v != b][0]
                # keep the winding: replace the edge a-b of t by two triangles through m
                i = t.index(a)
                if t[(i + 1) % 3] == b:
                    tris[k] = [a, m, c]
                    tris.append([m, b, c])
                else:
                    tris[k] = [b, m, c]
                    tris.append([m, a, c])
                done.add(k)


def rim_vertices(tris, n):
    from collections import Counter
    c = Counter()
    for a, b, d in tris:
        for e in ((a, b), (b, d), (d, a)):
            c[tuple(sorted(e))] += 1
    rim = np.zeros(n, bool)
    for (a, b), k in c.items():
        if k == 1:
            rim[a] = rim[b] = True
    return rim


# ----------------------------------------------------------------------------------------------- skin weights

def smoothstep(e0, e1, x):
    t = np.clip((x - e0) / (e1 - e0), 0, 1)
    return t * t * (3 - 2 * t)


def skin(pts, bones):
    """bones: list of (name, parent, pivot, tip, radius), parents first. Each bone takes over the vertices inside its
    capsule beyond its pivot, softly across the joint; whatever is left stays with the bones above."""
    n = len(pts)
    w = np.zeros((n, len(bones)))
    w[:, 0] = 1.0
    for i, (name, parent, pivot, tip, rad) in enumerate(bones):
        if parent is None:
            continue
        pv, tp = np.array(pivot, float), np.array(tip, float)
        axis = tp - pv
        length = np.linalg.norm(axis)
        axis /= length
        rel = pts - pv
        s = rel @ axis
        perp = np.abs(rel @ np.array([-axis[1], axis[0]]))
        blend = min(14.0, rad * 0.6)
        m = smoothstep(-blend, blend, s) * (1 - smoothstep(rad, rad + 10, perp)) * (1 - smoothstep(length + rad, length + rad + 12, s))
        # take the vertex from the parent or from any bone above it (an ear also claims the tip that lies outside the
        # head's own capsule; it still follows the head, being its child)
        a = parent
        while a is not None:
            take = w[:, a] * m
            w[:, a] -= take
            w[:, i] += take
            a = bones[a][1]
    order = np.argsort(-w, axis=1)[:, :4]
    jw = np.take_along_axis(w, order, 1)
    jw /= np.maximum(jw.sum(1, keepdims=True), 1e-9)
    jw[jw < 1e-4] = 0
    jw /= jw.sum(1, keepdims=True)
    return order, jw


# ----------------------------------------------------------------------------------------------- one bunny

def build(name, col, back, mask, valid, out, opts, bones, lying):
    h, w = mask.shape
    ys, xs = np.where(mask > 0)
    x0, x1, y0, y1 = xs.min(), xs.max(), ys.min(), ys.max()
    side = int(max(x1 - x0, y1 - y0) * 1.04) + 4
    cx, cy = (x0 + x1) / 2, (y0 + y1) / 2
    ox, oy = int(cx - side / 2), int(cy - side / 2)
    # texture atlas: front | back, each the square crop around the toy
    def crop(img):
        pad = cv2.copyMakeBorder(img, side, side, side, side, cv2.BORDER_REPLICATE)
        c = pad[oy + side: oy + 2 * side, ox + side: ox + 2 * side]
        return cv2.resize(c, (TEX, TEX), interpolation=cv2.INTER_AREA)
    # the outermost pixels of the toy are often dark (shadow, background showing through the fleece): take the rim
    # colour from a little further inside
    src = cv2.erode(valid, disc(2))
    front = push_pull(crop(col), crop(src))
    backt = push_pull(crop(back), crop(src))
    atlas = np.concatenate([front, backt], axis=1)
    cv2.imwrite(os.path.join(out, name + '.png'), atlas)

    pts, tris = triangulate(mask, opts['rim'], opts['step'], opts['ring'])
    pts, tris = split_rim_chords(pts, tris)
    if 'parts' in opts:
        zf = height_parts(mask, bones, opts['parts'], opts['overlap'])
    else:
        zf, _ = height_field(mask, opts['cap'], opts['k'], opts['dome'])
    z = bilinear(zf, pts[:, 0], pts[:, 1])
    rim = rim_vertices(tris, len(pts))
    z[rim] = 0.0
    zb = z * opts['back']
    s = opts['scale'] / max(x1 - x0, y1 - y0)
    n = len(pts)
    # positions: photo plane -> model space
    def to_model(x, y, d):
        X = (x - cx) * s
        if lying:  # photo plane = ground, the thickness points up, the top of the photo is far away
            return np.stack([X, d * s, (y - cy) * s], -1)
        return np.stack([X, -(y - cy) * s, d * s], -1)
    pf = to_model(pts[:, 0], pts[:, 1], z)
    pb = to_model(pts[:, 0], pts[:, 1], -zb)
    up = np.array([0, 1, 0]) if lying else np.array([0, 0, 1])
    # one closed surface for the normals: rim vertices are shared by both sides
    bi = np.where(rim, np.arange(n), np.arange(n) + n)
    allp = np.concatenate([pf, pb])
    faces = []
    for t in tris:
        for side_, ids, want in ((0, t, up), (1, bi[t], -up)):
            a, b, c = allp[ids]
            nrm = np.cross(b - a, c - a)
            faces.append(list(ids) if nrm @ want > 0 else [ids[0], ids[2], ids[1]])
    faces = np.array(faces)
    nacc = np.zeros_like(allp)
    for f in faces:
        a, b, c = allp[f]
        nacc[f] += np.cross(b - a, c - a)
    nrm = nacc / np.maximum(np.linalg.norm(nacc, axis=1, keepdims=True), 1e-9)
    # vertices: front copies use the left half of the atlas, back copies the right half (rim vertices get both)
    u = (pts[:, 0] - ox) / side * 0.5
    v = (pts[:, 1] - oy) / side
    joints, weights = skin(pts, bones)
    verts = []
    for i in range(n):
        verts.append([*pf[i], *nrm[i], u[i], v[i], *joints[i], *weights[i]])
    for i in range(n):
        verts.append([*pb[i], *nrm[bi[i]], u[i] + 0.5, v[i], *joints[i], *weights[i]])
    out_tris = []
    for k, f in enumerate(faces):
        if k % 2 == 1:  # back face: use the back copies, also for rim vertices
            f = [i if i >= n else i + n for i in f]
        out_tris.append([int(i) for i in f])
    from collections import Counter
    edges = Counter()
    keyp = [tuple(np.round(vv[:3], 6)) for vv in verts]
    for f in out_tris:
        for e in ((f[0], f[1]), (f[1], f[2]), (f[2], f[0])):
            edges[tuple(sorted((keyp[e[0]], keyp[e[1]])))] += 1
    bad = sum(1 for c in edges.values() if c != 2)
    if bad:
        print(f'{name}: warning: {bad} edges are not shared by exactly two triangles')
    bone_out = []
    for (bname, parent, pivot, tip, rad) in bones:
        p = to_model(np.array([pivot[0]]), np.array([pivot[1]]), np.array([0.0]))[0]
        q = to_model(np.array([tip[0]]), np.array([tip[1]]), np.array([0.0]))[0]
        bone_out.append({'name': bname, 'parent': -1 if parent is None else parent, 'pos': [float(c) for c in p], 'tip': [float(c) for c in q]})
    lo = np.concatenate([pf, pb]).min(0)
    hi = np.concatenate([pf, pb]).max(0)
    doc = {'name': name, 'texture': name + '.png', 'verts': [[round(float(c), 5) for c in vv[:8]] + [int(c) for c in vv[8:12]] + [round(float(c), 4) for c in vv[12:]] for vv in verts],
           'tris': out_tris, 'bones': bone_out, 'bounds': [lo.tolist(), hi.tolist()]}
    with open(os.path.join(out, name + '.json'), 'w') as fh:
        json.dump(doc, fh, separators=(',', ':'))
    print(f'{name}: {len(verts)} vertices, {len(out_tris)} triangles, {len(bones)} bones, size {hi - lo}')
    # a debug picture: outline, triangles, bones
    dbg = col.copy()
    for t in tris:
        cv2.polylines(dbg, [pts[t].astype(np.int32)], True, (0, 200, 0), 1)
    for i, (bname, parent, pivot, tip, rad) in enumerate(bones):
        cv2.circle(dbg, tuple(int(c) for c in pivot), 4, (0, 0, 255), -1)
        if parent is not None:
            cv2.line(dbg, tuple(int(c) for c in pivot), tuple(int(c) for c in tip), (255, 0, 0), 2)
    cv2.imwrite(os.path.join(out, name + '_debug.png'), dbg)


WHITE_BONES = [
    ('Body', None, (272, 262), (272, 262), 0),
    ('Head_L', 0, (243, 204), (186, 118), 62),
    ('Head_R', 0, (312, 206), (372, 146), 60),
    ('Ear_L_Out', 1, (170, 112), (160, 36), 24),
    ('Ear_L_In', 1, (222, 104), (243, 28), 24),
    ('Ear_R_In', 2, (370, 130), (442, 58), 24),
    ('Ear_R_Out', 2, (398, 163), (490, 163), 24),
    ('Arm_L', 0, (216, 258), (126, 268), 27),
    ('Arm_R', 0, (324, 247), (400, 238), 27),
    ('Leg_L', 0, (246, 314), (240, 402), 26),
    ('Leg_R', 0, (298, 316), (314, 406), 26),
]

CREAM_BONES = [
    ('Body', None, (500, 270), (500, 270), 0),
    ('Head', 0, (402, 300), (296, 346), 92),
    ('Ear_Top', 1, (284, 286), (256, 376), 24),
    ('Ear_Long', 1, (308, 398), (262, 600), 38),
    ('Paw_Up', 0, (392, 262), (352, 200), 28),
    ('Paw_Down', 0, (442, 372), (566, 388), 27),
    ('Foot_1', 0, (512, 212), (566, 108), 40),
    ('Foot_2', 0, (586, 248), (634, 172), 38),
]


def floor_tile(patch):
    """One floor tile (the texture repeats once per tile): the grain of the photographed tiles, a dark grout line and a
    slightly worn edge."""
    n = 128
    base = seamless(cv2.resize(patch, (n, n), interpolation=cv2.INTER_AREA)).astype(np.float32)
    grain = base - cv2.GaussianBlur(base, (0, 0), 8)
    mean = patch.reshape(-1, 3).astype(np.float32).mean(0)
    tone = mean * np.array([0.62, 0.62, 0.62])  # darker, as the tiles look in the room
    y, x = np.mgrid[0:n, 0:n].astype(np.float32)
    edge = np.minimum(np.minimum(x + 0.5, n - 0.5 - x), np.minimum(y + 0.5, n - 0.5 - y))
    shade = 0.82 + 0.18 * smoothstep(1.5, 7.0, edge)
    rng = np.random.default_rng(7)
    blotch = cv2.GaussianBlur(rng.normal(0, 1, (n, n)).astype(np.float32), (0, 0), 10)
    blotch = blotch / (np.abs(blotch).max() + 1e-6)
    img = (tone[None, None, :] * (1 + 0.06 * blotch[..., None]) + grain * 1.3) * shade[..., None]
    grout = edge < 1.6
    img[grout] = tone * 0.42
    return np.clip(img, 0, 255).astype(np.uint8)


def pink(rug):
    """The rug is dusty pink: keep the pile from the photo, push its colour towards pink."""
    f = rug.astype(np.float32)
    lum = f.mean(axis=2, keepdims=True)
    target = np.array([196, 170, 222], np.float32)  # BGR, dusty pink
    out = (lum / lum.mean()) * target * 0.82 + (f - lum) * 0.5
    return np.clip(out, 0, 255).astype(np.uint8)


def main():
    if len(sys.argv) < 4:
        print(__doc__)
        sys.exit(1)
    a_path, b_path, out = sys.argv[1:4]
    os.makedirs(out, exist_ok=True)
    col, back, mask, valid = white_bunny(a_path)
    build('white_bunny', col, back, mask, valid, out,
          dict(rim=11, step=17, ring=4.5, cap=26, k=0.85, dome=0.9, back=0.75, scale=0.95), WHITE_BONES, lying=True)
    col, back, mask, valid, rug, tile = cream_bunny(b_path)
    build('cream_bunny', col, back, mask, valid, out,
          dict(rim=12, step=19, ring=5.0, back=0.30, scale=1.35, overlap=16,
               parts={'Body': (0.78, 90), 'Head': (0.92, 80), 'Ear_Top': (0.45, 26), 'Ear_Long': (0.5, 34),
                      'Paw_Up': (0.85, 30), 'Paw_Down': (0.8, 30), 'Foot_1': (0.9, 44), 'Foot_2': (0.9, 42)}),
          CREAM_BONES, lying=True)
    cv2.imwrite(os.path.join(out, 'rug.png'), pink(rug))
    cv2.imwrite(os.path.join(out, 'tiles.png'), floor_tile(tile))


if __name__ == '__main__':
    main()
