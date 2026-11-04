//! Layers: an ordered stack of images (bottom first) that is flattened into one picture.
//!
//! The same [`Layer`] is used in two places:
//! * texture layers: a stack that is flattened into the texture a model uses (see `Asset::layers`);
//! * scene layers: pictures drawn behind or in front of the 3D scene (see `Scene::layers`).
use crate::texture::{Rgba, Texture};

#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub name: String,
    pub visible: bool,
    /// A locked layer cannot be painted on.
    pub locked: bool,
    /// 0 = invisible, 255 = fully opaque.
    pub opacity: u8,
    pub img: Texture,
}

impl Layer {
    /// An empty (fully transparent) layer.
    pub fn new(name: &str, w: u32, h: u32) -> Layer {
        Layer { name: name.into(), visible: true, locked: false, opacity: 255, img: Texture::new(name, w, h, [0, 0, 0, 0]) }
    }
    pub fn from_texture(name: &str, img: Texture) -> Layer {
        Layer { name: name.into(), visible: true, locked: false, opacity: 255, img }
    }
    /// True if the layer contributes to the picture.
    pub fn shows(&self) -> bool {
        self.visible && self.opacity > 0
    }
}

/// Source-over blend of `src` (scaled by `opacity`) onto `dst`. Works for a destination with transparency too.
#[inline]
pub fn blend_over(dst: &mut Rgba, src: Rgba, opacity: u8) {
    let sa = (src[3] as u32 * opacity as u32 + 127) / 255;
    if sa == 0 {
        return;
    }
    if sa == 255 {
        *dst = [src[0], src[1], src[2], 255];
        return;
    }
    if dst[3] == 255 {
        for c in 0..3 {
            dst[c] = ((src[c] as u32 * sa + dst[c] as u32 * (255 - sa) + 127) / 255) as u8;
        }
        return;
    }
    let da = dst[3] as u32;
    let oa = sa * 255 + da * (255 - sa); // alpha scaled by 255
    if oa == 0 {
        return;
    }
    for c in 0..3 {
        let num = src[c] as u32 * sa * 255 + dst[c] as u32 * da * (255 - sa);
        dst[c] = ((num + oa / 2) / oa).min(255) as u8;
    }
    dst[3] = ((oa + 127) / 255).min(255) as u8;
}

#[derive(Clone, Debug, PartialEq)]
pub struct LayerStack {
    /// Bottom first.
    pub layers: Vec<Layer>,
    pub active: usize,
}

impl LayerStack {
    /// A stack with one layer holding `base`.
    pub fn single(base: Texture) -> LayerStack {
        LayerStack { layers: vec![Layer::from_texture("Layer 1", base)], active: 0 }
    }
    pub fn size(&self) -> (u32, u32) {
        self.layers.first().map_or((1, 1), |l| (l.img.w, l.img.h))
    }
    pub fn active_layer(&self) -> Option<&Layer> {
        self.layers.get(self.active)
    }
    pub fn active_layer_mut(&mut self) -> Option<&mut Layer> {
        self.layers.get_mut(self.active)
    }
    fn fix_active(&mut self) {
        if self.layers.is_empty() {
            self.active = 0;
        } else {
            self.active = self.active.min(self.layers.len() - 1);
        }
    }
    /// Adds an empty layer above the active one and selects it.
    pub fn add(&mut self, name: &str) -> usize {
        let (w, h) = self.size();
        let at = if self.layers.is_empty() { 0 } else { (self.active + 1).min(self.layers.len()) };
        self.layers.insert(at, Layer::new(name, w, h));
        self.active = at;
        at
    }
    pub fn duplicate(&mut self, i: usize) -> Option<usize> {
        let mut l = self.layers.get(i)?.clone();
        l.name = format!("{} copy", l.name);
        self.layers.insert(i + 1, l);
        self.active = i + 1;
        Some(i + 1)
    }
    /// Removes a layer; the last remaining layer cannot be removed.
    pub fn remove(&mut self, i: usize) -> bool {
        if self.layers.len() <= 1 || i >= self.layers.len() {
            return false;
        }
        self.layers.remove(i);
        self.fix_active();
        true
    }
    /// Moves a layer one step up (towards the top of the stack).
    pub fn move_up(&mut self, i: usize) -> bool {
        if i + 1 >= self.layers.len() {
            return false;
        }
        self.layers.swap(i, i + 1);
        if self.active == i {
            self.active = i + 1;
        } else if self.active == i + 1 {
            self.active = i;
        }
        true
    }
    pub fn move_down(&mut self, i: usize) -> bool {
        i > 0 && i < self.layers.len() && self.move_up(i - 1)
    }
    /// Merges layer `i` into the one below it.
    pub fn merge_down(&mut self, i: usize) -> bool {
        if i == 0 || i >= self.layers.len() {
            return false;
        }
        let top = self.layers.remove(i);
        if top.shows() && self.layers[i - 1].img.px.len() == top.img.px.len() {
            let below = &mut self.layers[i - 1];
            for (d, s) in below.img.px.iter_mut().zip(&top.img.px) {
                blend_over(d, *s, top.opacity);
            }
        }
        self.active = i - 1;
        true
    }
    /// All visible layers blended into one picture. `name` is given to the result.
    pub fn flatten(&self, name: &str) -> Texture {
        let (w, h) = self.size();
        let mut out = Texture::new(name, w, h, [0, 0, 0, 0]);
        let n = out.px.len();
        for l in self.layers.iter().filter(|l| l.shows() && l.img.px.len() == n) {
            for (d, s) in out.px.iter_mut().zip(&l.img.px) {
                blend_over(d, *s, l.opacity);
            }
        }
        out
    }
    /// Resizes every layer (nearest neighbour).
    pub fn resize(&mut self, w: u32, h: u32) {
        for l in &mut self.layers {
            l.img = l.img.resized(w, h);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blend_opaque_and_half() {
        let mut d = [10, 20, 30, 255];
        blend_over(&mut d, [200, 100, 50, 255], 255);
        assert_eq!(d, [200, 100, 50, 255]);
        let mut d = [0, 0, 0, 255];
        blend_over(&mut d, [255, 255, 255, 255], 128);
        assert!((d[0] as i32 - 128).abs() <= 1 && d[3] == 255);
        let mut d = [9, 9, 9, 255];
        blend_over(&mut d, [255, 0, 0, 0], 255);
        assert_eq!(d, [9, 9, 9, 255], "a transparent pixel changes nothing");
    }

    #[test]
    fn blend_onto_transparent_keeps_colour() {
        let mut d = [0, 0, 0, 0];
        blend_over(&mut d, [200, 100, 50, 128], 255);
        assert_eq!(&d[..3], &[200, 100, 50]);
        assert_eq!(d[3], 128);
    }

    #[test]
    fn stack_operations() {
        let mut s = LayerStack::single(Texture::new("t", 4, 4, [255, 0, 0, 255]));
        let top = s.add("Layer 2");
        assert_eq!(top, 1);
        s.layers[1].img.set(1, 1, [0, 255, 0, 255]);
        let flat = s.flatten("x");
        assert_eq!(flat.get(1, 1), Some([0, 255, 0, 255]));
        assert_eq!(flat.get(0, 0), Some([255, 0, 0, 255]));
        s.layers[1].visible = false;
        assert_eq!(s.flatten("x").get(1, 1), Some([255, 0, 0, 255]));
        s.layers[1].visible = true;
        s.layers[1].opacity = 0;
        assert_eq!(s.flatten("x").get(1, 1), Some([255, 0, 0, 255]));
        s.layers[1].opacity = 255;
        assert!(s.move_down(1));
        assert_eq!(s.flatten("x").get(1, 1), Some([255, 0, 0, 255]), "the red layer is now on top");
        assert!(s.move_up(0));
        assert!(s.merge_down(1));
        assert_eq!(s.layers.len(), 1);
        assert_eq!(s.layers[0].img.get(1, 1), Some([0, 255, 0, 255]));
        assert!(!s.remove(0), "the last layer stays");
        s.add("a");
        assert!(s.duplicate(1).is_some());
        assert_eq!(s.layers.len(), 3);
        assert!(s.remove(2));
        assert_eq!(s.active, 1);
    }
}
