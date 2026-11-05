//! Picture-wide effects that run after the triangles are drawn: ambient occlusion, lamp halos, bloom and the CRT screen.
//!
//! They are the "modern touches" of a render style (`PsxStyle::ao`, `bloom`, `halo`, `crt_*`, `chroma`). All of them are cheap on
//! purpose, so the retro look stays fast on weak machines:
//! * ambient occlusion reads only the depth picture (a handful of lookups per pixel, no extra geometry);
//! * bloom is worked out at half or quarter resolution and blurred with running box sums;
//! * a halo is one small additive sprite per lamp;
//! * the CRT pass is one lookup per pixel.
//!
//! Every pass works on whole rows, split between threads; the picture is identical for any number of threads.
use crate::lights::LiveLight;
use crate::math::*;
use crate::raster::{Camera, Frame, PsxStyle, NO_TRI};

/// Runs `f(rows, first_row)` over chunks of whole rows on up to `threads` threads.
pub(crate) fn par_rows<T: Send, F>(color: &mut [T], w: usize, threads: usize, f: F)
where
    F: Fn(&mut [T], usize) + Sync,
{
    let h = color.len() / w.max(1);
    if h == 0 {
        return;
    }
    let threads = threads.clamp(1, 16);
    if threads == 1 || w * h < 30_000 {
        f(color, 0);
        return;
    }
    let rows = h.div_ceil(threads * 2).max(4);
    let chunks: Vec<(usize, &mut [T])> = color.chunks_mut(rows * w).enumerate().map(|(i, c)| (i * rows, c)).collect();
    let queue = std::sync::Mutex::new(chunks);
    std::thread::scope(|sc| {
        for _ in 0..threads {
            sc.spawn(|| loop {
                let job = queue.lock().expect("row queue").pop();
                match job {
                    Some((y0, c)) => f(c, y0),
                    None => break,
                }
            });
        }
    });
}

#[inline]
fn lin_depth(z: f32, near: f32, far: f32) -> f32 {
    2.0 * far * near / ((far + near) - z * (far - near))
}

#[inline]
fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Soft contact shadows in creases and corners, from the depth picture alone.
///
/// The inverse of the view depth changes linearly across any flat surface on screen. So for pairs of opposite neighbours the
/// centre pixel lies exactly on the chord between them on a flat surface, in front of it on a ridge and behind it in a crease:
/// only the last darkens. Big jumps (an object far in front of the floor) are ignored, which avoids dark outlines.
/// Worked out at half resolution with taps turned and scaled per 2x2 cell, then smoothed with a small [1 2 1] filter (which also
/// removes the 2x2 pattern of the taps).
pub(crate) fn ambient_occlusion(f: &mut Frame, cam: &Camera, s: &PsxStyle, qlut: &[u8; 256], threads: usize) {
    let (w, h) = (f.w as usize, f.h as usize);
    let (hw, hh) = (w.div_ceil(2), h.div_ceil(2));
    let (near, far) = (cam.near, cam.far);
    // inverse view depth (0 = background) of the top-left pixel of every 2x2 block
    let mut iz = vec![0f32; hw * hh];
    for (i, v) in iz.iter_mut().enumerate() {
        let (x, y) = ((i % hw) * 2, (i / hw) * 2);
        let j = y * w + x;
        let z = f.depth[j];
        if f.tri_at[j] != NO_TRI && z.is_finite() {
            let d = lin_depth(z, near, far);
            if d > 1e-4 {
                *v = 1.0 / d;
            }
        }
    }
    let focal = hh as f32 / (2.0 * (cam.fov_y * 0.5).tan());
    let rad = s.ao_radius.clamp(0.02, 20.0);
    let fog = s.fog_on.then_some((s.fog_near, s.fog_far));
    let max_r = (hh as f32 * 0.1).max(2.0);
    // four tap directions; each 2x2 cell turns them by another eighth of a turn
    let rot: [[(f32, f32); 4]; 4] = std::array::from_fn(|cell| {
        let a = cell as f32 * std::f32::consts::FRAC_PI_8;
        std::array::from_fn(|k| {
            let b = a + k as f32 * std::f32::consts::FRAC_PI_4;
            (b.cos(), b.sin())
        })
    });
    let mut occ = vec![0f32; hw * hh];
    let izr = &iz;
    par_rows(&mut occ, hw, threads, |rows, y0| {
        let nr = rows.len() / hw;
        for r in 0..nr {
            let y = y0 + r;
            for x in 0..hw {
                let c = izr[y * hw + x];
                if c <= 0.0 {
                    continue;
                }
                let d = 1.0 / c;
                let rp = (rad * focal / d).clamp(1.0, max_r);
                let at = |ox: f32, oy: f32| -> Option<f32> {
                    let (sx, sy) = ((x as f32 + ox + 0.5).floor() as i64, (y as f32 + oy + 0.5).floor() as i64);
                    if sx < 0 || sy < 0 || sx >= hw as i64 || sy >= hh as i64 {
                        return None;
                    }
                    Some(izr[sy as usize * hw + sx as usize])
                };
                let mut o = 0.0;
                let cell = (x & 1) * 2 + (y & 1);
                let jit = 0.8 + 0.13 * cell as f32;
                for &(dx, dy) in &rot[cell] {
                    for sc in [0.3f32, 0.62, 1.0] {
                        let (ox, oy) = (dx * rp * sc * jit, dy * rp * sc * jit);
                        let (Some(a1), Some(b1)) = (at(ox, oy), at(-ox, -oy)) else { continue };
                        if a1 <= 0.0 || b1 <= 0.0 {
                            continue;
                        }
                        let avg = 0.5 * (a1 + b1);
                        if avg <= c * 1.002 {
                            continue;
                        }
                        let crease = d - 1.0 / avg;
                        o += smooth(crease / (rad * 0.15)) * (1.0 - smooth((crease - rad * 0.4) / (rad * 0.6)));
                    }
                }
                let mut o = (o / 12.0 * 1.6).min(1.0);
                if let Some((n0, n1)) = fog {
                    o *= 1.0 - ((d - n0) / (n1 - n0).max(1e-3)).clamp(0.0, 1.0);
                }
                rows[r * hw + x] = o;
            }
        }
    });
    // [1 2 1] across and down, twice
    let mut tmp = vec![0f32; hw * hh];
    for _ in 0..2 {
        for y in 0..hh {
            let row = &occ[y * hw..(y + 1) * hw];
            for x in 0..hw {
                tmp[y * hw + x] = (row[x.saturating_sub(1)] + 2.0 * row[x] + row[(x + 1).min(hw - 1)]) * 0.25;
            }
        }
        for y in 0..hh {
            let (up, dn) = (y.saturating_sub(1) * hw, (y + 1).min(hh - 1) * hw);
            for x in 0..hw {
                occ[y * hw + x] = (tmp[up + x] + 2.0 * tmp[y * hw + x] + tmp[dn + x]) * 0.25;
            }
        }
    }
    let strength = s.ao.clamp(0.0, 1.0) * 0.85;
    let (occ, tri_at) = (&occ, &f.tri_at);
    par_rows(&mut f.color, w, threads, |rows, y0| {
        let nr = rows.len() / w;
        for r in 0..nr {
            let y = y0 + r;
            let fy = y as f32 * 0.5;
            let ya = (fy.max(0.0) as usize).min(hh - 1);
            let yb = (ya + 1).min(hh - 1);
            let t = (fy - ya as f32).clamp(0.0, 1.0);
            for x in 0..w {
                if tri_at[y * w + x] == NO_TRI {
                    continue;
                }
                let fx = x as f32 * 0.5;
                let xa = (fx.max(0.0) as usize).min(hw - 1);
                let xb = (xa + 1).min(hw - 1);
                let u = (fx - xa as f32).clamp(0.0, 1.0);
                let o = (occ[ya * hw + xa] * (1.0 - u) + occ[ya * hw + xb] * u) * (1.0 - t) + (occ[yb * hw + xa] * (1.0 - u) + occ[yb * hw + xb] * u) * t;
                if o <= 0.01 {
                    continue;
                }
                let keep = 1.0 - strength * o;
                let p = &mut rows[r * w + x];
                for ch in 0..3 {
                    p[ch] = qlut[((p[ch] as f32 * keep) + 0.5).clamp(0.0, 255.0) as usize];
                }
            }
        }
    });
}

/// A soft glow around every lamp that is not hidden behind something.
pub(crate) fn halos(f: &mut Frame, cam: &Camera, vp: &Mat4, s: &PsxStyle, lights: &[LiveLight]) {
    let (w, h) = (f.w as i32, f.h as i32);
    let focal = h as f32 / (2.0 * (cam.fov_y * 0.5).tan());
    for l in lights {
        let k = (l.halo * s.halo).min(3.0);
        if k <= 0.0 {
            continue;
        }
        let c = vp.mul_vec4([l.pos.x, l.pos.y, l.pos.z, 1.0]);
        if c[3] <= cam.near {
            continue;
        }
        let (sx, sy, depth) = ((c[0] / c[3] * 0.5 + 0.5) * w as f32, (0.5 - c[1] / c[3] * 0.5) * h as f32, c[3]);
        let world_r = 0.12 + 0.45 * k.min(2.0);
        let r = (world_r * focal / depth).clamp(1.5, h as f32 * 0.3);
        if sx + r < 0.0 || sy + r < 0.0 || sx - r > w as f32 || sy - r > h as f32 {
            continue;
        }
        // hidden? something clearly nearer than the glow itself covers the lamp (a lantern's own walls do not count)
        let step = (r * 0.12).max(1.0);
        let mut vis = 0.0;
        for (ox, oy) in [(0.0, 0.0), (-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
            let (px, py) = ((sx + ox * step) as i32, (sy + oy * step) as i32);
            if px < 0 || py < 0 || px >= w || py >= h {
                vis += 1.0;
                continue;
            }
            let i = (py * w + px) as usize;
            if f.tri_at[i] == NO_TRI || !f.depth[i].is_finite() || lin_depth(f.depth[i], cam.near, cam.far) > depth - world_r * 1.5 {
                vis += 1.0;
            }
        }
        vis /= 5.0;
        if vis <= 0.0 {
            continue;
        }
        let fade = if s.fog_on { 1.0 - ((depth - s.fog_near) / (s.fog_far - s.fog_near).max(1e-3)).clamp(0.0, 1.0) * 0.85 } else { 1.0 };
        let m = l.rgb[0].max(l.rgb[1]).max(l.rgb[2]).max(1e-4);
        let col = [l.rgb[0] / m, l.rgb[1] / m, l.rgb[2] / m];
        let power = vis * fade * (0.35 + 0.65 * (m / 1.5).min(1.0)) * k.min(1.5);
        let (x0, x1) = (((sx - r).floor() as i32).max(0), ((sx + r).ceil() as i32).min(w - 1));
        let (y0, y1) = (((sy - r).floor() as i32).max(0), ((sy + r).ceil() as i32).min(h - 1));
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (dx, dy) = (x as f32 + 0.5 - sx, y as f32 + 0.5 - sy);
                let dd = (dx * dx + dy * dy).sqrt() / r;
                if dd >= 1.0 {
                    continue;
                }
                let g = 1.0 - dd;
                let glow = g * g * 0.5 * power;
                let core = ((1.0 - dd * 3.5).max(0.0)).powi(2) * power;
                let p = &mut f.color[(y * w + x) as usize];
                for ch in 0..3 {
                    let add = (col[ch] * glow + (0.55 + 0.45 * col[ch]) * core) * 255.0;
                    p[ch] = (p[ch] as f32 + add).min(255.0) as u8;
                }
            }
        }
    }
}

fn box_blur_h(src: &[[f32; 3]], dst: &mut [[f32; 3]], w: usize, h: usize, r: usize) {
    let norm = 1.0 / (2 * r + 1) as f32;
    for y in 0..h {
        let row = &src[y * w..(y + 1) * w];
        let get = |x: i64| row[x.clamp(0, w as i64 - 1) as usize];
        let mut acc = [0.0f32; 3];
        for x in -(r as i64)..=(r as i64) {
            let v = get(x);
            (0..3).for_each(|c| acc[c] += v[c]);
        }
        for x in 0..w {
            dst[y * w + x] = [acc[0] * norm, acc[1] * norm, acc[2] * norm];
            let (add, sub) = (get(x as i64 + r as i64 + 1), get(x as i64 - r as i64));
            (0..3).for_each(|c| acc[c] += add[c] - sub[c]);
        }
    }
}
fn box_blur_v(src: &[[f32; 3]], dst: &mut [[f32; 3]], w: usize, h: usize, r: usize) {
    let norm = 1.0 / (2 * r + 1) as f32;
    for x in 0..w {
        let get = |y: i64| src[y.clamp(0, h as i64 - 1) as usize * w + x];
        let mut acc = [0.0f32; 3];
        for y in -(r as i64)..=(r as i64) {
            let v = get(y);
            (0..3).for_each(|c| acc[c] += v[c]);
        }
        for y in 0..h {
            dst[y * w + x] = [acc[0] * norm, acc[1] * norm, acc[2] * norm];
            let (add, sub) = (get(y as i64 + r as i64 + 1), get(y as i64 - r as i64));
            (0..3).for_each(|c| acc[c] += add[c] - sub[c]);
        }
    }
}

/// Glow around the bright parts of the picture: a bright pass at low resolution, blurred, added back.
pub(crate) fn bloom(f: &mut Frame, s: &PsxStyle, threads: usize) {
    let (w, h) = (f.w as usize, f.h as usize);
    let fct = if w * h > 350_000 { 4 } else { 2 };
    let (lw, lh) = (w.div_ceil(fct), h.div_ceil(fct));
    let thr = s.bloom_threshold.clamp(0.0, 0.98);
    // brightness (0..765 summed channels, scaled by luma weights) to bloom amount, as a table
    let mut ramp = [0f32; 256];
    for (i, v) in ramp.iter_mut().enumerate() {
        let b = ((i as f32 / 255.0 - thr) / (1.0 - thr)).clamp(0.0, 1.0);
        *v = b * b * (3.0 - 2.0 * b) / 255.0;
    }
    let mut low = vec![[0f32; 3]; lw * lh];
    let color = &f.color;
    par_rows(&mut low, lw, threads, |rows, y0| {
        for (r, row) in rows.chunks_mut(lw).enumerate() {
            let ly = y0 + r;
            let (ya, yb) = (ly * fct, ((ly + 1) * fct).min(h));
            for (lx, out) in row.iter_mut().enumerate() {
                let (xa, xb) = (lx * fct, ((lx + 1) * fct).min(w));
                let mut acc = [0u32; 3];
                for y in ya..yb {
                    for p in &color[y * w + xa..y * w + xb] {
                        acc[0] += p[0] as u32;
                        acc[1] += p[1] as u32;
                        acc[2] += p[2] as u32;
                    }
                }
                let n = ((yb - ya) * (xb - xa)) as u32;
                let l = (acc[0] * 77 + acc[1] * 151 + acc[2] * 28) / (256 * n);
                let b = ramp[l.min(255) as usize];
                if b > 0.0 {
                    let k = b / n as f32;
                    *out = [acc[0] as f32 * k, acc[1] as f32 * k, acc[2] as f32 * k];
                }
            }
        }
    });
    if low.iter().all(|v| v[0] + v[1] + v[2] <= 0.0) {
        return;
    }
    let r = ((1.0 + s.bloom_radius.clamp(0.0, 1.0) * lh as f32 * 0.05) * 0.6).round().max(1.0) as usize;
    let mut tmp = vec![[0f32; 3]; lw * lh];
    for _ in 0..3 {
        box_blur_h(&low, &mut tmp, lw, lh, r);
        box_blur_v(&tmp, &mut low, lw, lh, r);
    }
    let k = s.bloom.clamp(0.0, 2.0) * 255.0 * 1.4;
    low.iter_mut().for_each(|v| v.iter_mut().for_each(|c| *c *= k));
    // bilinear weights for every column, worked out once
    let cols: Vec<(usize, usize, f32)> = (0..w)
        .map(|x| {
            let fx = (x as f32 + 0.5) / fct as f32 - 0.5;
            let xa = (fx.floor().max(0.0) as usize).min(lw - 1);
            (xa, (xa + 1).min(lw - 1), (fx - fx.floor()).clamp(0.0, 1.0))
        })
        .collect();
    let (low, cols) = (&low, &cols);
    par_rows(&mut f.color, w, threads, |rows, y0| {
        for (r, row) in rows.chunks_mut(w).enumerate() {
            let fy = ((y0 + r) as f32 + 0.5) / fct as f32 - 0.5;
            let ya = (fy.floor().max(0.0) as usize).min(lh - 1);
            let yb = (ya + 1).min(lh - 1);
            let t = (fy - fy.floor()).clamp(0.0, 1.0);
            let (la, lb) = (&low[ya * lw..(ya + 1) * lw], &low[yb * lw..(yb + 1) * lw]);
            for (p, &(xa, xb, u)) in row.iter_mut().zip(cols) {
                let (a, b, c, d) = (la[xa], la[xb], lb[xa], lb[xb]);
                if a[0] + a[1] + a[2] + b[0] + b[1] + b[2] + c[0] + c[1] + c[2] + d[0] + d[1] + d[2] < 0.5 {
                    continue;
                }
                for ch in 0..3 {
                    let top = a[ch] + (b[ch] - a[ch]) * u;
                    let v = top + (c[ch] + (d[ch] - c[ch]) * u - top) * t;
                    p[ch] = (p[ch] as f32 + v).min(255.0) as u8;
                }
            }
        }
    });
}

/// A CRT screen: bulging glass, colour fringes and an RGB shadow mask.
pub(crate) fn crt(f: &mut Frame, s: &PsxStyle, threads: usize) {
    let (w, h) = (f.w as usize, f.h as usize);
    let k = s.crt_curve.clamp(0.0, 1.0) * 0.22;
    let ca = s.chroma.clamp(0.0, 1.0) * 0.012;
    let mask = s.crt_mask.clamp(0.0, 1.0);
    let warp = k > 0.0 || ca > 0.0;
    // the shadow mask as two brightness tables: the pixel's own colour stripe and the other two
    let boost = 1.0 + mask * 0.4;
    let lo = (1.0 - mask * 0.55) * boost;
    let mut hi_t = [0u8; 256];
    let mut lo_t = [0u8; 256];
    for i in 0..256 {
        hi_t[i] = (i as f32 * boost).min(255.0) as u8;
        lo_t[i] = (i as f32 * lo).min(255.0) as u8;
    }
    let src = if warp { f.color.clone() } else { Vec::new() };
    // per column: the screen position and the curve factor for rows
    let cols: Vec<(f32, f32)> = (0..w)
        .map(|x| {
            let u = (x as f32 + 0.5) / w as f32 * 2.0 - 1.0;
            (u, 1.0 + k * u * u)
        })
        .collect();
    let (src, cols, hi_t, lo_t) = (&src, &cols, &hi_t, &lo_t);
    let (wf, hf) = (w as f32 * 0.5, h as f32 * 0.5);
    let fetch = move |su: f32, sv: f32| -> usize {
        let sx = (((su + 1.0) * wf) as i32).clamp(0, w as i32 - 1) as usize;
        let sy = (((sv + 1.0) * hf) as i32).clamp(0, h as i32 - 1) as usize;
        sy * w + sx
    };
    par_rows(&mut f.color, w, threads, |rows, y0| {
        for (r, row) in rows.chunks_mut(w).enumerate() {
            let v = ((y0 + r) as f32 + 0.5) / h as f32 * 2.0 - 1.0;
            let ky = 1.0 + k * v * v;
            for (x, (p, &(u, kx))) in row.iter_mut().zip(cols).enumerate() {
                if warp {
                    let (cu, cv) = (u * ky, v * kx);
                    if cu.abs() > 1.0 || cv.abs() > 1.0 {
                        *p = [0, 0, 0, 255];
                        continue;
                    }
                    let g = src[fetch(cu, cv)];
                    let (rr, bb) = if ca > 0.0 {
                        (src[fetch(cu * (1.0 + ca), cv * (1.0 + ca))][0], src[fetch(cu * (1.0 - ca), cv * (1.0 - ca))][2])
                    } else {
                        (g[0], g[2])
                    };
                    let mut c = [rr, g[1], bb, 255];
                    if k > 0.0 {
                        // the rounded edge of the tube
                        let e = (1.0 - cu.abs()).min(1.0 - cv.abs()) * 28.0;
                        if e < 1.0 {
                            c.iter_mut().take(3).for_each(|v| *v = (*v as f32 * e) as u8);
                        }
                    }
                    *p = c;
                }
                if mask > 0.0 {
                    let ph = x % 3;
                    for ch in 0..3 {
                        p[ch] = if ch == ph { hi_t[p[ch] as usize] } else { lo_t[p[ch] as usize] };
                    }
                }
            }
        }
    });
}
