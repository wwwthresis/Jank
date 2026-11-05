//! Frame composition: scene objects + poses + emotions + the hand-drawn layer become pixels.
use crate::anim::*;
use crate::math::*;
use crate::pose::Pose;
use crate::project::*;
use crate::raster::*;
use crate::texture::Texture;
use std::borrow::Cow;

/// The textures of a model with the frame's emotion and one-off repaints applied. With no replacements this is a plain borrowed reference.
pub fn resolve_textures<'a>(asset: &'a Asset, st: &ObjectState) -> Cow<'a, [Texture]> {
    let emotion = st.emotion.and_then(|i| asset.emotions.get(i));
    let patches: Vec<&(usize, Texture)> = st.tex_patch.iter().filter(|(i, _)| *i < asset.model.textures.len()).collect();
    if emotion.is_none() && patches.is_empty() {
        return Cow::Borrowed(&asset.model.textures);
    }
    let mut t = asset.model.textures.clone();
    if let Some(e) = emotion {
        if e.texture < t.len() {
            t[e.texture] = e.image.clone();
        }
    }
    for (i, img) in patches {
        t[*i] = img.clone(); // a frame repaint wins over the emotion
    }
    Cow::Owned(t)
}

/// Alpha-composites a drawing of any size (nearest-neighbour sampling) onto the frame.
pub fn overlay_hand_layer(frame: &mut Frame, layer: &Texture) {
    for y in 0..frame.h {
        let sy = (y as u64 * layer.h as u64 / frame.h as u64) as usize;
        for x in 0..frame.w {
            let sx = (x as u64 * layer.w as u64 / frame.w as u64) as usize;
            let s = layer.px[sy * layer.w as usize + sx];
            if s[3] == 0 {
                continue;
            }
            let a = s[3] as f32 / 255.0;
            let d = &mut frame.color[(y * frame.w + x) as usize];
            for c in 0..3 {
                d[c] = (d[c] as f32 * (1.0 - a) + s[c] as f32 * a).round() as u8;
            }
        }
    }
}

/// The id of an object in the pick buffer (`Hit::obj`). 0xFFFF is reserved for "nothing".
pub fn hit_object_id(object: u32) -> u16 {
    (object % 0xFFFF) as u16
}

pub struct FrameSpec<'a> {
    pub states: &'a [ObjectState],
    pub camera: Camera,
    pub style: &'a PsxStyle,
    pub hand_layer: Option<&'a Texture>,
    pub hand_only: bool,
    /// Draw the scene layers (pictures behind and in front of the 3D objects). Off for a clean 3D reference.
    pub scene_layers: bool,
    /// Time in seconds (for flickering lamps).
    pub time: f32,
}

/// Base render: draws the object states into `r.frame`.
pub fn render_states(project: &Project, spec: &FrameSpec, r: &mut Renderer) {
    // own the skinning data and textures for the duration of the render; `Instance` only borrows them
    struct Owned<'a> {
        asset: &'a Asset,
        pos: Vec<Vec<Vec3>>,
        nrm: Vec<Vec<Vec3>>,
        tex: Cow<'a, [Texture]>,
        world: Mat4,
        id: u16,
        tint: [f32; 3],
    }
    let mut owned: Vec<Owned> = Vec::new();
    if !spec.hand_only {
        for st in spec.states.iter().filter(|s| s.visible) {
            let Some(obj) = project.scene.object(st.object) else { continue };
            if !obj.visible {
                continue;
            }
            let Some(asset) = project.assets.get(obj.asset) else { continue };
            let pose: Option<&Pose> = (!st.pose.bones.is_empty()).then_some(&st.pose);
            let (pos, nrm) = skin_model(&asset.model, pose);
            owned.push(Owned { asset, pos, nrm, tex: resolve_textures(asset, st), world: st.transform.matrix(), id: hit_object_id(st.object), tint: obj.tint });
        }
    }
    // time of day (when on) decides the light and the sky
    let style = project.scene.lighting.apply(spec.style);
    let style: &PsxStyle = &style;
    let lit = &project.scene.lighting;
    let (mode, quality) = match r.shadow_cap {
        Some(cap) => cap.limit(lit.shadows, lit.quality),
        None => (lit.shadows, lit.quality),
    };
    // shadows: a map of the scene as the light sees it, applied by the renderer to what it draws
    r.shadow = if mode == crate::lighting::ShadowMode::Off || lit.shadow_strength <= 0.0 {
        None
    } else {
        let casters: Vec<crate::shadow::Caster> = owned.iter().map(|o| crate::shadow::Caster { model: &o.asset.model, pos: &o.pos, world: o.world }).collect();
        crate::shadow::ShadowMap::build_cached(&casters, style.light_dir, mode == crate::lighting::ShadowMode::Blob, quality, lit.softness).map(|map| crate::shadow::ShadowInput { map, strength: lit.shadow_strength })
    };
    // lamps: attached ones follow their object as it stands in this frame
    r.lights = if spec.hand_only || project.scene.lights.is_empty() {
        Vec::new()
    } else {
        let world_of = |id: u32| -> Option<Mat4> {
            let obj = project.scene.object(id).filter(|o| o.visible)?;
            match spec.states.iter().find(|s| s.object == id) {
                Some(st) if st.visible => Some(st.transform.matrix()),
                Some(_) => None,
                None => Some(obj.transform.matrix()),
            }
        };
        crate::lights::resolve(&project.scene.lights, &world_of, spec.time)
    };
    let insts: Vec<Instance> = owned.iter().map(|o| Instance { model: &o.asset.model, pos: &o.pos, nrm: &o.nrm, world: o.world, textures: &o.tex, id: o.id, tint: o.tint }).collect();
    // scene layers: pictures behind and in front of the 3D objects
    fn mk(l: &crate::layers::Layer) -> Plane<'_> {
        Plane { w: l.img.w, h: l.img.h, px: &l.img.px, opacity: l.opacity }
    }
    let sc = &project.scene;
    let split = sc.layers_back.min(sc.layers.len());
    let show = |l: &&crate::layers::Layer| spec.scene_layers && l.shows();
    let back: Vec<Plane> = sc.layers[..split].iter().filter(show).map(mk).collect();
    let front: Vec<Plane> = sc.layers[split..].iter().filter(show).map(mk).collect();
    r.render_layered(style, &spec.camera, &insts, &back, &front);
    if let Some(layer) = spec.hand_layer {
        overlay_hand_layer(&mut r.frame, layer);
    }
}

/// Renders an animation frame by index (without tweening), for editing a specific frame.
pub fn render_frame_index(project: &Project, anim: &Animation, frame: usize, r: &mut Renderer) -> bool {
    let Some(f) = anim.frames.get(frame) else { return false };
    let style = f.style.as_ref().unwrap_or(&project.scene.style);
    let time = anim.frame_start(frame) as f32 / anim.fps().max(1e-3);
    render_states(project, &FrameSpec { states: &f.states, camera: f.camera, style, hand_layer: f.hand_layer.as_ref(), hand_only: f.hand_only, scene_layers: true, time }, r);
    true
}

/// Renders a playback tick (with tweening and holds).
pub fn render_tick(project: &Project, anim: &Animation, tick: u32, r: &mut Renderer) -> bool {
    let Some(ev) = anim.eval(tick) else { return false };
    let f = &anim.frames[ev.frame_index];
    let style = f.style.as_ref().unwrap_or(&project.scene.style);
    let time = tick as f32 / anim.fps().max(1e-3);
    render_states(project, &FrameSpec { states: &ev.states, camera: ev.camera, style, hand_layer: f.hand_layer.as_ref(), hand_only: f.hand_only, scene_layers: true, time }, r);
    true
}

/// The state used for live scene editing without animation: the objects' base transforms and the rest pose.
pub fn scene_states(project: &Project) -> Vec<ObjectState> {
    project.scene.objects.iter().map(|o| ObjectState::base(o.id, o.transform.clone(), project.bones_of(o.asset))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modeling::*;

    fn project() -> Project {
        let mut p = Project::new("t");
        let model = model_from_mesh("cube", cube(Vec3::new(2.5, 2.5, 2.5)), 16);
        p.place_model(model);
        p.scene.style = PsxStyle { width: 64, height: 64, ..PsxStyle::ps1_classic() };
        p.scene.camera = Camera { eye: Vec3::new(2.0, 1.5, 3.0), target: Vec3::ZERO, ..Default::default() };
        p
    }

    #[test]
    fn renders_scene_objects() {
        let p = project();
        let mut r = Renderer::new(64, 64);
        render_states(&p, &FrameSpec { states: &scene_states(&p), camera: p.scene.camera, style: &p.scene.style, hand_layer: None, hand_only: false, scene_layers: true, time: 0.0 }, &mut r);
        assert!(r.frame.any_hit(), "the cube must be visible");
    }
    fn lamp_scene(style: PsxStyle) -> Project {
        let mut p = Project::new("lamps");
        let floor = model_from_mesh("floor", plane(6.0, 6.0, 6, 6), 16);
        p.place_model(floor);
        let id = p.place_model(model_from_mesh("box", cube(Vec3::new(0.6, 0.6, 0.6)), 16));
        p.scene.object_mut(id).unwrap().transform.pos = Vec3::new(0.6, 0.3, 0.0);
        p.scene.style = PsxStyle { width: 96, height: 64, ambient: [0.1, 0.1, 0.1], light_color: [0.1, 0.1, 0.1], fog_on: false, ..style };
        p.scene.camera = Camera { eye: Vec3::new(0.0, 3.0, 4.0), target: Vec3::ZERO, ..Default::default() };
        p
    }
    fn picture(p: &Project, threads: usize) -> Vec<[u8; 4]> {
        let mut r = Renderer::new(1, 1);
        r.threads = threads;
        render_states(p, &FrameSpec { states: &scene_states(p), camera: p.scene.camera, style: &p.scene.style, hand_layer: None, hand_only: false, scene_layers: true, time: 0.5 }, &mut r);
        r.frame.color.clone()
    }
    fn brightness(px: &[[u8; 4]]) -> u64 {
        px.iter().map(|c| c[0] as u64 + c[1] as u64 + c[2] as u64).sum()
    }
    #[test]
    fn lamps_light_the_scene_per_vertex_and_per_pixel() {
        for pixel in [false, true] {
            let mut p = lamp_scene(PsxStyle { pixel_lights: pixel, ..PsxStyle::ps1_classic() });
            let dark = brightness(&picture(&p, 1));
            let l = p.scene.add_light(crate::lights::LightPreset::Bulb);
            p.scene.light_mut(l).unwrap().pos = Vec3::new(-0.5, 0.8, 0.5);
            let lit = brightness(&picture(&p, 1));
            assert!(lit > dark + dark / 5, "pixel={pixel}: a lamp brightens the floor ({dark} -> {lit})");
            p.scene.light_mut(l).unwrap().enabled = false;
            assert_eq!(brightness(&picture(&p, 1)), dark, "a switched-off lamp changes nothing");
            p.scene.light_mut(l).unwrap().enabled = true;
            p.scene.light_mut(l).unwrap().pos = Vec3::new(40.0, 0.5, 0.0);
            assert_eq!(picture(&p, 1), picture(&lamp_scene(PsxStyle { pixel_lights: pixel, ..PsxStyle::ps1_classic() }), 1), "a lamp out of reach is culled and changes nothing");
        }
    }
    #[test]
    fn modern_effects_are_the_same_on_any_number_of_threads() {
        let mut p = lamp_scene(PsxStyle { width: 200, height: 160, crt_curve: 0.4, crt_mask: 0.3, chroma: 0.4, ..PsxStyle::night_glow() });
        p.scene.style.width = 200;
        p.scene.style.height = 160;
        for (i, preset) in [crate::lights::LightPreset::Lantern, crate::lights::LightPreset::Spotlight, crate::lights::LightPreset::Neon].into_iter().enumerate() {
            let l = p.scene.add_light(preset);
            p.scene.light_mut(l).unwrap().pos = Vec3::new(i as f32 - 1.0, 0.7, 0.4);
        }
        let one = picture(&p, 1);
        assert_ne!(brightness(&one), 0);
        for t in [2, 3, 5] {
            assert!(picture(&p, t) == one, "{t} threads give the same picture");
        }
    }
    #[test]
    fn attached_lamp_follows_its_object() {
        let mut p = lamp_scene(PsxStyle { pixel_lights: true, ..PsxStyle::ps1_classic() });
        let obj = p.scene.objects[1].id;
        let l = p.scene.add_light(crate::lights::LightPreset::Bulb);
        p.scene.light_mut(l).unwrap().pos = Vec3::new(0.0, 0.8, 0.0);
        p.scene.light_mut(l).unwrap().attach = Some(obj);
        let a = picture(&p, 1);
        p.scene.object_mut(obj).unwrap().transform.pos = Vec3::new(-1.5, 0.3, 0.0);
        assert_ne!(picture(&p, 1), a, "moving the object moves its lamp");
        p.scene.remove_object(obj);
        assert!(p.scene.light(l).unwrap().attach.is_none(), "deleting the object keeps the lamp, now free");
    }

    #[test]
    fn emotion_swaps_texture_and_patch_overrides_it() {
        let mut p = project();
        let blank = p.assets[0].model.textures[0].clone();
        let mut red = blank.clone();
        red.px.iter_mut().for_each(|c| *c = [255, 0, 0, 255]);
        p.assets[0].emotions.push(Emotion { name: "anger".into(), texture: 0, image: red.clone() });
        let mut st = scene_states(&p).remove(0);
        assert!(matches!(resolve_textures(&p.assets[0], &st), Cow::Borrowed(_)), "no replacement means no copy");
        st.emotion = Some(0);
        assert_eq!(resolve_textures(&p.assets[0], &st)[0].px[0], [255, 0, 0, 255]);
        let mut green = blank;
        green.px.iter_mut().for_each(|c| *c = [0, 255, 0, 255]);
        st.tex_patch.push((0, green));
        assert_eq!(resolve_textures(&p.assets[0], &st)[0].px[0], [0, 255, 0, 255], "a frame repaint wins over the emotion");
    }
    #[test]
    fn emotion_changes_rendered_pixels() {
        let mut p = project();
        let mut red = p.assets[0].model.textures[0].clone();
        red.px.iter_mut().for_each(|c| *c = [255, 0, 0, 255]);
        p.assets[0].emotions.push(Emotion { name: "e".into(), texture: 0, image: red });
        let mut anim = Animation::from_scene("a", &p.scene, &|a| p.bones_of(a));
        anim.duplicate_frame(0);
        anim.frames[1].states[0].emotion = Some(0);
        let (mut r0, mut r1) = (Renderer::new(64, 64), Renderer::new(64, 64));
        assert!(render_frame_index(&p, &anim, 0, &mut r0));
        assert!(render_frame_index(&p, &anim, 1, &mut r1));
        assert_ne!(r0.frame.color, r1.frame.color);
        assert!(!render_frame_index(&p, &anim, 9, &mut r0));
    }
    #[test]
    fn hand_layer_overlays_and_hand_only_hides_3d() {
        let p = project();
        let mut anim = Animation::from_scene("a", &p.scene, &|a| p.bones_of(a));
        let mut layer = Texture::new("h", 64, 64, [0, 0, 0, 0]);
        layer.rect(0, 0, 63, 63, [0, 0, 255, 255], false); // blue border
        anim.frames[0].hand_layer = Some(layer);
        let mut r = Renderer::new(64, 64);
        render_frame_index(&p, &anim, 0, &mut r);
        assert_eq!(r.frame.color[0][..3], [0, 0, 255], "the border is on top");
        assert!(r.frame.any_hit());
        anim.frames[0].hand_only = true;
        render_frame_index(&p, &anim, 0, &mut r);
        assert!(!r.frame.any_hit(), "in drawing mode the 3D image is hidden");
    }
    #[test]
    fn scene_layers_go_behind_and_in_front_of_the_model() {
        use crate::layers::Layer;
        let mut p = project();
        fn draw(p: &Project, r: &mut Renderer) {
            render_states(p, &FrameSpec { states: &scene_states(p), camera: p.scene.camera, style: &p.scene.style, hand_layer: None, hand_only: false, scene_layers: true, time: 0.0 }, r);
        }
        let mut plain = Renderer::new(64, 64);
        draw(&p, &mut plain);
        assert!(plain.frame.any_hit());
        // a full red layer behind the model: only pixels without a model change
        let mut red = Layer::new("sky", 32, 32); // a smaller layer is stretched to the render size
        red.img.px.iter_mut().for_each(|c| *c = [255, 0, 0, 255]);
        p.scene.layers.push(red);
        p.scene.layers_back = 1;
        let mut r = Renderer::new(64, 64);
        draw(&p, &mut r);
        let (mut bg_px, mut model_px) = (0, 0);
        for i in 0..64 * 64 {
            if plain.frame.tri_at[i] == NO_TRI {
                assert!(r.frame.color[i][0] > r.frame.color[i][1] + 100, "background pixel {i} must be red: {:?}", r.frame.color[i]);
                bg_px += 1;
            } else {
                assert_eq!(r.frame.color[i], plain.frame.color[i], "model pixel {i} must be untouched by a layer behind it");
                model_px += 1;
            }
        }
        assert!(bg_px > 100 && model_px > 100);
        // the same layer in front covers the model too
        p.scene.layers_back = 0;
        draw(&p, &mut r);
        assert!(r.frame.color.iter().all(|c| c[0] > c[1] + 100), "everything is red");
        assert!(r.frame.any_hit(), "layers do not hide the picking buffer");
        // hidden layers and layers with zero opacity do nothing
        p.scene.layers[0].visible = false;
        draw(&p, &mut r);
        assert_eq!(r.frame.color, plain.frame.color);
        p.scene.layers[0].visible = true;
        p.scene.layers[0].opacity = 0;
        draw(&p, &mut r);
        assert_eq!(r.frame.color, plain.frame.color);
    }
    #[test]
    fn shadows_fall_on_the_ground_wherever_it_is_and_never_on_the_lit_object() {
        use crate::lighting::ShadowMode;
        // the ground is at an arbitrary height (-0.7): shadows follow the surface, not a preset height
        let mut p = Project::new("t");
        let cube_id = p.place_model(model_from_mesh("cube", cube(Vec3::new(2.0, 2.0, 2.0)), 16));
        p.scene.objects[0].transform.pos = Vec3::new(0.0, 1.6, 0.0);
        let gid = p.place_model(model_from_mesh("ground", plane(14.0, 14.0, 6, 6), 16));
        p.scene.object_mut(gid).unwrap().transform.pos = Vec3::new(0.0, -0.7, 0.0);
        p.scene.style = PsxStyle { width: 128, height: 128, z_buffer: true, fog_on: false, dither: false, color_bits: 8, vignette: 0.0, scanlines: 0.0, grain: 0.0, ..PsxStyle::ps1_classic() };
        p.scene.style.light_dir = Vec3::new(-1.0, -0.8, 0.0).normalized(); // the shadow falls to the right of the cube, in plain view
        p.scene.camera = Camera { eye: Vec3::new(0.0, 7.0, 8.0), target: Vec3::new(1.0, 0.0, 0.0), ..Default::default() };
        let draw = |p: &Project| {
            let mut r = Renderer::new(128, 128);
            render_states(p, &FrameSpec { states: &scene_states(p), camera: p.scene.camera, style: &p.scene.style, hand_layer: None, hand_only: false, scene_layers: true, time: 0.0 }, &mut r);
            r
        };
        let plain = draw(&p);
        let lum = |c: [u8; 4]| c[0] as u32 + c[1] as u32 + c[2] as u32;
        let cube_obj = hit_object_id(cube_id);
        for mode in [ShadowMode::Blob, ShadowMode::Projected] {
            p.scene.lighting.shadows = mode;
            p.scene.lighting.shadow_strength = 0.6;
            let shaded = draw(&p);
            let (mut ground_darker, mut brighter, mut sky_changed, mut cube_px, mut cube_changed) = (0, 0, 0, 0, 0);
            for i in 0..128 * 128 {
                let (a, b) = (lum(plain.frame.color[i]), lum(shaded.frame.color[i]));
                let hit = plain.frame.hit_at((i % 128) as i32, (i / 128) as i32);
                if b > a {
                    brighter += 1;
                }
                if hit == Hit::NONE {
                    sky_changed += (b != a) as u32;
                } else if hit.obj == cube_obj {
                    cube_px += 1;
                    cube_changed += (b + 6 < a) as u32;
                } else if b < a {
                    ground_darker += 1;
                }
            }
            assert!(ground_darker > 150, "{mode:?}: the shadow must be visible on the ground, {ground_darker} pixels changed");
            assert_eq!((brighter, sky_changed), (0, 0), "{mode:?}");
            assert!(cube_changed * 50 < cube_px, "{mode:?}: the lit cube must not get speckled with its own shadow ({cube_changed} of {cube_px})");
        }
        // light travelling upwards: the ground is on the light's side of the cube, so nothing is shadowed
        p.scene.style.light_dir = Vec3::new(0.2, 1.0, 0.1).normalized();
        let with = draw(&p);
        p.scene.lighting.shadows = ShadowMode::Off;
        assert_eq!(with.frame.color, draw(&p).frame.color, "no shadow towards the light");
    }
    #[test]
    fn tick_playback_with_tween_moves_object() {
        let p = project();
        let mut anim = Animation::from_scene("a", &p.scene, &|a| p.bones_of(a));
        anim.frames[0].hold = 4;
        anim.frames[0].tween = true;
        anim.duplicate_frame(0);
        anim.frames[1].states[0].transform.pos = Vec3::new(0.8, 0.0, 0.0);
        anim.frames[1].tween = false;
        let (mut a, mut b) = (Renderer::new(64, 64), Renderer::new(64, 64));
        render_tick(&p, &anim, 0, &mut a);
        render_tick(&p, &anim, 2, &mut b);
        assert_ne!(a.frame.color, b.frame.color);
    }
}
