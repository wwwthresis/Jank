//! Bone poses and skinning (linear blend skinning).
use crate::math::*;
use crate::mesh::*;

/// A bone's deviation from its rest pose. Everything is relative to the rest pose, so the zero pose equals the bind pose.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BonePose {
    pub pos: Vec3,
    pub rot: Quat,
    pub scale: Vec3,
}
impl Default for BonePose {
    fn default() -> Self {
        Self { pos: Vec3::ZERO, rot: Quat::IDENTITY, scale: Vec3::ONE }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pose {
    pub bones: Vec<BonePose>,
}

pub fn slerp(a: Quat, mut b: Quat, t: f32) -> Quat {
    let mut d = a.x * b.x + a.y * b.y + a.z * b.z + a.w * b.w;
    if d < 0.0 {
        b = Quat { x: -b.x, y: -b.y, z: -b.z, w: -b.w };
        d = -d;
    }
    if d > 0.9995 {
        return Quat { x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t, z: a.z + (b.z - a.z) * t, w: a.w + (b.w - a.w) * t }.normalized();
    }
    let th = d.clamp(-1.0, 1.0).acos();
    let (s0, s1) = (((1.0 - t) * th).sin() / th.sin(), (t * th).sin() / th.sin());
    Quat { x: a.x * s0 + b.x * s1, y: a.y * s0 + b.y * s1, z: a.z * s0 + b.z * s1, w: a.w * s0 + b.w * s1 }.normalized()
}

impl Pose {
    pub fn rest(n: usize) -> Pose {
        Pose { bones: vec![BonePose::default(); n] }
    }
    /// Resizes the pose to the number of bones of the skeleton (in case the skeleton changed).
    pub fn fit(&mut self, n: usize) {
        self.bones.resize(n, BonePose::default());
    }
    pub fn lerp(&self, other: &Pose, t: f32) -> Pose {
        let n = self.bones.len().min(other.bones.len());
        Pose {
            bones: (0..n)
                .map(|i| {
                    let (a, b) = (self.bones[i], other.bones[i]);
                    BonePose { pos: a.pos.lerp(b.pos, t), rot: slerp(a.rot, b.rot, t), scale: a.scale.lerp(b.scale, t) }
                })
                .collect(),
        }
    }
    /// Mirrors the pose along X: swaps bones whose names carry L/R suffixes (_L, .L, Left and so on) and reflects the rotations.
    pub fn mirrored(&self, skel: &Skeleton) -> Pose {
        let mut out = self.clone();
        for (i, b) in skel.bones.iter().enumerate() {
            let j = mirror_name(&b.name).and_then(|n| skel.find(&n)).unwrap_or(i);
            let s = self.bones.get(j).copied().unwrap_or_default();
            // reflection along X: quaternion (x,y,z,w) becomes (x,-y,-z,w), and pos.x becomes -pos.x
            out.bones[i] = BonePose { pos: Vec3::new(-s.pos.x, s.pos.y, s.pos.z), rot: Quat { x: s.rot.x, y: -s.rot.y, z: -s.rot.z, w: s.rot.w }, scale: s.scale };
        }
        out
    }
}

fn mirror_name(n: &str) -> Option<String> {
    for (a, b) in [("Left", "Right"), ("left", "right"), ("_L", "_R"), (".L", ".R"), ("_l", "_r"), (".l", ".r")] {
        if let Some(i) = n.find(a) {
            return Some(format!("{}{}{}", &n[..i], b, &n[i + a.len()..]));
        }
        if let Some(i) = n.find(b) {
            return Some(format!("{}{}{}", &n[..i], a, &n[i + b.len()..]));
        }
    }
    None
}

/// World matrices of the bones for a pose.
pub fn world_matrices(skel: &Skeleton, pose: &Pose) -> Vec<Mat4> {
    let mut w: Vec<Mat4> = Vec::with_capacity(skel.bones.len());
    for (i, b) in skel.bones.iter().enumerate() {
        let p = pose.bones.get(i).copied().unwrap_or_default();
        let local = Mat4::from_trs(b.rest_t + p.pos, b.rest_r.mul(p.rot).normalized(), b.rest_s * p.scale);
        w.push(match b.parent {
            Some(par) if par < i => w[par] * local,
            _ => local,
        });
    }
    w
}

/// Skinning matrices = world * inverse_bind.
pub fn skin_matrices(skel: &Skeleton, pose: &Pose) -> Vec<Mat4> {
    let (w, ib) = (world_matrices(skel, pose), skel.inverse_bind());
    w.iter().zip(ib.iter()).map(|(a, b)| *a * *b).collect()
}

/// Applies a pose to the vertices of a mesh. Returns (positions, normals) in model space.
pub fn skin_mesh(mesh: &Mesh, skin: &[Mat4]) -> (Vec<Vec3>, Vec<Vec3>) {
    let mut ps = Vec::with_capacity(mesh.verts.len());
    let mut ns = Vec::with_capacity(mesh.verts.len());
    for v in &mesh.verts {
        let total: f32 = v.weights.iter().sum();
        if total < 1e-6 {
            ps.push(v.pos);
            ns.push(v.nrm);
            continue;
        }
        let (mut p, mut n) = (Vec3::ZERO, Vec3::ZERO);
        for k in 0..4 {
            let w = v.weights[k] / total;
            if w <= 0.0 {
                continue;
            }
            if let Some(m) = skin.get(v.joints[k] as usize) {
                p += m.transform_point(v.pos) * w;
                n += m.transform_dir(v.nrm) * w;
            }
        }
        ps.push(p);
        ns.push(n.normalized());
    }
    (ps, ns)
}

/// Segments (parent to bone) used to draw the skeleton. The end is the bone position, the start is the parent position.
pub fn bone_segments(skel: &Skeleton, world: &[Mat4]) -> Vec<(Vec3, Vec3)> {
    skel.bones
        .iter()
        .enumerate()
        .filter_map(|(i, b)| {
            let head = b.parent.and_then(|p| world.get(p)).map(|m| m.transform_point(Vec3::ZERO))?;
            Some((head, world.get(i)?.transform_point(Vec3::ZERO)))
        })
        .collect()
}

/// Simple two-bone inverse kinematics (hip-knee-foot, shoulder-elbow-hand) in the plane of the target.
/// Returns the rotations (around local Z) for the upper and lower bone. Enough to place a hand or a foot.
pub fn two_bone_ik_angles(l1: f32, l2: f32, target_dist: f32) -> (f32, f32) {
    let d = target_dist.clamp((l1 - l2).abs() + 1e-4, l1 + l2 - 1e-4);
    // law of cosines
    let a1 = ((l1 * l1 + d * d - l2 * l2) / (2.0 * l1 * d)).clamp(-1.0, 1.0).acos();
    let a2 = ((l1 * l1 + l2 * l2 - d * d) / (2.0 * l1 * l2)).clamp(-1.0, 1.0).acos();
    (a1, std::f32::consts::PI - a2)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn arm() -> Skeleton {
        let b = |n: &str, p: Option<usize>, t: Vec3| Bone { name: n.into(), parent: p, rest_t: t, rest_r: Quat::IDENTITY, rest_s: Vec3::ONE };
        Skeleton { bones: vec![b("root", None, Vec3::ZERO), b("hand_L", Some(0), Vec3::new(1.0, 0.0, 0.0)), b("hand_R", Some(0), Vec3::new(-1.0, 0.0, 0.0))] }
    }
    #[test]
    fn rest_pose_is_identity_skin() {
        let s = arm();
        let sk = skin_matrices(&s, &Pose::rest(3));
        let p = Vec3::new(0.3, 0.4, 0.5);
        for m in sk {
            assert!((m.transform_point(p) - p).length() < 1e-5);
        }
    }
    #[test]
    fn rotating_root_moves_child_and_skinned_vertex() {
        let s = arm();
        let mut pose = Pose::rest(3);
        pose.bones[0].rot = Quat::from_axis_angle(Vec3::new(0.0, 0.0, 1.0), std::f32::consts::FRAC_PI_2);
        let w = world_matrices(&s, &pose);
        let hand = w[1].transform_point(Vec3::ZERO);
        assert!((hand - Vec3::new(0.0, 1.0, 0.0)).length() < 1e-4);
        let mut mesh = Mesh::new("m");
        let mut v = Vertex::new(Vec3::new(1.0, 0.0, 0.0), Default::default());
        v.weights = [1.0, 0.0, 0.0, 0.0];
        mesh.verts.push(v);
        let (ps, _) = skin_mesh(&mesh, &skin_matrices(&s, &pose));
        assert!((ps[0] - Vec3::new(0.0, 1.0, 0.0)).length() < 1e-4);
    }
    #[test]
    fn mirror_swaps_sides() {
        let s = arm();
        let mut pose = Pose::rest(3);
        pose.bones[1].pos = Vec3::new(0.0, 0.5, 0.0);
        let m = pose.mirrored(&s);
        assert!((m.bones[2].pos.y - 0.5).abs() < 1e-6);
        assert!(m.bones[1].pos.y.abs() < 1e-6);
    }
    #[test]
    fn slerp_halfway() {
        let a = Quat::IDENTITY;
        let b = Quat::from_axis_angle(Vec3::Y, 1.0);
        let h = slerp(a, b, 0.5);
        let e = Quat::from_axis_angle(Vec3::Y, 0.5);
        assert!((h.y - e.y).abs() < 1e-4 && (h.w - e.w).abs() < 1e-4);
    }
    #[test]
    fn ik_straight_and_folded() {
        let (a, b) = two_bone_ik_angles(1.0, 1.0, 2.0);
        assert!(a.abs() < 0.05 && b.abs() < 0.05);
        let (_, b2) = two_bone_ik_angles(1.0, 1.0, 1.0);
        assert!(b2 > 1.0);
    }
}
