//! Inverse kinematics for posing: drag the end of a limb and the bones above it follow.
//!
//! Cyclic coordinate descent (CCD): starting at the bone next to the end, every bone of the chain
//! is turned so that the end points at the target, and the pass repeats a few times. It needs no
//! set-up (no IK constraints to rig), handles any chain length and keeps the bone lengths.
use crate::math::*;
use crate::mesh::Skeleton;
use crate::pose::{world_matrices, Pose};

/// World (model-space) rotation of every bone, ignoring scale.
fn world_rots(skel: &Skeleton, pose: &Pose) -> Vec<Quat> {
    let mut out: Vec<Quat> = Vec::with_capacity(skel.bones.len());
    for (i, b) in skel.bones.iter().enumerate() {
        let p = pose.bones.get(i).copied().unwrap_or_default();
        let local = b.rest_r.mul(p.rot).normalized();
        out.push(match b.parent {
            Some(par) if par < i => out[par].mul(local).normalized(),
            _ => local,
        });
    }
    out
}

fn conj(q: Quat) -> Quat {
    Quat { x: -q.x, y: -q.y, z: -q.z, w: q.w }
}

/// The joint (origin) of a bone in model space.
pub fn joint(skel: &Skeleton, pose: &Pose, bone: usize) -> Vec3 {
    world_matrices(skel, pose).get(bone).map_or(Vec3::ZERO, |m| m.transform_point(Vec3::ZERO))
}

/// The bones that move when `end` is dragged: its parent, grandparent… up to `chain` bones.
pub fn chain_of(skel: &Skeleton, end: usize, chain: usize) -> Vec<usize> {
    let mut out = vec![];
    let mut cur = skel.bones.get(end).and_then(|b| b.parent);
    while let Some(p) = cur {
        if out.len() >= chain {
            break;
        }
        out.push(p);
        cur = skel.bones[p].parent;
    }
    out
}

/// Moves the joint of bone `end` towards `target` (model space) by rotating up to `chain` bones
/// above it. Returns the remaining distance between the joint and the target.
pub fn solve(skel: &Skeleton, pose: &mut Pose, end: usize, chain: usize, target: Vec3, iterations: usize) -> f32 {
    if end >= skel.bones.len() {
        return f32::MAX;
    }
    pose.fit(skel.bones.len());
    let bones = chain_of(skel, end, chain.max(1));
    if bones.is_empty() {
        return (joint(skel, pose, end) - target).length();
    }
    for _ in 0..iterations.max(1) {
        for &a in &bones {
            let w = world_matrices(skel, pose);
            let pa = w[a].transform_point(Vec3::ZERO);
            let pe = w[end].transform_point(Vec3::ZERO);
            if (pe - target).length() < 1e-5 {
                return 0.0;
            }
            let (u, v) = ((pe - pa).normalized(), (target - pa).normalized());
            if u == Vec3::ZERO || v == Vec3::ZERO {
                continue;
            }
            let axis = u.cross(v);
            let s = axis.length();
            let c = u.dot(v).clamp(-1.0, 1.0);
            if s < 1e-6 {
                continue;
            }
            // limit each step a little so the chain does not snap around
            let angle = s.atan2(c).min(1.2);
            // rotation about a world axis, expressed in the frame the bone's own rotation lives in
            let rots = world_rots(skel, pose);
            let b = &skel.bones[a];
            let parent = match b.parent {
                Some(p) if p < a => rots[p],
                _ => Quat::IDENTITY,
            };
            let frame = parent.mul(b.rest_r).normalized();
            let axis_local = conj(frame).rotate(axis / s);
            let r = Quat::from_axis_angle(axis_local, angle);
            pose.bones[a].rot = r.mul(pose.bones[a].rot).normalized();
        }
    }
    (joint(skel, pose, end) - target).length()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::Bone;

    fn arm() -> Skeleton {
        // three bones along +X, one unit each
        let mk = |name: &str, parent: Option<usize>, x: f32| Bone { name: name.into(), parent, rest_t: Vec3::new(x, 0.0, 0.0), rest_r: Quat::IDENTITY, rest_s: Vec3::ONE };
        Skeleton { bones: vec![mk("shoulder", None, 0.0), mk("elbow", Some(0), 1.0), mk("wrist", Some(1), 1.0)] }
    }

    #[test]
    fn reaches_a_target_within_range() {
        let skel = arm();
        let mut pose = Pose::rest(3);
        let target = Vec3::new(1.0, 1.2, 0.3);
        let d = solve(&skel, &mut pose, 2, 2, target, 30);
        assert!(d < 1e-3, "distance {d}");
        // bone lengths are kept
        let (p0, p1, p2) = (joint(&skel, &pose, 0), joint(&skel, &pose, 1), joint(&skel, &pose, 2));
        assert!(((p1 - p0).length() - 1.0).abs() < 1e-4);
        assert!(((p2 - p1).length() - 1.0).abs() < 1e-4);
    }

    #[test]
    fn out_of_reach_points_the_chain_at_the_target() {
        let skel = arm();
        let mut pose = Pose::rest(3);
        let d = solve(&skel, &mut pose, 2, 2, Vec3::new(0.0, 5.0, 0.0), 30);
        assert!((d - 3.0).abs() < 1e-2, "distance {d}");
        assert_eq!(chain_of(&skel, 2, 1), vec![1]);
    }
}
