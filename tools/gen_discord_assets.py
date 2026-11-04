#!/usr/bin/env python3
"""Art assets for Discord Rich Presence (upload them in the Discord Developer Portal under
Rich Presence -> Art Assets, with the file name as the key).

    python3 tools/gen_discord_assets.py      # writes docs/discord/*.png (needs Pillow)

The pictures are the 16x16 pixel icons of tools/gen_icons.py, scaled up by whole pixels so they
stay hard-edged, on the Graphite and violet colours of the design system.
"""
import importlib.util
import os

from PIL import Image, ImageDraw

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
spec = importlib.util.spec_from_file_location('gen_icons', os.path.join(ROOT, 'tools', 'gen_icons.py'))
gi = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gi)

SIZE = 1024
BG0 = (0x13, 0x13, 0x16)
BG1 = (0x1b, 0x1b, 0x1f)
BG2 = (0x26, 0x26, 0x2b)
VIOLET = (0x8a, 0x7b, 0xf2)
VIOLET_DARK = (0x5a, 0x4c, 0xc4)
WHITE = (0xf4, 0xf3, 0xff)


def mix(a, b, t):
    return tuple(round(x + (y - x) * t) for x, y in zip(a, b))


def draw_icon(img, name, scale, ink, shade, cx, cy, shadow=None):
    c = gi.I[name]
    n = gi.N
    ox, oy = cx - n * scale // 2, cy - n * scale // 2
    d = ImageDraw.Draw(img)
    if shadow:
        for y in range(n):
            for x in range(n):
                if c.p[y][x]:
                    d.rectangle([ox + x * scale + scale // 2, oy + y * scale + scale // 2, ox + (x + 1) * scale - 1 + scale // 2, oy + (y + 1) * scale - 1 + scale // 2], fill=shadow)
    for y in range(n):
        for x in range(n):
            v = c.p[y][x]
            if v:
                d.rectangle([ox + x * scale, oy + y * scale, ox + (x + 1) * scale - 1, oy + (y + 1) * scale - 1], fill=ink if v == 2 else shade)


def dither_bg(img, a, b, cell):
    """PSX-style ordered dither from colour a (top) to b (bottom), in big square cells."""
    bayer = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]]
    d = ImageDraw.Draw(img)
    rows = SIZE // cell
    for j in range(rows):
        t = j / (rows - 1)
        for i in range(SIZE // cell):
            col = b if t * 16 > bayer[j % 4][i % 4] + 0.5 else a
            d.rectangle([i * cell, j * cell, (i + 1) * cell - 1, (j + 1) * cell - 1], fill=col)


def logo(path):
    img = Image.new('RGB', (SIZE, SIZE), VIOLET)
    dither_bg(img, mix(VIOLET, WHITE, 0.12), VIOLET_DARK, 32)
    draw_icon(img, 'Cube', 44, WHITE, mix(WHITE, VIOLET_DARK, 0.55), SIZE // 2, SIZE // 2, shadow=mix(VIOLET_DARK, BG0, 0.45))
    img.save(path, optimize=True)


def badge(path, name):
    # Discord shows small images in a circle: keep the motif well inside it
    img = Image.new('RGB', (SIZE, SIZE), BG1)
    d = ImageDraw.Draw(img)
    d.ellipse([0, 0, SIZE - 1, SIZE - 1], fill=BG2)
    d.ellipse([48, 48, SIZE - 49, SIZE - 49], fill=BG1)
    draw_icon(img, name, 36, VIOLET, mix(VIOLET, BG1, 0.55), SIZE // 2, SIZE // 2)
    img.save(path, optimize=True)


BADGES = {
    'mode_model': 'Cube',
    'mode_edit': 'Vertex',
    'mode_texture': 'Brush',
    'mode_pose': 'Bone',
    'mode_animate': 'Film',
    'mode_render': 'Camera',
    'mode_export': 'Export',
    'mode_idle': 'Clock',
}

if __name__ == '__main__':
    out = os.path.join(ROOT, 'docs', 'discord')
    os.makedirs(out, exist_ok=True)
    logo(os.path.join(out, 'jank_logo.png'))
    for key, icon in BADGES.items():
        badge(os.path.join(out, key + '.png'), icon)
    print('wrote', 1 + len(BADGES), 'pictures to', out)
