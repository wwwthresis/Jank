//! Minimal linear algebra: Vec2/Vec3/Quat/Mat4 (column-major).
use std::ops::{Add, AddAssign, Div, Mul, Neg, Sub};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}
impl Vec2 {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}
impl Add for Vec2 {
    type Output = Vec2;
    fn add(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x + o.x, self.y + o.y)
    }
}
impl Sub for Vec2 {
    type Output = Vec2;
    fn sub(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x - o.x, self.y - o.y)
    }
}
impl Mul<f32> for Vec2 {
    type Output = Vec2;
    fn mul(self, s: f32) -> Vec2 {
        Vec2::new(self.x * s, self.y * s)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}
impl Vec3 {
    pub const ZERO: Vec3 = Vec3::new(0.0, 0.0, 0.0);
    pub const ONE: Vec3 = Vec3::new(1.0, 1.0, 1.0);
    pub const Y: Vec3 = Vec3::new(0.0, 1.0, 0.0);
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }
    pub fn dot(self, o: Vec3) -> f32 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    pub fn cross(self, o: Vec3) -> Vec3 {
        Vec3::new(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }
    pub fn length(self) -> f32 {
        self.dot(self).sqrt()
    }
    pub fn normalized(self) -> Vec3 {
        let l = self.length();
        if l > 1e-12 {
            self / l
        } else {
            Vec3::ZERO
        }
    }
    pub fn lerp(self, o: Vec3, t: f32) -> Vec3 {
        self + (o - self) * t
    }
    pub fn min(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x.min(o.x), self.y.min(o.y), self.z.min(o.z))
    }
    pub fn max(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x.max(o.x), self.y.max(o.y), self.z.max(o.z))
    }
}
impl Add for Vec3 {
    type Output = Vec3;
    fn add(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}
impl AddAssign for Vec3 {
    fn add_assign(&mut self, o: Vec3) {
        *self = *self + o;
    }
}
impl Sub for Vec3 {
    type Output = Vec3;
    fn sub(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}
impl Mul<f32> for Vec3 {
    type Output = Vec3;
    fn mul(self, s: f32) -> Vec3 {
        Vec3::new(self.x * s, self.y * s, self.z * s)
    }
}
impl Mul<Vec3> for Vec3 {
    type Output = Vec3;
    fn mul(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x * o.x, self.y * o.y, self.z * o.z)
    }
}
impl Div<f32> for Vec3 {
    type Output = Vec3;
    fn div(self, s: f32) -> Vec3 {
        Vec3::new(self.x / s, self.y / s, self.z / s)
    }
}
impl Neg for Vec3 {
    type Output = Vec3;
    fn neg(self) -> Vec3 {
        Vec3::new(-self.x, -self.y, -self.z)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quat {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}
impl Default for Quat {
    fn default() -> Self {
        Quat::IDENTITY
    }
}
impl Quat {
    pub const IDENTITY: Quat = Quat { x: 0.0, y: 0.0, z: 0.0, w: 1.0 };
    pub fn from_axis_angle(axis: Vec3, angle: f32) -> Quat {
        let a = axis.normalized();
        let (s, c) = (angle * 0.5).sin_cos();
        Quat { x: a.x * s, y: a.y * s, z: a.z * s, w: c }
    }
    /// Euler angles in XYZ order (intrinsic), radians.
    pub fn from_euler(e: Vec3) -> Quat {
        let qx = Quat::from_axis_angle(Vec3::new(1.0, 0.0, 0.0), e.x);
        let qy = Quat::from_axis_angle(Vec3::new(0.0, 1.0, 0.0), e.y);
        let qz = Quat::from_axis_angle(Vec3::new(0.0, 0.0, 1.0), e.z);
        qz.mul(qy).mul(qx)
    }
    pub fn mul(self, o: Quat) -> Quat {
        Quat {
            w: self.w * o.w - self.x * o.x - self.y * o.y - self.z * o.z,
            x: self.w * o.x + self.x * o.w + self.y * o.z - self.z * o.y,
            y: self.w * o.y - self.x * o.z + self.y * o.w + self.z * o.x,
            z: self.w * o.z + self.x * o.y - self.y * o.x + self.z * o.w,
        }
    }
    pub fn normalized(self) -> Quat {
        let l = (self.x * self.x + self.y * self.y + self.z * self.z + self.w * self.w).sqrt();
        if l < 1e-12 {
            return Quat::IDENTITY;
        }
        Quat { x: self.x / l, y: self.y / l, z: self.z / l, w: self.w / l }
    }
    pub fn rotate(self, v: Vec3) -> Vec3 {
        let u = Vec3::new(self.x, self.y, self.z);
        let t = u.cross(v) * 2.0;
        v + t * self.w + u.cross(t)
    }
}

/// 4x4 matrix, column-major: m[col][row].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mat4 {
    pub m: [[f32; 4]; 4],
}
impl Default for Mat4 {
    fn default() -> Self {
        Mat4::IDENTITY
    }
}
impl Mat4 {
    pub const IDENTITY: Mat4 = Mat4 {
        m: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    };
    pub fn translation(t: Vec3) -> Mat4 {
        let mut r = Mat4::IDENTITY;
        r.m[3][0] = t.x;
        r.m[3][1] = t.y;
        r.m[3][2] = t.z;
        r
    }
    pub fn scale(s: Vec3) -> Mat4 {
        let mut r = Mat4::IDENTITY;
        r.m[0][0] = s.x;
        r.m[1][1] = s.y;
        r.m[2][2] = s.z;
        r
    }
    pub fn from_quat(q: Quat) -> Mat4 {
        let (x, y, z, w) = (q.x, q.y, q.z, q.w);
        let mut r = Mat4::IDENTITY;
        r.m[0][0] = 1.0 - 2.0 * (y * y + z * z);
        r.m[0][1] = 2.0 * (x * y + z * w);
        r.m[0][2] = 2.0 * (x * z - y * w);
        r.m[1][0] = 2.0 * (x * y - z * w);
        r.m[1][1] = 1.0 - 2.0 * (x * x + z * z);
        r.m[1][2] = 2.0 * (y * z + x * w);
        r.m[2][0] = 2.0 * (x * z + y * w);
        r.m[2][1] = 2.0 * (y * z - x * w);
        r.m[2][2] = 1.0 - 2.0 * (x * x + y * y);
        r
    }
    pub fn from_trs(t: Vec3, r: Quat, s: Vec3) -> Mat4 {
        Mat4::translation(t) * Mat4::from_quat(r) * Mat4::scale(s)
    }
    /// Right-handed camera (looks along -Z).
    pub fn look_at(eye: Vec3, target: Vec3, up: Vec3) -> Mat4 {
        let f = (target - eye).normalized();
        let s = f.cross(up).normalized();
        let u = s.cross(f);
        let mut r = Mat4::IDENTITY;
        r.m[0][0] = s.x;
        r.m[1][0] = s.y;
        r.m[2][0] = s.z;
        r.m[0][1] = u.x;
        r.m[1][1] = u.y;
        r.m[2][1] = u.z;
        r.m[0][2] = -f.x;
        r.m[1][2] = -f.y;
        r.m[2][2] = -f.z;
        r.m[3][0] = -s.dot(eye);
        r.m[3][1] = -u.dot(eye);
        r.m[3][2] = f.dot(eye);
        r
    }
    pub fn perspective(fov_y: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
        let f = 1.0 / (fov_y * 0.5).tan();
        let mut r = Mat4 { m: [[0.0; 4]; 4] };
        r.m[0][0] = f / aspect;
        r.m[1][1] = f;
        r.m[2][2] = (far + near) / (near - far);
        r.m[2][3] = -1.0;
        r.m[3][2] = 2.0 * far * near / (near - far);
        r
    }
    pub fn mul_vec4(&self, v: [f32; 4]) -> [f32; 4] {
        let mut o = [0.0; 4];
        for (r, out) in o.iter_mut().enumerate() {
            *out = self.m[0][r] * v[0] + self.m[1][r] * v[1] + self.m[2][r] * v[2] + self.m[3][r] * v[3];
        }
        o
    }
    pub fn transform_point(&self, p: Vec3) -> Vec3 {
        let o = self.mul_vec4([p.x, p.y, p.z, 1.0]);
        Vec3::new(o[0], o[1], o[2])
    }
    pub fn transform_dir(&self, d: Vec3) -> Vec3 {
        let o = self.mul_vec4([d.x, d.y, d.z, 0.0]);
        Vec3::new(o[0], o[1], o[2])
    }
    /// General matrix inverse (Gauss-Jordan elimination). `None` if the matrix is singular.
    pub fn inverse(&self) -> Option<Mat4> {
        let mut a = [[0f64; 8]; 4];
        for r in 0..4 {
            for c in 0..4 {
                a[r][c] = self.m[c][r] as f64;
            }
            a[r][4 + r] = 1.0;
        }
        for col in 0..4 {
            let mut piv = col;
            for r in col + 1..4 {
                if a[r][col].abs() > a[piv][col].abs() {
                    piv = r;
                }
            }
            if a[piv][col].abs() < 1e-12 {
                return None;
            }
            a.swap(col, piv);
            let d = a[col][col];
            for c in 0..8 {
                a[col][c] /= d;
            }
            for r in 0..4 {
                if r != col {
                    let f = a[r][col];
                    for c in 0..8 {
                        a[r][c] -= f * a[col][c];
                    }
                }
            }
        }
        let mut out = Mat4::IDENTITY;
        for r in 0..4 {
            for c in 0..4 {
                out.m[c][r] = a[r][4 + c] as f32;
            }
        }
        Some(out)
    }
}
impl Mul for Mat4 {
    type Output = Mat4;
    fn mul(self, o: Mat4) -> Mat4 {
        let mut r = Mat4 { m: [[0.0; 4]; 4] };
        for c in 0..4 {
            for row in 0..4 {
                let mut s = 0.0;
                for k in 0..4 {
                    s += self.m[k][row] * o.m[c][k];
                }
                r.m[c][row] = s;
            }
        }
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inverse_roundtrip() {
        let m = Mat4::from_trs(
            Vec3::new(1.0, 2.0, 3.0),
            Quat::from_euler(Vec3::new(0.3, 0.5, -0.2)),
            Vec3::new(2.0, 1.0, 0.5),
        );
        let i = m.inverse().unwrap();
        let p = Vec3::new(0.7, -1.2, 4.0);
        let q = i.transform_point(m.transform_point(p));
        assert!((p - q).length() < 1e-4);
    }
    #[test]
    fn quat_matches_matrix() {
        let q = Quat::from_euler(Vec3::new(0.4, 1.1, -0.7));
        let v = Vec3::new(1.0, 2.0, 3.0);
        let a = q.rotate(v);
        let b = Mat4::from_quat(q).transform_dir(v);
        assert!((a - b).length() < 1e-4);
    }
}

/// `x.floor() as i32` without the library call that baseline x86-64 needs for `floor`
/// (identical results, NaN and out-of-range values included).
#[inline(always)]
pub fn ifloor(x: f32) -> i32 {
    let t = x as i32;
    t.saturating_sub(((t as f32) > x) as i32)
}

#[cfg(test)]
mod ifloor_tests {
    use super::*;

    #[test]
    fn ifloor_matches_the_library() {
        let mut x = -70000.0f32;
        while x < 70000.0 {
            assert_eq!(ifloor(x), x.floor() as i32, "{x}");
            x += 0.37;
        }
        for x in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 3e9, -3e9, -0.0, -0.5, 0.5, -1e-30, 2147483520.0] {
            assert_eq!(ifloor(x), x.floor() as i32, "{x}");
        }
    }
}

