//! Transform gizmos: the interactive arrows, rings and boxes used to move, rotate and scale an
//! object in the viewport. This module holds only the geometry and maths (picking a handle,
//! turning a mouse drag into a new transform), so it can be tested without any window. The GUI
//! draws the handles and forwards pointer positions.
//!
//! All screen positions are in render pixels of a `w` x `h` frame, the same space as
//! `Camera::project` and `Camera::ray`.

use crate::math::{Quat, Vec3};
use crate::raster::Camera;
use crate::scene::Transform;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GizmoMode {
    Translate,
    Rotate,
    Scale,
}

impl GizmoMode {
    pub fn title(self) -> &'static str {
        match self {
            GizmoMode::Translate => "Move",
            GizmoMode::Rotate => "Rotate",
            GizmoMode::Scale => "Scale",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    pub const ALL: [Axis; 3] = [Axis::X, Axis::Y, Axis::Z];
    pub fn index(self) -> usize {
        self as usize
    }
    pub fn unit(self) -> Vec3 {
        match self {
            Axis::X => Vec3::new(1.0, 0.0, 0.0),
            Axis::Y => Vec3::new(0.0, 1.0, 0.0),
            Axis::Z => Vec3::new(0.0, 0.0, 1.0),
        }
    }
    /// The two other axes, in a fixed right-handed order.
    pub fn others(self) -> (Axis, Axis) {
        match self {
            Axis::X => (Axis::Y, Axis::Z),
            Axis::Y => (Axis::Z, Axis::X),
            Axis::Z => (Axis::X, Axis::Y),
        }
    }
}

/// Which part of the gizmo the pointer grabbed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Handle {
    /// An arrow, ring or scale box tied to one axis.
    Axis(Axis),
    /// A small square that moves along the plane whose normal is the given axis.
    Plane(Axis),
    /// The centre box of the scale gizmo: scales on all axes together.
    Uniform,
}

/// Rounding of results while dragging. `enabled` is usually tied to a held key or a toggle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Snap {
    pub enabled: bool,
    pub step_move: f32,
    pub step_rotate_deg: f32,
    pub step_scale: f32,
}

impl Default for Snap {
    fn default() -> Self {
        Snap { enabled: false, step_move: 0.25, step_rotate_deg: 15.0, step_scale: 0.1 }
    }
}

/// Rounds to the nearest multiple of `step` (a non-positive step leaves the value alone).
pub fn snap_value(v: f32, step: f32) -> f32 {
    if step > 0.0 {
        (v / step).round() * step
    } else {
        v
    }
}

/// Converts a rotation to the XYZ Euler angles (degrees) that `Transform` stores, so that
/// `Quat::from_euler(result)` rebuilds the same rotation.
pub fn euler_from_quat(q: Quat) -> Vec3 {
    let q = q.normalized();
    let c0 = q.rotate(Vec3::new(1.0, 0.0, 0.0));
    let c1 = q.rotate(Vec3::new(0.0, 1.0, 0.0));
    let c2 = q.rotate(Vec3::new(0.0, 0.0, 1.0));
    // R = Rz * Ry * Rx, so R[2][0] = -sin(b)
    let sb = (-c0.z).clamp(-1.0, 1.0);
    let b = sb.asin();
    let (a, c) = if b.cos().abs() > 2e-3 { (c1.z.atan2(c2.z), c0.y.atan2(c0.x)) } else { ((sb * c1.x).atan2(c1.y), 0.0) };
    Vec3::new(a.to_degrees(), b.to_degrees(), c.to_degrees())
}

/// World-space length that appears `px` pixels long near `at` (keeps the gizmo a constant size on screen).
pub fn world_size(cam: &Camera, at: Vec3, h: u32, px: f32) -> f32 {
    let fwd = (cam.target - cam.eye).normalized();
    let depth = (at - cam.eye).dot(fwd).max(cam.near);
    let px_per_unit = h as f32 / (2.0 * depth * (cam.fov_y * 0.5).tan());
    px / px_per_unit.max(1e-6)
}

/// Parameter `t` of the point on the line `axis_o + t * axis_d` that is closest to the ray.
/// `None` when the ray and the line are almost parallel.
pub fn closest_param_on_axis(ray_o: Vec3, ray_d: Vec3, axis_o: Vec3, axis_d: Vec3) -> Option<f32> {
    let w0 = ray_o - axis_o;
    let (a, b, c) = (ray_d.dot(ray_d), ray_d.dot(axis_d), axis_d.dot(axis_d));
    let (d, e) = (ray_d.dot(w0), axis_d.dot(w0));
    let denom = a * c - b * b;
    if denom.abs() < 1e-6 * a * c {
        return None;
    }
    Some((a * e - b * d) / denom)
}

/// Intersection of a ray with the plane through `plane_o` with normal `n`.
pub fn ray_plane(ray_o: Vec3, ray_d: Vec3, plane_o: Vec3, n: Vec3) -> Option<Vec3> {
    let denom = ray_d.dot(n);
    if denom.abs() < 1e-6 {
        return None;
    }
    let t = (plane_o - ray_o).dot(n) / denom;
    if t < 0.0 {
        return None;
    }
    Some(ray_o + ray_d * t)
}

/// Directions of the gizmo axes in world space: the object's own axes (`local`) or the world axes.
pub fn axes(t: &Transform, local: bool) -> [Vec3; 3] {
    if local {
        let q = Quat::from_euler(Vec3::new(t.rot_deg.x.to_radians(), t.rot_deg.y.to_radians(), t.rot_deg.z.to_radians()));
        [q.rotate(Axis::X.unit()), q.rotate(Axis::Y.unit()), q.rotate(Axis::Z.unit())]
    } else {
        [Axis::X.unit(), Axis::Y.unit(), Axis::Z.unit()]
    }
}

/// Points of the ring used by the rotate gizmo for `axis`, as world positions.
pub fn ring_points(origin: Vec3, dirs: &[Vec3; 3], axis: Axis, radius: f32, segments: usize) -> Vec<Vec3> {
    let (u, v) = axis.others();
    let (du, dv) = (dirs[u.index()], dirs[v.index()]);
    (0..=segments)
        .map(|i| {
            let a = i as f32 / segments as f32 * std::f32::consts::TAU;
            origin + du * (a.cos() * radius) + dv * (a.sin() * radius)
        })
        .collect()
}

/// Corners of the small plane-handle square for the plane whose normal is `normal_axis`.
pub fn plane_handle_corners(origin: Vec3, dirs: &[Vec3; 3], normal_axis: Axis, length: f32) -> [Vec3; 4] {
    let (u, v) = normal_axis.others();
    let (du, dv) = (dirs[u.index()], dirs[v.index()]);
    let (a, b) = (length * 0.30, length * 0.58);
    [origin + du * a + dv * a, origin + du * b + dv * a, origin + du * b + dv * b, origin + du * a + dv * b]
}

fn dist_to_segment(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (abx, aby) = (b.0 - a.0, b.1 - a.1);
    let len2 = abx * abx + aby * aby;
    let t = if len2 < 1e-9 { 0.0 } else { (((p.0 - a.0) * abx + (p.1 - a.1) * aby) / len2).clamp(0.0, 1.0) };
    let (cx, cy) = (a.0 + abx * t, a.1 + aby * t);
    ((p.0 - cx).powi(2) + (p.1 - cy).powi(2)).sqrt()
}

fn inside_quad(p: (f32, f32), q: &[(f32, f32); 4]) -> bool {
    let mut sign = 0.0f32;
    for i in 0..4 {
        let (a, b) = (q[i], q[(i + 1) % 4]);
        let cross = (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0);
        if cross.abs() < 1e-6 {
            continue;
        }
        if sign == 0.0 {
            sign = cross.signum();
        } else if cross.signum() != sign {
            return false;
        }
    }
    true
}

/// Everything needed to place and hit-test a gizmo.
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub w: u32,
    pub h: u32,
    /// Handle length on screen, in pixels.
    pub size_px: f32,
}

/// Finds the handle under the pointer. `tol_px` is the grab distance in pixels.
pub fn pick_handle(mode: GizmoMode, cam: &Camera, t: &Transform, local: bool, px: (f32, f32), view: View, tol_px: f32) -> Option<Handle> {
    let origin = t.pos;
    let len = world_size(cam, origin, view.h, view.size_px);
    let dirs = axes(t, local || mode == GizmoMode::Scale);
    let proj = |p: Vec3| cam.project(p, view.w, view.h).map(|(x, y, _)| (x, y));
    let o = proj(origin)?;
    let mut best: Option<(Handle, f32)> = None;
    let mut consider = |h: Handle, d: f32| {
        if d <= tol_px && best.map_or(true, |(_, bd)| d < bd) {
            best = Some((h, d));
        }
    };
    match mode {
        GizmoMode::Translate => {
            for ax in Axis::ALL {
                if let Some(e) = proj(origin + dirs[ax.index()] * len) {
                    consider(Handle::Axis(ax), dist_to_segment(px, o, e));
                }
                let c = plane_handle_corners(origin, &dirs, ax, len);
                let pts: Vec<(f32, f32)> = c.iter().filter_map(|p| proj(*p)).collect();
                if pts.len() == 4 {
                    let quad = [pts[0], pts[1], pts[2], pts[3]];
                    if inside_quad(px, &quad) {
                        consider(Handle::Plane(ax), 0.0); // squares win over the arrows next to them
                    }
                }
            }
        }
        GizmoMode::Scale => {
            let dc = ((px.0 - o.0).powi(2) + (px.1 - o.1).powi(2)).sqrt();
            consider(Handle::Uniform, (dc - 9.0).max(0.0) * 0.5);
            for ax in Axis::ALL {
                if let Some(e) = proj(origin + dirs[ax.index()] * len) {
                    consider(Handle::Axis(ax), dist_to_segment(px, o, e));
                }
            }
        }
        GizmoMode::Rotate => {
            for ax in Axis::ALL {
                let pts: Vec<(f32, f32)> = ring_points(origin, &dirs, ax, len, 48).iter().filter_map(|p| proj(*p)).collect();
                for pair in pts.windows(2) {
                    consider(Handle::Axis(ax), dist_to_segment(px, pair[0], pair[1]));
                }
            }
        }
    }
    best.map(|(h, _)| h)
}

/// A drag in progress. Created when the pointer presses a handle; [`GizmoDrag::update`] turns
/// each new pointer position into the transform the object should have.
#[derive(Clone, Debug)]
pub struct GizmoDrag {
    pub mode: GizmoMode,
    pub handle: Handle,
    start: Transform,
    dirs: [Vec3; 3],
    len: f32,
    size_px: f32,
    start_param: f32,
    start_point: Vec3,
    origin_px: (f32, f32),
    start_dist_px: f32,
}

impl GizmoDrag {
    pub fn begin(mode: GizmoMode, handle: Handle, t: &Transform, local: bool, cam: &Camera, px: (f32, f32), view: View) -> Option<GizmoDrag> {
        let origin = t.pos;
        let len = world_size(cam, origin, view.h, view.size_px);
        let dirs = axes(t, local || mode == GizmoMode::Scale);
        let (ro, rd) = cam.ray(px.0, px.1, view.w, view.h);
        let origin_px = cam.project(origin, view.w, view.h).map(|(x, y, _)| (x, y))?;
        let start_dist_px = ((px.0 - origin_px.0).powi(2) + (px.1 - origin_px.1).powi(2)).sqrt();
        let (mut start_param, mut start_point) = (0.0, origin);
        match (mode, handle) {
            (GizmoMode::Translate, Handle::Axis(a)) | (GizmoMode::Scale, Handle::Axis(a)) => {
                start_param = closest_param_on_axis(ro, rd, origin, dirs[a.index()])?;
            }
            (GizmoMode::Translate, Handle::Plane(a)) | (GizmoMode::Rotate, Handle::Axis(a)) => {
                start_point = ray_plane(ro, rd, origin, dirs[a.index()])?;
            }
            (GizmoMode::Scale, Handle::Uniform) => {}
            _ => return None,
        }
        Some(GizmoDrag { mode, handle, start: t.clone(), dirs, len, size_px: view.size_px, start_param, start_point, origin_px, start_dist_px })
    }

    pub fn start_transform(&self) -> &Transform {
        &self.start
    }

    /// The transform for the pointer at `px`. Returns the start transform when the pointer ray
    /// cannot be resolved (for example when it runs parallel to the dragged axis).
    pub fn update(&self, cam: &Camera, px: (f32, f32), view: View, snap: Snap) -> Transform {
        let mut out = self.start.clone();
        let origin = self.start.pos;
        let (ro, rd) = cam.ray(px.0, px.1, view.w, view.h);
        match (self.mode, self.handle) {
            (GizmoMode::Translate, Handle::Axis(a)) => {
                let d = self.dirs[a.index()];
                if let Some(t) = closest_param_on_axis(ro, rd, origin, d) {
                    let mut delta = t - self.start_param;
                    if snap.enabled {
                        delta = snap_value(delta, snap.step_move);
                    }
                    out.pos = origin + d * delta;
                }
            }
            (GizmoMode::Translate, Handle::Plane(a)) => {
                if let Some(p) = ray_plane(ro, rd, origin, self.dirs[a.index()]) {
                    let delta = p - self.start_point;
                    let (u, v) = a.others();
                    let (du, dv) = (self.dirs[u.index()], self.dirs[v.index()]);
                    let (mut su, mut sv) = (delta.dot(du), delta.dot(dv));
                    if snap.enabled {
                        su = snap_value(su, snap.step_move);
                        sv = snap_value(sv, snap.step_move);
                    }
                    out.pos = origin + du * su + dv * sv;
                }
            }
            (GizmoMode::Rotate, Handle::Axis(a)) => {
                let n = self.dirs[a.index()];
                if let Some(p) = ray_plane(ro, rd, origin, n) {
                    let (v0, v1) = (self.start_point - origin, p - origin);
                    if v0.length() > 1e-6 && v1.length() > 1e-6 {
                        let mut angle = n.dot(v0.cross(v1)).atan2(v0.dot(v1)).to_degrees();
                        if snap.enabled {
                            angle = snap_value(angle, snap.step_rotate_deg);
                        }
                        let start_q = Quat::from_euler(Vec3::new(self.start.rot_deg.x.to_radians(), self.start.rot_deg.y.to_radians(), self.start.rot_deg.z.to_radians()));
                        let q = Quat::from_axis_angle(n, angle.to_radians()).mul(start_q);
                        out.rot_deg = euler_from_quat(q);
                    }
                }
            }
            (GizmoMode::Scale, Handle::Axis(a)) => {
                let d = self.dirs[a.index()];
                if let Some(t) = closest_param_on_axis(ro, rd, origin, d) {
                    let mut f = 1.0 + (t - self.start_param) / self.len.max(1e-6);
                    if snap.enabled {
                        f = snap_value(f, snap.step_scale);
                    }
                    let f = f.max(0.01);
                    match a {
                        Axis::X => out.scale.x = self.start.scale.x * f,
                        Axis::Y => out.scale.y = self.start.scale.y * f,
                        Axis::Z => out.scale.z = self.start.scale.z * f,
                    }
                }
            }
            (GizmoMode::Scale, Handle::Uniform) => {
                let dist = ((px.0 - self.origin_px.0).powi(2) + (px.1 - self.origin_px.1).powi(2)).sqrt();
                let mut f = 1.0 + (dist - self.start_dist_px) / self.size_px.max(1.0);
                if snap.enabled {
                    f = snap_value(f, snap.step_scale);
                }
                let f = f.max(0.01);
                out.scale = self.start.scale * f;
            }
            _ => {}
        }
        out.scale = Vec3::new(out.scale.x.max(0.001), out.scale.y.max(0.001), out.scale.z.max(0.001));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: u32 = 320;
    const H: u32 = 240;
    fn view() -> View {
        View { w: W, h: H, size_px: 90.0 }
    }
    fn cam() -> Camera {
        Camera::orbit(Vec3::ZERO, 0.6, 0.45, 6.0)
    }
    fn px_of(c: &Camera, p: Vec3) -> (f32, f32) {
        let (x, y, _) = c.project(p, W, H).unwrap();
        (x, y)
    }
    fn near(a: f32, b: f32, tol: f32) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn snapping_rounds_to_steps() {
        assert_eq!(snap_value(0.37, 0.25), 0.25);
        assert_eq!(snap_value(0.38, 0.25), 0.5);
        assert_eq!(snap_value(-7.4, 15.0), 0.0);
        assert_eq!(snap_value(1.234, 0.0), 1.234);
    }

    #[test]
    fn euler_conversion_round_trips() {
        for (x, y, z) in [(0.0, 0.0, 0.0), (30.0, 40.0, 50.0), (-120.0, 20.0, 170.0), (10.0, -80.0, -30.0), (90.0, 0.0, 0.0)] {
            let e = Vec3::new(x, y, z);
            let q = Quat::from_euler(Vec3::new(x.to_radians(), y.to_radians(), z.to_radians()));
            let back = euler_from_quat(q);
            let q2 = Quat::from_euler(Vec3::new(back.x.to_radians(), back.y.to_radians(), back.z.to_radians()));
            for v in [Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.3, -0.7, 0.2)] {
                let (a, b) = (q.rotate(v), q2.rotate(v));
                assert!((a - b).length() < 1e-4, "euler {e:?} -> {back:?}");
            }
        }
    }

    #[test]
    fn gimbal_lock_still_gives_a_valid_rotation() {
        let q = Quat::from_euler(Vec3::new(0.3, std::f32::consts::FRAC_PI_2, 0.2));
        let e = euler_from_quat(q);
        let q2 = Quat::from_euler(Vec3::new(e.x.to_radians(), e.y.to_radians(), e.z.to_radians()));
        let v = Vec3::new(0.5, 0.6, 0.7);
        assert!((q.rotate(v) - q2.rotate(v)).length() < 1e-3);
    }

    #[test]
    fn closest_point_on_axis_is_found_and_parallel_is_rejected() {
        let t = closest_param_on_axis(Vec3::new(2.0, 0.0, 5.0), Vec3::new(0.0, 0.0, -1.0), Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0)).unwrap();
        assert!(near(t, 2.0, 1e-4));
        assert!(closest_param_on_axis(Vec3::new(0.0, 1.0, 0.0), Vec3::new(1.0, 0.0, 0.0), Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0)).is_none());
    }

    #[test]
    fn gizmo_keeps_its_screen_size_at_any_distance() {
        let near_cam = Camera::orbit(Vec3::ZERO, 0.3, 0.2, 3.0);
        let far_cam = Camera::orbit(Vec3::ZERO, 0.3, 0.2, 30.0);
        for c in [near_cam, far_cam] {
            let l = world_size(&c, Vec3::ZERO, H, 90.0);
            let a = px_of(&c, Vec3::ZERO);
            let b = px_of(&c, Vec3::new(l, 0.0, 0.0));
            let d = ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt();
            assert!(d > 30.0 && d < 100.0, "projected length {d}");
        }
    }

    #[test]
    fn dragging_the_x_arrow_moves_along_x_only() {
        let c = cam();
        let t = Transform::default();
        let len = world_size(&c, t.pos, H, 90.0);
        let start = px_of(&c, Vec3::new(len * 0.5, 0.0, 0.0));
        assert_eq!(pick_handle(GizmoMode::Translate, &c, &t, false, start, view(), 6.0), Some(Handle::Axis(Axis::X)));
        let drag = GizmoDrag::begin(GizmoMode::Translate, Handle::Axis(Axis::X), &t, false, &c, start, view()).unwrap();
        let end = px_of(&c, Vec3::new(len * 0.5 + 1.5, 0.3, -0.4)); // the pointer wanders off the axis
        let r = drag.update(&c, end, view(), Snap::default());
        assert!(near(r.pos.x, 1.5, 0.15), "x = {}", r.pos.x);
        assert!(r.pos.y == 0.0 && r.pos.z == 0.0);
    }

    #[test]
    fn translate_snaps_to_the_step() {
        let c = cam();
        let t = Transform::default();
        let len = world_size(&c, t.pos, H, 90.0);
        let start = px_of(&c, Vec3::new(len * 0.5, 0.0, 0.0));
        let drag = GizmoDrag::begin(GizmoMode::Translate, Handle::Axis(Axis::X), &t, false, &c, start, view()).unwrap();
        let end = px_of(&c, Vec3::new(len * 0.5 + 1.37, 0.0, 0.0));
        let r = drag.update(&c, end, view(), Snap { enabled: true, step_move: 0.5, ..Snap::default() });
        assert!(near(r.pos.x, 1.5, 1e-3), "x = {}", r.pos.x);
    }

    #[test]
    fn plane_handle_moves_in_two_axes() {
        let c = cam();
        let t = Transform::default();
        let len = world_size(&c, t.pos, H, 90.0);
        // the XZ plane has normal Y; grab the square between the X and Z arrows
        let s = Vec3::new(len * 0.45, 0.0, len * 0.45);
        let start = px_of(&c, s);
        assert_eq!(pick_handle(GizmoMode::Translate, &c, &t, false, start, view(), 6.0), Some(Handle::Plane(Axis::Y)));
        let drag = GizmoDrag::begin(GizmoMode::Translate, Handle::Plane(Axis::Y), &t, false, &c, start, view()).unwrap();
        let r = drag.update(&c, px_of(&c, s + Vec3::new(1.0, 0.0, -2.0)), view(), Snap::default());
        assert!(near(r.pos.x, 1.0, 0.1) && near(r.pos.z, -2.0, 0.1) && r.pos.y.abs() < 1e-4, "{:?}", r.pos);
    }

    #[test]
    fn rotating_the_y_ring_a_quarter_turn_gives_ninety_degrees() {
        let c = Camera::orbit(Vec3::ZERO, 0.0, 1.2, 6.0); // looking mostly down on the Y ring
        let t = Transform::default();
        let len = world_size(&c, t.pos, H, 90.0);
        let dirs = axes(&t, false);
        let ring = ring_points(t.pos, &dirs, Axis::Y, len, 48);
        // ring starts on +Z (others of Y are Z, X) and moves towards +X
        let start = px_of(&c, ring[6]); // 45 degrees round the ring, away from the points where rings cross
        assert_eq!(pick_handle(GizmoMode::Rotate, &c, &t, false, start, view(), 6.0), Some(Handle::Axis(Axis::Y)));
        let drag = GizmoDrag::begin(GizmoMode::Rotate, Handle::Axis(Axis::Y), &t, false, &c, start, view()).unwrap();
        let r = drag.update(&c, px_of(&c, ring[18]), view(), Snap::default()); // 12 more segments of 7.5 degrees = a quarter turn
        // the ring goes from +Z towards +X, which is a rotation of +90 degrees about +Y
        assert!(near(r.rot_deg.y, 90.0, 1.0), "rot = {:?}", r.rot_deg);
        assert!(r.rot_deg.x.abs() < 1.0 && r.rot_deg.z.abs() < 1.0);
        let snapped = drag.update(&c, px_of(&c, ring[16]), view(), Snap { enabled: true, step_rotate_deg: 45.0, ..Snap::default() });
        assert!(near(snapped.rot_deg.y, 90.0, 1.0), "{:?}", snapped.rot_deg);
    }

    #[test]
    fn scale_axis_and_uniform() {
        let c = cam();
        let mut t = Transform::default();
        t.scale = Vec3::new(2.0, 2.0, 2.0);
        let len = world_size(&c, t.pos, H, 90.0);
        let start = px_of(&c, Vec3::new(0.0, len, 0.0));
        let drag = GizmoDrag::begin(GizmoMode::Scale, Handle::Axis(Axis::Y), &t, false, &c, start, view()).unwrap();
        let r = drag.update(&c, px_of(&c, Vec3::new(0.0, len * 1.5, 0.0)), view(), Snap::default());
        assert!(near(r.scale.y, 3.0, 0.1), "{:?}", r.scale);
        assert!(r.scale.x == 2.0 && r.scale.z == 2.0);
        // uniform: pull away from the centre
        let o = px_of(&c, Vec3::ZERO);
        let u = GizmoDrag::begin(GizmoMode::Scale, Handle::Uniform, &t, false, &c, (o.0 + 20.0, o.1), view()).unwrap();
        let r = u.update(&c, (o.0 + 65.0, o.1), view(), Snap::default());
        assert!(r.scale.x > 2.5 && near(r.scale.x, r.scale.y, 1e-5) && near(r.scale.y, r.scale.z, 1e-5), "{:?}", r.scale);
        // shrinking never reaches zero or goes negative
        let r = u.update(&c, (o.0 - 500.0, o.1), view(), Snap::default());
        assert!(r.scale.x >= 0.001);
    }

    #[test]
    fn picking_far_from_every_handle_finds_nothing() {
        let c = cam();
        let t = Transform::default();
        assert_eq!(pick_handle(GizmoMode::Translate, &c, &t, false, (5.0, 5.0), view(), 6.0), None);
        assert_eq!(pick_handle(GizmoMode::Rotate, &c, &t, false, (5.0, 5.0), view(), 6.0), None);
    }

    #[test]
    fn local_axes_follow_the_object_rotation() {
        let mut t = Transform::default();
        t.rot_deg = Vec3::new(0.0, 90.0, 0.0);
        let l = axes(&t, true);
        assert!((l[0] - Vec3::new(0.0, 0.0, -1.0)).length() < 1e-5, "{:?}", l[0]);
        let w = axes(&t, false);
        assert_eq!(w[0], Vec3::new(1.0, 0.0, 0.0));
    }
}
