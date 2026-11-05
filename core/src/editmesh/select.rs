//! Selection in the edit mesh: the three select modes, flushing between them, loops, rings,
//! shortest paths, linked geometry and grow/shrink.
use super::*;
use std::collections::VecDeque;

impl EditMesh {
    pub fn face_hidden(&self, f: u32) -> bool {
        self.hidden_f.contains(&f)
    }
    pub fn vert_hidden(&self, v: u32) -> bool {
        self.hidden_v.contains(&v)
    }
    /// A row of [`EditMesh::edge_table`] is hidden: an end is hidden, or every polygon using it is.
    fn edge_row_hidden(&self, &(e, faces, all_hidden): &(Edge, u32, bool)) -> bool {
        self.vert_hidden(e.0) || self.vert_hidden(e.1) || faces > 0 && all_hidden
    }

    /// Recomputes the two derived selection sets from the one the current mode owns
    /// (vertices in Vertex mode, edges in Edge mode, faces in Face mode).
    pub fn sync_selection(&mut self) {
        let nv = self.verts.len() as u32;
        self.sel_v.retain(|&v| v < nv);
        let nf = self.faces.len() as u32;
        self.sel_f.retain(|&f| f < nf);
        match self.mode {
            SelectMode::Vertex => {
                let s = &self.sel_v;
                self.sel_e = self.all_edges().into_iter().filter(|e| s.contains(&e.0) && s.contains(&e.1)).collect();
                self.sel_f = (0..nf).filter(|&f| self.faces[f as usize].verts().all(|v| s.contains(&v))).collect();
            }
            SelectMode::Edge => {
                let all: BTreeSet<Edge> = self.all_edges().into_iter().collect();
                self.sel_e = self.sel_e.intersection(&all).copied().collect();
                self.sel_v = self.sel_e.iter().flat_map(|e| [e.0, e.1]).collect();
                let s = &self.sel_e;
                self.sel_f = (0..nf).filter(|&f| self.faces[f as usize].edges().all(|(a, b)| s.contains(&ek(a, b)))).collect();
            }
            SelectMode::Face => {
                self.sel_v = self.sel_f.iter().flat_map(|&f| self.faces[f as usize].verts().collect::<Vec<_>>()).collect();
                self.sel_e = self.sel_f.iter().flat_map(|&f| self.faces[f as usize].edges().map(|(a, b)| ek(a, b)).collect::<Vec<_>>()).collect();
            }
        }
        // hidden elements are never selected
        if !self.hidden_v.is_empty() || !self.hidden_f.is_empty() {
            let hv = self.hidden_v.clone();
            self.sel_v.retain(|v| !hv.contains(v));
            self.sel_e.retain(|e| !hv.contains(&e.0) && !hv.contains(&e.1));
            let hf = self.hidden_f.clone();
            self.sel_f.retain(|f| !hf.contains(f));
        }
    }

    /// Switches the select mode. Going to a "bigger" element keeps only fully selected ones (as in Blender).
    pub fn set_mode(&mut self, mode: SelectMode) {
        if mode == self.mode {
            return;
        }
        self.sync_selection();
        self.mode = mode;
        self.sync_selection();
    }

    pub fn select_all(&mut self, on: bool) {
        if on {
            self.sel_v = (0..self.verts.len() as u32).filter(|&v| !self.vert_hidden(v)).collect();
            self.sel_e = self.edge_table().into_iter().filter(|x| !self.edge_row_hidden(x)).map(|x| x.0).collect();
            self.sel_f = (0..self.faces.len() as u32).filter(|&f| !self.face_hidden(f)).collect();
        } else {
            self.sel_v.clear();
            self.sel_e.clear();
            self.sel_f.clear();
            self.active = None;
        }
        self.sync_selection();
    }
    pub fn any_selected(&self) -> bool {
        !self.sel_v.is_empty()
    }
    pub fn select_invert(&mut self) {
        match self.mode {
            SelectMode::Vertex => self.sel_v = (0..self.verts.len() as u32).filter(|v| !self.sel_v.contains(v) && !self.vert_hidden(*v)).collect(),
            SelectMode::Edge => self.sel_e = self.edge_table().into_iter().filter(|x| !self.sel_e.contains(&x.0) && !self.edge_row_hidden(x)).map(|x| x.0).collect(),
            SelectMode::Face => self.sel_f = (0..self.faces.len() as u32).filter(|f| !self.sel_f.contains(f) && !self.face_hidden(*f)).collect(),
        }
        self.sync_selection();
    }

    /// Click selection of one element. `extend` toggles it in the current selection (Shift+click).
    pub fn click_vert(&mut self, v: u32, extend: bool) {
        if !extend {
            self.sel_v.clear();
            self.sel_e.clear();
            self.sel_f.clear();
        }
        let mode = self.mode;
        self.mode = SelectMode::Vertex;
        self.sync_selection();
        if extend && self.sel_v.contains(&v) && self.active == Some(v) {
            self.sel_v.remove(&v);
            self.active = None;
        } else {
            self.sel_v.insert(v);
            self.active = Some(v);
        }
        self.sync_selection();
        self.mode = mode;
        self.sync_selection();
    }
    pub fn click_edge(&mut self, e: Edge, extend: bool) {
        let e = ek(e.0, e.1);
        if !extend {
            self.sel_v.clear();
            self.sel_e.clear();
            self.sel_f.clear();
        }
        self.mode = SelectMode::Edge;
        self.sync_selection();
        if extend && self.sel_e.contains(&e) {
            self.sel_e.remove(&e);
        } else {
            self.sel_e.insert(e);
            self.active = Some(e.1);
        }
        self.sync_selection();
    }
    pub fn click_face(&mut self, f: u32, extend: bool) {
        if !extend {
            self.sel_v.clear();
            self.sel_e.clear();
            self.sel_f.clear();
        }
        self.mode = SelectMode::Face;
        self.sync_selection();
        if extend && self.sel_f.contains(&f) {
            self.sel_f.remove(&f);
        } else {
            self.sel_f.insert(f);
            self.active = self.faces.get(f as usize).and_then(|x| x.c.first()).map(|c| c.v);
        }
        self.sync_selection();
    }

    /// Adds (or with `deselect`, removes) a set of vertices, edges or faces chosen by a box or a lasso.
    pub fn select_many(&mut self, verts: &[u32], edges: &[Edge], faces: &[u32], deselect: bool) {
        match self.mode {
            SelectMode::Vertex => {
                for &v in verts {
                    if deselect {
                        self.sel_v.remove(&v);
                    } else {
                        self.sel_v.insert(v);
                    }
                }
            }
            SelectMode::Edge => {
                for &e in edges {
                    let e = ek(e.0, e.1);
                    if deselect {
                        self.sel_e.remove(&e);
                    } else {
                        self.sel_e.insert(e);
                    }
                }
            }
            SelectMode::Face => {
                for &f in faces {
                    if deselect {
                        self.sel_f.remove(&f);
                    } else {
                        self.sel_f.insert(f);
                    }
                }
            }
        }
        self.sync_selection();
    }

    /// Everything connected to the selection (Ctrl+L).
    pub fn select_linked(&mut self) {
        let topo = self.topo();
        let mut seen: BTreeSet<u32> = self.sel_v.clone();
        let mut q: VecDeque<u32> = seen.iter().copied().collect();
        while let Some(v) = q.pop_front() {
            for &n in &topo.vert_nbrs[v as usize] {
                if !self.vert_hidden(n) && seen.insert(n) {
                    q.push_back(n);
                }
            }
        }
        self.select_verts_flush(seen, &topo);
    }
    /// The connected piece under one element (L in Blender, hovering an element).
    pub fn select_linked_from(&mut self, v: u32, deselect: bool) {
        let topo = self.topo();
        let mut seen: BTreeSet<u32> = [v].into();
        let mut q: VecDeque<u32> = [v].into();
        while let Some(x) = q.pop_front() {
            for &n in &topo.vert_nbrs[x as usize] {
                if !self.vert_hidden(n) && seen.insert(n) {
                    q.push_back(n);
                }
            }
        }
        if deselect {
            let keep: BTreeSet<u32> = self.sel_v.difference(&seen).copied().collect();
            self.sel_v.clear();
            self.sel_e.clear();
            self.sel_f.clear();
            self.select_verts_flush(keep, &topo);
        } else {
            let mut all = self.sel_v.clone();
            all.extend(seen);
            self.select_verts_flush(all, &topo);
        }
    }
    /// Selects exactly these vertices and the edges/faces they enclose, whatever the mode.
    pub fn select_verts_flush(&mut self, verts: BTreeSet<u32>, _topo: &Topo) {
        self.sel_v = verts;
        let s = &self.sel_v;
        self.sel_e = self.all_edges().into_iter().filter(|e| s.contains(&e.0) && s.contains(&e.1)).collect();
        self.sel_f = (0..self.faces.len() as u32).filter(|&f| self.faces[f as usize].verts().all(|v| s.contains(&v))).collect();
        self.sync_selection();
    }

    /// Grows the selection by one ring of neighbours (Ctrl+Numpad +).
    pub fn grow(&mut self) {
        let topo = self.topo();
        match self.mode {
            SelectMode::Face => {
                let verts = self.sel_v.clone();
                for (fi, f) in self.faces.iter().enumerate() {
                    if !self.face_hidden(fi as u32) && f.verts().any(|v| verts.contains(&v)) {
                        self.sel_f.insert(fi as u32);
                    }
                }
                self.sync_selection();
            }
            _ => {
                let mut s = self.sel_v.clone();
                for &v in &self.sel_v {
                    for &n in &topo.vert_nbrs[v as usize] {
                        if !self.vert_hidden(n) {
                            s.insert(n);
                        }
                    }
                }
                self.select_verts_flush(s, &topo);
            }
        }
    }
    /// Shrinks the selection by one ring (Ctrl+Numpad −).
    pub fn shrink(&mut self) {
        let topo = self.topo();
        // a vertex stays when all of its neighbours are selected
        let keep: BTreeSet<u32> = self.sel_v.iter().copied().filter(|&v| topo.vert_nbrs[v as usize].iter().all(|n| self.sel_v.contains(n))).collect();
        match self.mode {
            SelectMode::Face => {
                self.sel_f = self.sel_f.iter().copied().filter(|&f| self.faces[f as usize].verts().all(|v| keep.contains(&v) || topo.vert_faces[v as usize].iter().all(|g| self.sel_f.contains(g)))).collect();
                // faces on the rim of the region go away
                let rim: BTreeSet<u32> = self.sel_f.iter().copied().filter(|&f| self.faces[f as usize].edges().any(|(a, b)| topo.faces_of(ek(a, b)).iter().any(|g| !self.sel_f.contains(g)))).collect();
                self.sel_f = self.sel_f.difference(&rim).copied().collect();
                self.sync_selection();
            }
            _ => self.select_verts_flush(keep, &topo),
        }
    }

    /// The edge loop through `e`: across vertices with four edges, or along a boundary (Alt+click).
    pub fn edge_loop(&self, topo: &Topo, e: Edge) -> Vec<Edge> {
        let e = ek(e.0, e.1);
        let boundary = topo.is_boundary(e);
        let mut out: Vec<Edge> = vec![e];
        let mut seen: BTreeSet<Edge> = [e].into();
        for (from, to) in [(e.0, e.1), (e.1, e.0)] {
            let (mut a, mut b) = (from, to);
            loop {
                let next = if boundary { self.next_boundary(topo, a, b) } else { self.next_loop_edge(topo, a, b) };
                let Some(c) = next else { break };
                let ne = ek(b, c);
                if !seen.insert(ne) {
                    break;
                }
                out.push(ne);
                a = b;
                b = c;
            }
        }
        out
    }
    /// Continues a loop that arrived at `b` coming from `a`: the edge at `b` that shares no face with (a, b).
    fn next_loop_edge(&self, topo: &Topo, a: u32, b: u32) -> Option<u32> {
        let nb = &topo.vert_nbrs[b as usize];
        if nb.len() != 4 {
            return None;
        }
        let fa = topo.faces_of(ek(a, b));
        if fa.len() != 2 {
            return None;
        }
        // all faces around b must be quads... no: only the edge not touching the faces of (a,b)
        nb.iter().copied().find(|&c| c != a && topo.faces_of(ek(b, c)).iter().all(|f| !fa.contains(f)) && !topo.faces_of(ek(b, c)).is_empty())
    }
    fn next_boundary(&self, topo: &Topo, a: u32, b: u32) -> Option<u32> {
        let cands: Vec<u32> = topo.vert_nbrs[b as usize].iter().copied().filter(|&c| c != a && topo.is_boundary(ek(b, c))).collect();
        (cands.len() == 1).then(|| cands[0])
    }

    /// The edge ring through `e`: the opposite edges of consecutive quads (Ctrl+Alt+click, loop cut).
    pub fn edge_ring(&self, topo: &Topo, e: Edge) -> Vec<Edge> {
        let e = ek(e.0, e.1);
        let mut out = vec![e];
        let mut seen: BTreeSet<Edge> = [e].into();
        let mut seen_f: BTreeSet<u32> = BTreeSet::new();
        for &start in topo.faces_of(e) {
            let (mut cur, mut f) = (e, start);
            loop {
                if !seen_f.insert(f) {
                    break;
                }
                let face = &self.faces[f as usize];
                if face.c.len() != 4 {
                    break;
                }
                let Some(k) = face.edges().position(|(a, b)| ek(a, b) == cur) else { break };
                let (a, b) = face.edges().nth((k + 2) % 4).unwrap();
                let opp = ek(a, b);
                if !seen.insert(opp) {
                    break;
                }
                out.push(opp);
                let Some(&g) = topo.faces_of(opp).iter().find(|&&g| g != f) else { break };
                cur = opp;
                f = g;
            }
        }
        out
    }
    /// The quads crossed by the ring through `e` (face loop select).
    pub fn face_loop(&self, topo: &Topo, e: Edge) -> Vec<u32> {
        let ring = self.edge_ring(topo, e);
        let set: BTreeSet<Edge> = ring.iter().copied().collect();
        let mut out = vec![];
        for (fi, f) in self.faces.iter().enumerate() {
            if f.c.len() == 4 && f.edges().filter(|&(a, b)| set.contains(&ek(a, b))).count() >= 2 {
                out.push(fi as u32);
            }
        }
        out
    }

    /// Loop selection (Alt+click): edges in Vertex/Edge mode, quads in Face mode.
    pub fn select_loop(&mut self, e: Edge, extend: bool, ring: bool) {
        let topo = self.topo();
        if !extend {
            self.sel_v.clear();
            self.sel_e.clear();
            self.sel_f.clear();
        }
        match self.mode {
            SelectMode::Face => {
                for f in self.face_loop(&topo, e) {
                    if !self.face_hidden(f) {
                        self.sel_f.insert(f);
                    }
                }
            }
            SelectMode::Vertex => {
                let edges = if ring { self.edge_ring(&topo, e) } else { self.edge_loop(&topo, e) };
                for (a, b) in edges {
                    self.sel_v.insert(a);
                    self.sel_v.insert(b);
                }
            }
            SelectMode::Edge => {
                let edges = if ring { self.edge_ring(&topo, e) } else { self.edge_loop(&topo, e) };
                self.sel_e.extend(edges);
            }
        }
        self.sync_selection();
    }

    /// Shortest path along edges between two vertices (Ctrl+click), by edge length.
    pub fn shortest_path(&self, topo: &Topo, from: u32, to: u32) -> Vec<u32> {
        let n = self.verts.len();
        let mut dist = vec![f32::INFINITY; n];
        let mut prev = vec![u32::MAX; n];
        dist[from as usize] = 0.0;
        let mut heap: std::collections::BinaryHeap<(std::cmp::Reverse<u32>, u32)> = std::collections::BinaryHeap::new();
        heap.push((std::cmp::Reverse(0), from));
        while let Some((std::cmp::Reverse(dk), v)) = heap.pop() {
            let d = f32::from_bits(dk);
            if d > dist[v as usize] {
                continue;
            }
            if v == to {
                break;
            }
            for &nb in &topo.vert_nbrs[v as usize] {
                if self.vert_hidden(nb) {
                    continue;
                }
                let nd = d + (self.pos(v) - self.pos(nb)).length();
                if nd < dist[nb as usize] {
                    dist[nb as usize] = nd;
                    prev[nb as usize] = v;
                    heap.push((std::cmp::Reverse(nd.to_bits()), nb));
                }
            }
        }
        if !dist[to as usize].is_finite() {
            return vec![];
        }
        let mut path = vec![to];
        let mut c = to;
        while c != from {
            c = prev[c as usize];
            if c == u32::MAX {
                return vec![];
            }
            path.push(c);
        }
        path.reverse();
        path
    }
    /// Adds the shortest path from the active vertex to `v` to the selection.
    pub fn select_path_to(&mut self, v: u32) {
        let Some(a) = self.active else {
            self.click_vert(v, true);
            return;
        };
        let topo = self.topo();
        let path = self.shortest_path(&topo, a, v);
        let mode = self.mode;
        match mode {
            SelectMode::Face => {
                // faces along the path: those that contain two consecutive path vertices
                for w in path.windows(2) {
                    for &f in topo.faces_of(ek(w[0], w[1])) {
                        self.sel_f.insert(f);
                    }
                }
            }
            SelectMode::Edge => {
                for w in path.windows(2) {
                    self.sel_e.insert(ek(w[0], w[1]));
                }
            }
            SelectMode::Vertex => self.sel_v.extend(path.iter().copied()),
        }
        self.active = Some(v);
        self.sync_selection();
    }

    /// Faces that are selected in the current mode (all their vertices or edges are selected).
    pub fn selected_faces(&self) -> Vec<u32> {
        self.sel_f.iter().copied().collect()
    }
    pub fn selected_edges(&self) -> Vec<Edge> {
        self.sel_e.iter().copied().collect()
    }
    pub fn selected_verts(&self) -> Vec<u32> {
        self.sel_v.iter().copied().collect()
    }
    /// Median of the selected vertices (the pivot of transforms).
    pub fn selection_center(&self) -> Option<Vec3> {
        if self.sel_v.is_empty() {
            return None;
        }
        let mut s = Vec3::ZERO;
        for &v in &self.sel_v {
            s += self.pos(v);
        }
        Some(s / self.sel_v.len() as f32)
    }
    /// Average normal of the selection: the selected faces, or the vertex normals.
    pub fn selection_normal(&self) -> Vec3 {
        let mut n = Vec3::ZERO;
        if !self.sel_f.is_empty() {
            for &f in &self.sel_f {
                n += newell(&self.face_points(&self.faces[f as usize]));
            }
        } else {
            let topo = self.topo();
            for &v in &self.sel_v {
                n += self.vert_normal(&topo, v);
            }
        }
        n.normalized()
    }

    /// Hides the selection (H) or everything else (Shift+H).
    pub fn hide(&mut self, unselected: bool) {
        let topo = self.topo();
        let (hv, hf): (BTreeSet<u32>, BTreeSet<u32>) = if unselected {
            let v: BTreeSet<u32> = (0..self.verts.len() as u32).filter(|v| !self.sel_v.contains(v)).collect();
            let f: BTreeSet<u32> = (0..self.faces.len() as u32).filter(|f| !self.sel_f.contains(f)).collect();
            (v, f)
        } else if self.mode == SelectMode::Face {
            let f = self.sel_f.clone();
            // a vertex hides only when every face around it is hidden
            let v: BTreeSet<u32> = self.sel_v.iter().copied().filter(|&v| topo.vert_faces[v as usize].iter().all(|g| f.contains(g) || self.hidden_f.contains(g))).collect();
            (v, f)
        } else {
            let v = self.sel_v.clone();
            let f: BTreeSet<u32> = (0..self.faces.len() as u32).filter(|&f| self.faces[f as usize].verts().any(|x| v.contains(&x))).collect();
            (v, f)
        };
        self.hidden_v.extend(hv);
        self.hidden_f.extend(hf);
        self.sync_selection();
    }
    /// Shows everything hidden and selects it (Alt+H).
    pub fn reveal(&mut self) {
        let (hv, hf) = (std::mem::take(&mut self.hidden_v), std::mem::take(&mut self.hidden_f));
        match self.mode {
            SelectMode::Face => self.sel_f.extend(hf),
            SelectMode::Vertex => self.sel_v.extend(hv),
            SelectMode::Edge => {
                for e in self.all_edges() {
                    if hv.contains(&e.0) || hv.contains(&e.1) {
                        self.sel_e.insert(e);
                    }
                }
            }
        }
        self.sync_selection();
    }
}
