//! Vector icons drawn from lines and shapes: no image files or fonts, and crisp at any size.
use eframe::egui::{pos2, vec2, Color32, Painter, Pos2, Shape};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Cube, Brush, Eraser, Fill, Picker, Line, Rect, RectFilled, Bone, Camera, Film, Play, Pause, StepBack, StepFwd,
    Undo, Redo, Open, Save, Plus, Minus, Trash, Copy, Mirror, Eye, EyeOff, Scissors, Wand, Export, Move, Sculpt,
    Select, Hand, Lock, Palette, Sphere, Cylinder, Cone, Torus, Plane, Loop, Image, Warn, Search, Gear, Rotate, Scale, Butterfly,
}


/// Draws `icon` inside a square of side `s` centered on `c`.
pub fn draw(p: &Painter, icon: Icon, c: Pos2, s: f32, col: Color32) {
    let u = s / 24.0; // one unit: icons are designed on a 24×24 grid
    let w = (1.8 * u).max(1.2);
    let st = crate::compat::stroke(w, col);
    let pt = |x: f32, y: f32| pos2(c.x + x * u, c.y + y * u);
    let line = |a: Pos2, b: Pos2| {
        p.line_segment([a, b], st);
    };
    let poly = |pts: &[Pos2], fill: Color32| p.add(Shape::convex_polygon(pts.to_vec(), fill, st));
    let open = |pts: &[Pos2]| p.add(Shape::line(pts.to_vec(), st));
    let none = Color32::TRANSPARENT;
    match icon {
        Icon::Cube => {
            poly(&[pt(0., -9.), pt(8., -4.5), pt(8., 4.5), pt(0., 9.), pt(-8., 4.5), pt(-8., -4.5)], none);
            line(pt(0., 0.), pt(0., 9.));
            line(pt(0., 0.), pt(8., -4.5));
            line(pt(0., 0.), pt(-8., -4.5));
        }
        Icon::Brush => {
            line(pt(7., -8.), pt(-3., 2.));
            p.circle_filled(pt(-5., 5.), 4.0 * u, col);
            line(pt(5., -9.), pt(9., -5.));
        }
        Icon::Eraser => {
            poly(&[pt(-9., 3.), pt(-1., -6.), pt(8., 1.), pt(1., 8.), pt(-4., 8.)], none);
            line(pt(-5., -1.), pt(4., 6.));
        }
        Icon::Fill => {
            poly(&[pt(-7., 0.), pt(0., -7.), pt(7., 0.), pt(0., 7.)], none);
            p.circle_filled(pt(8., 6.), 2.5 * u, col);
            line(pt(-7., 0.), pt(7., 0.));
        }
        Icon::Picker => {
            line(pt(8., -8.), pt(-2., 2.));
            line(pt(-2., 2.), pt(-7., 7.));
            p.circle_stroke(pt(7., -7.), 3.0 * u, st);
            p.circle_filled(pt(-7., 7.), 1.8 * u, col);
        }
        Icon::Line => {
            line(pt(-8., 8.), pt(8., -8.));
            p.circle_filled(pt(-8., 8.), 2.0 * u, col);
            p.circle_filled(pt(8., -8.), 2.0 * u, col);
        }
        Icon::Rect => {
            poly(&[pt(-8., -6.), pt(8., -6.), pt(8., 6.), pt(-8., 6.)], none);
        }
        Icon::RectFilled => {
            poly(&[pt(-8., -6.), pt(8., -6.), pt(8., 6.), pt(-8., 6.)], col);
        }
        Icon::Bone => {
            line(pt(-6., 6.), pt(6., -6.));
            p.circle_filled(pt(-7., 7.), 3.0 * u, col);
            p.circle_filled(pt(7., -7.), 3.0 * u, col);
        }
        Icon::Camera => {
            poly(&[pt(-9., -4.), pt(5., -4.), pt(5., 6.), pt(-9., 6.)], none);
            poly(&[pt(5., -1.), pt(10., -4.), pt(10., 7.), pt(5., 4.)], none);
        }
        Icon::Film => {
            poly(&[pt(-8., -8.), pt(8., -8.), pt(8., 8.), pt(-8., 8.)], none);
            for y in [-5., 0., 5.] {
                line(pt(-8., y), pt(-5., y));
                line(pt(5., y), pt(8., y));
            }
        }
        Icon::Play => {
            poly(&[pt(-5., -8.), pt(8., 0.), pt(-5., 8.)], col);
        }
        Icon::Pause => {
            poly(&[pt(-6., -7.), pt(-2., -7.), pt(-2., 7.), pt(-6., 7.)], col);
            poly(&[pt(2., -7.), pt(6., -7.), pt(6., 7.), pt(2., 7.)], col);
        }
        Icon::StepBack => {
            poly(&[pt(7., -7.), pt(-3., 0.), pt(7., 7.)], col);
            line(pt(-7., -7.), pt(-7., 7.));
        }
        Icon::StepFwd => {
            poly(&[pt(-7., -7.), pt(3., 0.), pt(-7., 7.)], col);
            line(pt(7., -7.), pt(7., 7.));
        }
        Icon::Undo => {
            open(&[pt(8., 6.), pt(8., -1.), pt(2., -6.), pt(-4., -4.), pt(-7., 0.)]);
            poly(&[pt(-9., -4.), pt(-9., 3.), pt(-3., 0.)], col);
        }
        Icon::Redo => {
            open(&[pt(-8., 6.), pt(-8., -1.), pt(-2., -6.), pt(4., -4.), pt(7., 0.)]);
            poly(&[pt(9., -4.), pt(9., 3.), pt(3., 0.)], col);
        }
        Icon::Open => {
            open(&[pt(-9., 7.), pt(-9., -6.), pt(-3., -6.), pt(-1., -3.), pt(8., -3.), pt(8., 7.), pt(-9., 7.)]);
        }
        Icon::Save => {
            poly(&[pt(-8., -8.), pt(6., -8.), pt(8., -6.), pt(8., 8.), pt(-8., 8.)], none);
            poly(&[pt(-4., -8.), pt(3., -8.), pt(3., -3.), pt(-4., -3.)], none);
            poly(&[pt(-5., 2.), pt(5., 2.), pt(5., 8.), pt(-5., 8.)], none);
        }
        Icon::Plus => {
            line(pt(-8., 0.), pt(8., 0.));
            line(pt(0., -8.), pt(0., 8.));
        }
        Icon::Minus => {
            line(pt(-8., 0.), pt(8., 0.));
        }
        Icon::Trash => {
            open(&[pt(-6., -4.), pt(-5., 8.), pt(5., 8.), pt(6., -4.)]);
            line(pt(-8., -5.), pt(8., -5.));
            line(pt(-2., -5.), pt(-2., -8.));
            line(pt(2., -5.), pt(2., -8.));
            line(pt(-2., -8.), pt(2., -8.));
        }
        Icon::Copy => {
            poly(&[pt(-8., -3.), pt(3., -3.), pt(3., 8.), pt(-8., 8.)], none);
            open(&[pt(-3., -3.), pt(-3., -8.), pt(8., -8.), pt(8., 3.), pt(3., 3.)]);
        }
        Icon::Mirror => {
            line(pt(0., -9.), pt(0., 9.));
            poly(&[pt(-2., -6.), pt(-9., 6.), pt(-2., 6.)], none);
            poly(&[pt(2., -6.), pt(9., 6.), pt(2., 6.)], col);
        }
        Icon::Eye => {
            open(&[pt(-10., 0.), pt(-5., -5.), pt(0., -6.), pt(5., -5.), pt(10., 0.), pt(5., 5.), pt(0., 6.), pt(-5., 5.), pt(-10., 0.)]);
            p.circle_filled(pt(0., 0.), 2.6 * u, col);
        }
        Icon::Lock => {
            poly(&[pt(-6., -1.), pt(6., -1.), pt(6., 8.), pt(-6., 8.)], none);
            open(&[pt(-3.5, -1.), pt(-3.5, -4.), pt(-2., -7.), pt(2., -7.), pt(3.5, -4.), pt(3.5, -1.)]);
        }
        Icon::EyeOff => {
            open(&[pt(-10., 0.), pt(-5., -5.), pt(0., -6.), pt(5., -5.), pt(10., 0.), pt(5., 5.), pt(0., 6.), pt(-5., 5.), pt(-10., 0.)]);
            line(pt(-8., 8.), pt(8., -8.));
        }
        Icon::Scissors => {
            line(pt(-6., -8.), pt(7., 8.));
            line(pt(6., -8.), pt(-7., 8.));
            p.circle_stroke(pt(-6., 7.), 2.5 * u, st);
            p.circle_stroke(pt(6., 7.), 2.5 * u, st);
        }
        Icon::Wand => {
            line(pt(-8., 8.), pt(3., -3.));
            for (dx, dy) in [(6., -6.), (6., 0.), (0., -8.)] {
                line(pt(dx - 2., dy), pt(dx + 2., dy));
                line(pt(dx, dy - 2.), pt(dx, dy + 2.));
            }
        }
        Icon::Export => {
            open(&[pt(-8., 2.), pt(-8., 8.), pt(8., 8.), pt(8., 2.)]);
            line(pt(0., -8.), pt(0., 4.));
            open(&[pt(-4., -4.), pt(0., -8.), pt(4., -4.)]);
        }
        Icon::Move => {
            line(pt(-9., 0.), pt(9., 0.));
            line(pt(0., -9.), pt(0., 9.));
            for (x, y, dx, dy) in [(9., 0., -3., 3.), (-9., 0., 3., 3.), (0., -9., 3., 3.), (0., 9., 3., -3.)] {
                line(pt(x, y), pt(x + dx, y + dy));
                line(pt(x, y), pt(x + if dx != 0.0 && y == 0.0 { dx } else { -dx }, y - dy));
            }
        }
        Icon::Sculpt => {
            open(&[pt(-9., 6.), pt(-4., 6.), pt(-1., -6.), pt(2., 6.), pt(9., 6.)]);
            p.circle_filled(pt(-1., -6.), 2.0 * u, col);
        }
        Icon::Select => {
            poly(&[pt(-6., -8.), pt(-6., 6.), pt(-2., 3.), pt(1., 9.), pt(4., 8.), pt(1., 2.), pt(6., 2.)], none);
        }
        Icon::Hand => {
            for x in [-6., -2., 2., 6.] {
                line(pt(x, -6.), pt(x, 2.));
            }
            open(&[pt(-6., 2.), pt(-6., 6.), pt(-2., 9.), pt(5., 9.), pt(8., 5.), pt(8., 0.)]);
        }
        Icon::Palette => {
            p.circle_stroke(pt(0., 0.), 9.0 * u, st);
            for (x, y) in [(-4., -3.), (0., -5.), (4., -3.), (-4., 3.)] {
                p.circle_filled(pt(x, y), 1.6 * u, col);
            }
        }
        Icon::Sphere => {
            p.circle_stroke(pt(0., 0.), 8.0 * u, st);
            open(&[pt(-8., 0.), pt(-4., 3.), pt(4., 3.), pt(8., 0.)]);
            open(&[pt(0., -8.), pt(-3., 0.), pt(0., 8.)]);
        }
        Icon::Cylinder => {
            open(&[pt(-7., -6.), pt(-7., 6.), pt(0., 9.), pt(7., 6.), pt(7., -6.)]);
            poly(&[pt(-7., -6.), pt(0., -9.), pt(7., -6.), pt(0., -3.)], none);
        }
        Icon::Cone => {
            poly(&[pt(0., -9.), pt(8., 7.), pt(0., 9.), pt(-8., 7.)], none);
        }
        Icon::Torus => {
            p.circle_stroke(pt(0., 0.), 9.0 * u, st);
            p.circle_stroke(pt(0., 0.), 3.5 * u, st);
        }
        Icon::Butterfly => {
            // an original drawing: two pairs of wings, a body and two feelers
            for s in [1.0f32, -1.0] {
                poly(&[pt(s * 1.5, -1.0), pt(s * 9.0, -8.0), pt(s * 10.0, -1.5), pt(s * 1.5, 1.5)], none);
                poly(&[pt(s * 1.5, 2.0), pt(s * 8.0, 3.0), pt(s * 6.0, 9.0), pt(s * 1.5, 6.0)], none);
                line(pt(0.0, -5.0), pt(s * 3.5, -9.5));
            }
            line(pt(0.0, -6.0), pt(0.0, 9.0));
        }
        Icon::Plane => {
            poly(&[pt(-9., 4.), pt(-3., -5.), pt(9., -5.), pt(3., 4.)], none);
        }
        Icon::Loop => {
            open(&[pt(-7., 2.), pt(-7., -3.), pt(-3., -6.), pt(5., -6.), pt(8., -3.)]);
            open(&[pt(7., -2.), pt(7., 3.), pt(3., 6.), pt(-5., 6.), pt(-8., 3.)]);
            poly(&[pt(5., -9.), pt(10., -3.), pt(4., -3.)], col);
        }
        Icon::Image => {
            poly(&[pt(-9., -7.), pt(9., -7.), pt(9., 7.), pt(-9., 7.)], none);
            open(&[pt(-9., 5.), pt(-3., -1.), pt(1., 3.), pt(4., 0.), pt(9., 5.)]);
            p.circle_filled(pt(4., -3.), 1.6 * u, col);
        }
        Icon::Warn => {
            poly(&[pt(0., -9.), pt(9., 7.), pt(-9., 7.)], none);
            line(pt(0., -3.), pt(0., 2.));
            p.circle_filled(pt(0., 5.), 1.2 * u, col);
        }
        Icon::Search => {
            p.circle_stroke(pt(-2., -2.), 6.5 * u, st);
            line(pt(2.8, 2.8), pt(9., 9.));
        }
        Icon::Rotate => {
            let pts: Vec<Pos2> = (0..=18).map(|i| {
                let a = -0.6 + i as f32 / 18.0 * 4.6;
                pt(a.cos() * 8.0, a.sin() * 8.0)
            }).collect();
            open(&pts);
            let e = *pts.last().unwrap();
            line(e, e + vec2(-5.0 * u, -1.0 * u));
            line(e, e + vec2(1.0 * u, -5.5 * u));
        }
        Icon::Scale => {
            poly(&[pt(-9., -1.), pt(-1., -1.), pt(-1., 9.), pt(-9., 9.)], none);
            poly(&[pt(1., -9.), pt(9., -9.), pt(9., -1.), pt(1., -1.)], none);
            line(pt(-1., 1.), pt(5., -5.));
        }
        Icon::Gear => {
            p.circle_stroke(pt(0., 0.), 4.0 * u, st);
            for k in 0..8 {
                let a = k as f32 * std::f32::consts::FRAC_PI_4;
                let (sn, cs) = a.sin_cos();
                line(pt(cs * 7.0, sn * 7.0), pt(cs * 10.0, sn * 10.0));
            }
            p.circle_stroke(pt(0., 0.), 7.0 * u, st);
        }
    }
}
