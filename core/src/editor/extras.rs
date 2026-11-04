//! Smaller editor features built on top of the core: inverse kinematics while posing, the graph
//! editor's channel access, reference pictures in the viewport, indexed (palette) textures and
//! projection painting (painting through the screen instead of through the UV map).
use super::*;
use crate::anim::{Channel, Ease};
use crate::palette::{Indexed, Palette};
use crate::scene::RefImage;

/// Standard orientations for reference pictures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefView {
    /// Facing the front view (the picture lies in the XY plane).
    Front,
    /// Facing the side view (YZ plane).
    Side,
    /// Lying on the ground (XZ plane), seen from above.
    Top,
    /// Facing the current viewport camera.
    Camera,
}

/// State of one projection-paint stroke.
pub struct ProjStroke {
    /// Brush radius in screen pixels (chosen at the press so the brush looks as big as in UV painting).
    radius: f32,
    canvases: Vec<ProjCanvas>,
}
struct ProjCanvas {
    c: Canvas,
    orig: Texture,
    /// Strongest coverage of each texel so far: overlapping dabs of one stroke never darken twice.
    cov: Vec<f32>,
}

fn rot_y_deg(v: Vec3) -> (f32, f32) {
    let d = v.normalized();
    (d.x.atan2(d.z).to_degrees(), d.y.clamp(-1.0, 1.0).asin().to_degrees())
}

impl Editor {
    // -------------------------------------------------------------- inverse kinematics

    /// Drags the joint of `bone` of the selected object to screen position (sx, sy) of a w×h view:
    /// the `ik_chain` bones above it rotate so the joint follows the mouse (CCD inverse kinematics).
    /// The target lies on the plane through the joint that faces the camera.
    pub fn ik_drag(&mut self, bone: usize, sx: f32, sy: f32, w: u32, h: u32) -> bool {
        let Some(id) = self.selected_object else { return false };
        let Some(joint) = self.bone_joint_world(bone) else { return false };
        let Some(t) = self.object_transform(id) else { return false };
        let cam = self.active_camera();
        let (o, d) = cam.ray(sx, sy, w, h);
        let n = (cam.target - cam.eye).normalized();
        let Some(target_world) = crate::gizmo::ray_plane(o, d, joint, n) else { return false };
        let Some(inv) = t.matrix().inverse() else { return false };
        let target = inv.transform_point(target_world);
        let chain = self.ik_chain.max(1);
        self.pose_edit(|pose, skel| {
            crate::ik::solve(skel, pose, bone, chain, target, 16);
        })
    }

    // -------------------------------------------------------------- graph editor

    /// The animated channels of the current animation (values that change between frames).
    pub fn graph_channels(&self) -> Vec<Channel> {
        self.animation().map(|a| a.channels()).unwrap_or_default()
    }
    /// The value of a channel at frame `i` (the key).
    pub fn graph_key(&self, i: usize, ch: &Channel) -> Option<f32> {
        let f = self.animation()?.frames.get(i)?;
        ch.read(f.states.iter().find(|s| s.object == ch.object), &f.camera)
    }
    /// The curve: value at a fractional tick, with tweens and easing.
    pub fn graph_sample(&self, ch: &Channel, tick: f32) -> Option<f32> {
        self.animation()?.sample(ch, tick)
    }
    /// Changes the key of a channel at frame `i`. Several calls inside `begin_group`/`end_group` (a drag) undo together.
    pub fn graph_set_key(&mut self, i: usize, ch: &Channel, v: f32) -> bool {
        let ai = self.current_anim;
        if self.project.animations.get(ai).and_then(|a| a.frames.get(i)).is_none() || !v.is_finite() {
            return false;
        }
        self.record(Slot::Frame(ai, i));
        let ok = ch.write(&mut self.project.animations[ai].frames[i], v);
        self.dirty = true;
        ok
    }
    /// Sets the easing of the in-between that starts at frame `i` (and turns the in-between on).
    pub fn graph_set_ease(&mut self, i: usize, ease: Ease) -> bool {
        let ai = self.current_anim;
        if self.project.animations.get(ai).and_then(|a| a.frames.get(i)).is_none() {
            return false;
        }
        self.record(Slot::Frame(ai, i));
        let f = &mut self.project.animations[ai].frames[i];
        f.ease = ease.clamped();
        f.tween = true;
        self.dirty = true;
        true
    }

    // -------------------------------------------------------------- reference pictures

    /// Adds a reference picture in front of the current view. Reference pictures are only drawn in
    /// the editing viewport, never in renders or exports.
    pub fn ref_add(&mut self, image: Texture, name: &str) -> usize {
        self.end_group();
        self.record(Slot::Scene);
        let mut r = RefImage::new(name, image);
        let cam = self.view.camera();
        r.transform.pos = cam.target;
        r.size = ((cam.eye - cam.target).length() * 0.6).clamp(0.2, 50.0);
        self.project.scene.refs.push(r);
        let i = self.project.scene.refs.len() - 1;
        self.ref_align(i, RefView::Camera);
        self.selected_ref = Some(i);
        i
    }
    /// Loads a picture file as a reference.
    pub fn ref_add_path(&mut self, path: &Path) -> Result<usize, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("Could not read \"{}\": {e}", path.display()))?;
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("Reference").to_string();
        let img = crate::decode::image::decode_image(&name, &bytes)?;
        let mut t = Texture { name: name.clone(), w: img.w, h: img.h, px: img.rgba.chunks_exact(4).map(|p| [p[0], p[1], p[2], p[3]]).collect() };
        let m = t.w.max(t.h);
        if m > 2048 {
            let k = 2048.0 / m as f32;
            t = t.resized(((t.w as f32 * k) as u32).max(1), ((t.h as f32 * k) as u32).max(1));
        }
        Ok(self.ref_add(t, &name))
    }
    /// Changes a reference picture. Calls inside one undo group (a slider drag) undo together.
    pub fn ref_edit(&mut self, i: usize, f: impl FnOnce(&mut RefImage)) -> bool {
        if i >= self.project.scene.refs.len() {
            return false;
        }
        self.record(Slot::Scene);
        f(&mut self.project.scene.refs[i]);
        let r = &mut self.project.scene.refs[i];
        r.opacity = r.opacity.clamp(0.0, 1.0);
        r.size = r.size.clamp(0.01, 1000.0);
        self.dirty = true;
        true
    }
    pub fn ref_remove(&mut self, i: usize) -> bool {
        if i >= self.project.scene.refs.len() {
            return false;
        }
        self.end_group();
        self.record(Slot::Scene);
        self.project.scene.refs.remove(i);
        self.selected_ref = None;
        true
    }
    /// Turns a reference picture to a standard orientation (keeps its position).
    pub fn ref_align(&mut self, i: usize, view: RefView) -> bool {
        let rot = match view {
            RefView::Front => Vec3::ZERO,
            RefView::Side => Vec3::new(0.0, 90.0, 0.0),
            RefView::Top => Vec3::new(-90.0, 0.0, 0.0),
            RefView::Camera => {
                let cam = self.active_camera();
                let (yaw, pitch) = rot_y_deg(cam.eye - cam.target);
                Vec3::new(-pitch, yaw, 0.0)
            }
        };
        self.ref_edit(i, |r| r.transform.rot_deg = rot)
    }
    /// The reference picture under a screen position (locked and hidden pictures are skipped).
    pub fn ref_pick(&self, sx: f32, sy: f32, w: u32, h: u32) -> Option<usize> {
        let cam = self.active_camera();
        let (o, d) = cam.ray(sx, sy, w, h);
        let mut best: Option<(f32, usize)> = None;
        for (i, r) in self.project.scene.refs.iter().enumerate() {
            if !r.visible || r.locked {
                continue;
            }
            let c = r.corners();
            let (eu, ev) = (c[1] - c[0], c[3] - c[0]);
            let n = eu.cross(ev);
            let Some(p) = crate::gizmo::ray_plane(o, d, c[0], n) else { continue };
            let s = (p - c[0]).dot(eu) / eu.dot(eu).max(1e-12);
            let t = (p - c[0]).dot(ev) / ev.dot(ev).max(1e-12);
            let dist = (p - o).length();
            if (0.0..=1.0).contains(&s) && (0.0..=1.0).contains(&t) && best.map_or(true, |(bd, _)| dist < bd) {
                best = Some((dist, i));
            }
        }
        best.map(|(_, i)| i)
    }
    /// Draws the reference pictures into a rendered viewport frame (perspective-correct, with
    /// opacity, hidden behind models unless `on_top`). Never used for renders.
    pub fn draw_refs(&self, r: &mut Renderer, cam: &Camera) {
        if self.mode == Mode::Render || self.project.scene.refs.is_empty() {
            return;
        }
        let f = &mut r.frame;
        let (w, h) = (f.w, f.h);
        if w == 0 || h == 0 {
            return;
        }
        let vp = cam.proj(w as f32 / h as f32) * cam.view();
        let Some(inv) = vp.inverse() else { return };
        for (ri, rf) in self.project.scene.refs.iter().enumerate() {
            if !rf.visible || rf.opacity <= 0.0 || rf.image.w == 0 || rf.image.h == 0 {
                continue;
            }
            let c = rf.corners();
            let mut pts = vec![];
            for p in c {
                match cam.project(p, w, h) {
                    Some(s) => pts.push(s),
                    None => break,
                }
            }
            if pts.len() < 4 {
                continue;
            }
            let x0 = pts.iter().map(|p| p.0).fold(f32::MAX, f32::min).floor().max(0.0) as i32;
            let x1 = pts.iter().map(|p| p.0).fold(f32::MIN, f32::max).ceil().min(w as f32 - 1.0) as i32;
            let y0 = pts.iter().map(|p| p.1).fold(f32::MAX, f32::min).floor().max(0.0) as i32;
            let y1 = pts.iter().map(|p| p.1).fold(f32::MIN, f32::max).ceil().min(h as f32 - 1.0) as i32;
            if x0 > x1 || y0 > y1 {
                continue;
            }
            let (eu, ev) = (c[1] - c[0], c[3] - c[0]);
            let (luu, lvv) = (eu.dot(eu).max(1e-12), ev.dot(ev).max(1e-12));
            let n = eu.cross(ev);
            let img = &rf.image;
            let selected = self.selected_ref == Some(ri);
            let op = rf.opacity;
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let (nx, ny) = ((x as f32 + 0.5) / w as f32 * 2.0 - 1.0, 1.0 - (y as f32 + 0.5) / h as f32 * 2.0);
                    let a = inv.mul_vec4([nx, ny, -1.0, 1.0]);
                    let b = inv.mul_vec4([nx, ny, 1.0, 1.0]);
                    let (pa, pb) = (Vec3::new(a[0] / a[3], a[1] / a[3], a[2] / a[3]), Vec3::new(b[0] / b[3], b[1] / b[3], b[2] / b[3]));
                    let Some(p) = crate::gizmo::ray_plane(pa, (pb - pa).normalized(), c[0], n) else { continue };
                    let s = (p - c[0]).dot(eu) / luu;
                    let t = (p - c[0]).dot(ev) / lvv;
                    if !(0.0..1.0).contains(&s) || !(0.0..1.0).contains(&t) {
                        continue;
                    }
                    let idx = (y as u32 * w + x as u32) as usize;
                    if !rf.on_top {
                        let cl = vp.mul_vec4([p.x, p.y, p.z, 1.0]);
                        if cl[3] <= 0.0 || cl[2] / cl[3] >= f.depth[idx] {
                            continue;
                        }
                    }
                    let border = selected && {
                        let (bs, bt) = (s.min(1.0 - s) * (luu.sqrt()), t.min(1.0 - t) * (lvv.sqrt()));
                        let px_world = (p - cam.eye).length() * (cam.fov_y * 0.5).tan() * 2.0 / h as f32;
                        bs.min(bt) < px_world * 2.0
                    };
                    let src = if border {
                        [255, 200, 60, 255]
                    } else {
                        let (ix, iy) = (((s * img.w as f32) as u32).min(img.w - 1), ((t * img.h as f32) as u32).min(img.h - 1));
                        img.px[(iy * img.w + ix) as usize]
                    };
                    let k = if border { 1.0 } else { op * src[3] as f32 / 255.0 };
                    if k <= 0.0 {
                        continue;
                    }
                    let d = &mut f.color[idx];
                    for ch in 0..3 {
                        d[ch] = (d[ch] as f32 + (src[ch] as f32 - d[ch] as f32) * k).round() as u8;
                    }
                }
            }
        }
    }

    // -------------------------------------------------------------- indexed textures

    pub fn indexed(&self, asset: usize, tex: usize) -> Option<&Indexed> {
        self.project.assets.get(asset)?.indexed(tex)
    }
    /// Turns a texture into an indexed picture: every pixel takes the nearest colour of `palette`,
    /// and later changes of a palette colour recolour all its pixels. Textures with layers must be merged first.
    pub fn indexed_enable(&mut self, asset: usize, tex: usize, palette: Palette) -> bool {
        let Some(a) = self.project.assets.get(asset) else { return false };
        if a.stack(tex).is_some() {
            self.status = "Merge the texture layers first: indexed colour works on a single picture".into();
            return false;
        }
        if a.indexed(tex).is_some() || a.model.textures.get(tex).is_none() {
            return false;
        }
        self.end_group();
        self.begin_group();
        self.record(Slot::Indexed(asset));
        self.record(Slot::Texture(asset, tex));
        let a = &mut self.project.assets[asset];
        let ix = Indexed::from_texture(&mut a.model.textures[tex], palette);
        a.indexed.push((tex, ix));
        self.end_group();
        true
    }
    /// Stops using the palette (the picture stays as it is).
    pub fn indexed_disable(&mut self, asset: usize, tex: usize) -> bool {
        if self.indexed(asset, tex).is_none() {
            return false;
        }
        self.end_group();
        self.record(Slot::Indexed(asset));
        self.project.assets[asset].indexed.retain(|(t, _)| *t != tex);
        true
    }
    /// Runs a change of the palette of an indexed texture as one undo step and repaints the texture.
    pub fn indexed_edit(&mut self, asset: usize, tex: usize, f: impl FnOnce(&mut Indexed, &mut Texture)) -> bool {
        if self.indexed(asset, tex).is_none() {
            return false;
        }
        self.record(Slot::Indexed(asset));
        self.record(Slot::Texture(asset, tex));
        let a = &mut self.project.assets[asset];
        let Some(i) = a.indexed.iter().position(|(t, _)| *t == tex) else { return false };
        let (ixs, model) = (&mut a.indexed, &mut a.model);
        let Some(t) = model.textures.get_mut(tex) else { return false };
        f(&mut ixs[i].1, t);
        self.dirty = true;
        true
    }
    /// Recolours every pixel that uses palette entry `i`.
    pub fn indexed_set_color(&mut self, asset: usize, tex: usize, i: usize, c: Rgba) -> bool {
        self.indexed_edit(asset, tex, |ix, t| {
            ix.set_color(i, c, t);
        })
    }
    /// After painting on an indexed texture the pixels snap back to the palette.
    pub(super) fn sync_indexed_touched(&mut self) {
        let touched: Vec<(usize, usize)> = self.touched.iter().filter_map(|s| if let Slot::Texture(a, t) = *s { Some((a, t)) } else { None }).collect();
        for (a, t) in touched {
            if self.indexed(a, t).is_none() {
                continue;
            }
            self.record(Slot::Indexed(a));
            let asset = &mut self.project.assets[a];
            let (ixs, model) = (&mut asset.indexed, &mut asset.model);
            if let (Some(ix), Some(tex)) = (ixs.iter_mut().find(|(i, _)| *i == t), model.textures.get_mut(t)) {
                ix.1.sync_from(tex);
            }
        }
    }

    // -------------------------------------------------------------- projection painting

    fn proj_dab(&mut self, frame: &Frame, cx: f32, cy: f32) -> usize {
        let Some(mut st) = self.proj.take() else { return 0 };
        let radius = st.radius;
        let b = self.brush;
        let erase = b.tool == Tool::Eraser;
        let alpha = b.color[3] as f32 / 255.0;
        let (x0, x1) = ((cx - radius).floor() as i32, (cx + radius).ceil() as i32);
        let (y0, y1) = ((cy - radius).floor() as i32, (cy + radius).ceil() as i32);
        let mut changed: Vec<Canvas> = vec![];
        let mut n = 0;
        for y in y0.max(0)..=y1.min(frame.h as i32 - 1) {
            for x in x0.max(0)..=x1.min(frame.w as i32 - 1) {
                let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt() / radius.max(0.5);
                if d > 1.0 {
                    continue;
                }
                let k = if b.pixel {
                    1.0
                } else {
                    let hard = b.hardness.clamp(0.0, 0.99);
                    if d <= hard {
                        1.0
                    } else {
                        let t = 1.0 - (d - hard) / (1.0 - hard);
                        t * t * (3.0 - 2.0 * t)
                    }
                };
                let hit = frame.hit_at(x, y);
                let Some(info) = self.pick_info(hit) else { continue };
                let Some(tex) = info.tex else { continue };
                let Some(c) = self.canvas_for(info.object, info.asset, tex) else { continue };
                let pi = match st.canvases.iter().position(|pc| pc.c == c) {
                    Some(i) => i,
                    None => {
                        if !self.canvas_prepare(c) {
                            continue;
                        }
                        let Some(orig) = self.canvas_mut(c).map(|t| t.clone()) else { continue };
                        let cov = vec![0.0; orig.px.len()];
                        st.canvases.push(ProjCanvas { c, orig, cov });
                        st.canvases.len() - 1
                    }
                };
                // texel footprint of this screen pixel (so zoomed-out textures get no holes)
                let (tw, th) = (st.canvases[pi].orig.w as f32, st.canvases[pi].orig.h as f32);
                let (tu, tv) = (info.u * tw, info.v * th);
                let mut span = 0.0f32;
                for (nx, ny) in [(x + 1, y), (x, y + 1)] {
                    let hn = frame.hit_at(nx, ny);
                    if hn.is_some() && hn.obj == hit.obj && hn.mesh == hit.mesh && hn.tri == hit.tri {
                        span = span.max((hn.u * tw - tu).abs()).max((hn.v * th - tv).abs());
                    }
                }
                let half = (span * 0.5).min(6.0);
                let sel = self.selection_for(c).cloned();
                let Some(t) = self.canvas_mut(c) else { continue };
                let pc = &mut st.canvases[pi];
                let (ax, bx) = ((tu - half).floor() as i32, (tu + half).floor() as i32);
                let (ay, by) = ((tv - half).floor() as i32, (tv + half).floor() as i32);
                for ty in ay..=by {
                    for tx in ax..=bx {
                        let Some(i) = t.idx(tx, ty) else { continue };
                        if sel.as_ref().is_some_and(|s| !s.contains(tx, ty)) {
                            continue;
                        }
                        if k <= pc.cov[i] {
                            continue;
                        }
                        pc.cov[i] = k;
                        let o = pc.orig.px[i];
                        let a = k * alpha;
                        t.px[i] = if erase {
                            [o[0], o[1], o[2], (o[3] as f32 * (1.0 - a)).round() as u8]
                        } else if b.pixel && a >= 0.999 {
                            [b.color[0], b.color[1], b.color[2], 255]
                        } else {
                            let oa = o[3] as f32 / 255.0;
                            let na = a + oa * (1.0 - a);
                            let mix = |s: u8, d: u8| ((s as f32 * a + d as f32 * oa * (1.0 - a)) / na.max(1e-6)).round().clamp(0.0, 255.0) as u8;
                            [mix(b.color[0], o[0]), mix(b.color[1], o[1]), mix(b.color[2], o[2]), (na * 255.0).round() as u8]
                        };
                        n += 1;
                    }
                }
                if !changed.contains(&c) {
                    changed.push(c);
                }
            }
        }
        for c in changed {
            self.sync_stack(c);
        }
        self.proj = Some(st);
        n
    }
    /// Starts a projection-paint stroke at a viewport pixel: the brush is a circle on the screen and
    /// paints whatever surface is visible under it, across UV seams and neighbouring objects.
    pub fn proj_press(&mut self, frame: &Frame, x: i32, y: i32) -> bool {
        let hit = frame.hit_at(x, y);
        let Some(info) = self.pick_info(hit) else { return false };
        let Some(tex) = info.tex else { return false };
        let Some(c) = self.canvas_for(info.object, info.asset, tex) else { return false };
        let Some((tw, th)) = self.canvas_texture(c).map(|t| (t.w as f32, t.h as f32)) else { return false };
        // texels per screen pixel at the press point
        let mut tpp = 0.0f32;
        for (nx, ny) in [(x + 1, y), (x, y + 1), (x - 1, y), (x, y - 1)] {
            let hn = frame.hit_at(nx, ny);
            if hn.is_some() && hn.obj == hit.obj && hn.tri == hit.tri && hn.mesh == hit.mesh {
                tpp = tpp.max((hn.u - info.u).abs() * tw).max((hn.v - info.v).abs() * th);
            }
        }
        let ppt = if tpp > 1e-4 { 1.0 / tpp } else { 4.0 };
        let radius = (self.brush.size.max(1.0) * 0.5 * ppt).clamp(1.0, 400.0);
        self.begin_group();
        self.active_texture = tex;
        self.proj = Some(ProjStroke { radius, canvases: vec![] });
        self.proj_dab(frame, x as f32 + 0.5, y as f32 + 0.5);
        true
    }
    pub fn proj_drag(&mut self, frame: &Frame, from: (i32, i32), to: (i32, i32)) -> usize {
        let Some(r) = self.proj.as_ref().map(|p| p.radius) else { return 0 };
        let len = (((to.0 - from.0) as f32).powi(2) + ((to.1 - from.1) as f32).powi(2)).sqrt();
        let step = (r * 0.3).max(1.0);
        let n = (len / step).ceil().max(1.0) as i32;
        let mut total = 0;
        for i in 1..=n {
            let k = i as f32 / n as f32;
            total += self.proj_dab(frame, from.0 as f32 + (to.0 - from.0) as f32 * k + 0.5, from.1 as f32 + (to.1 - from.1) as f32 * k + 0.5);
        }
        total
    }
    /// The brush radius of the running projection stroke in screen pixels (for the cursor circle).
    pub fn proj_radius(&self) -> Option<f32> {
        self.proj.as_ref().map(|p| p.radius)
    }
}
