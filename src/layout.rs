//! Zone layouts as a tree of splits.
//!
//! This is the shape KWin uses for its custom tiles: a node is either a zone
//! or a row/column of child nodes, each child taking a share of its parent.
//! The KWin script builds tiles from exactly this structure, so the editor and
//! the result on screen always agree.

use serde::{Deserialize, Serialize};

/// Children side by side (KWin `LayoutDirection::Horizontal`).
pub const COLUMNS: u8 = 1;
/// Children stacked (KWin `LayoutDirection::Vertical`).
pub const ROWS: u8 = 2;

/// Smallest share a zone can be squeezed to while dragging a divider.
pub const MIN_SHARE: f64 = 0.05;

fn one() -> f64 {
    1.0
}

/// x, y, width, height.
pub type Rect = [f64; 4];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ZoneNode {
    /// Share of the parent along the parent's direction.
    #[serde(rename = "s", default = "one")]
    pub size: f64,
    /// [`COLUMNS`] or [`ROWS`]; only meaningful with children.
    #[serde(rename = "d", default)]
    pub dir: u8,
    #[serde(rename = "c", default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<ZoneNode>,
}

impl Default for ZoneNode {
    fn default() -> Self {
        Self::leaf(1.0)
    }
}

/// A divider between two neighbouring children, for dragging.
#[derive(Debug, Clone, PartialEq)]
pub struct Divider {
    /// Path of the container whose children it separates.
    pub path: Vec<usize>,
    /// The divider sits after child `index`.
    pub index: usize,
    pub dir: u8,
    /// The line, in the coordinates passed in.
    pub rect: Rect,
    /// The container's extent along its direction: start and length.
    pub span: (f64, f64),
}

impl ZoneNode {
    pub fn leaf(size: f64) -> Self {
        Self {
            size,
            dir: COLUMNS,
            children: Vec::new(),
        }
    }

    pub fn split_even(dir: u8, count: usize) -> Self {
        let count = count.max(1);
        if count == 1 {
            return Self::leaf(1.0);
        }
        Self {
            size: 1.0,
            dir,
            children: (0..count).map(|_| Self::leaf(1.0 / count as f64)).collect(),
        }
    }

    pub fn with_shares(dir: u8, shares: &[f64]) -> Self {
        Self {
            size: 1.0,
            dir,
            children: shares.iter().map(|&s| Self::leaf(s)).collect(),
        }
    }

    pub fn is_leaf(&self) -> bool {
        self.children.is_empty()
    }

    pub fn zone_count(&self) -> usize {
        if self.is_leaf() {
            1
        } else {
            self.children.iter().map(ZoneNode::zone_count).sum()
        }
    }

    pub fn node_mut(&mut self, path: &[usize]) -> Option<&mut ZoneNode> {
        match path.split_first() {
            None => Some(self),
            Some((i, rest)) => self.children.get_mut(*i)?.node_mut(rest),
        }
    }

    /// Zones in KWin's order (depth first), each with its path and rectangle.
    pub fn zones(&self, rect: Rect) -> Vec<(Vec<usize>, Rect)> {
        let mut out = Vec::new();
        self.collect_zones(rect, &mut Vec::new(), &mut out);
        out
    }

    fn collect_zones(&self, rect: Rect, path: &mut Vec<usize>, out: &mut Vec<(Vec<usize>, Rect)>) {
        if self.is_leaf() {
            out.push((path.clone(), rect));
            return;
        }
        for (i, (child, child_rect)) in self.child_rects(rect).into_iter().enumerate() {
            path.push(i);
            child.collect_zones(child_rect, path, out);
            path.pop();
        }
    }

    fn child_rects(&self, rect: Rect) -> Vec<(&ZoneNode, Rect)> {
        let total: f64 = self.children.iter().map(|c| c.size).sum::<f64>().max(1e-9);
        let [x, y, w, h] = rect;
        let mut offset = 0.0;
        self.children
            .iter()
            .map(|child| {
                let share = child.size / total;
                let r = if self.dir == ROWS {
                    [x, y + offset * h, w, share * h]
                } else {
                    [x + offset * w, y, share * w, h]
                };
                offset += share;
                (child, r)
            })
            .collect()
    }

    /// Every divider, for hit-testing and drawing.
    pub fn dividers(&self, rect: Rect) -> Vec<Divider> {
        let mut out = Vec::new();
        self.collect_dividers(rect, &mut Vec::new(), &mut out);
        out
    }

    fn collect_dividers(&self, rect: Rect, path: &mut Vec<usize>, out: &mut Vec<Divider>) {
        if self.is_leaf() {
            return;
        }
        let rects = self.child_rects(rect);
        let span = if self.dir == ROWS {
            (rect[1], rect[3])
        } else {
            (rect[0], rect[2])
        };
        for (i, (_, r)) in rects.iter().enumerate() {
            if i + 1 < rects.len() {
                let line = if self.dir == ROWS {
                    [r[0], r[1] + r[3], r[2], 0.0]
                } else {
                    [r[0] + r[2], r[1], 0.0, r[3]]
                };
                out.push(Divider {
                    path: path.clone(),
                    index: i,
                    dir: self.dir,
                    rect: line,
                    span,
                });
            }
        }
        for (i, (child, r)) in rects.into_iter().enumerate() {
            path.push(i);
            child.collect_dividers(r, path, out);
            path.pop();
        }
    }

    /// Split the zone at `path` in two along `dir`, the first part taking
    /// `at` (0..1) of it.
    pub fn split(&mut self, path: &[usize], dir: u8, at: f64) {
        let at = at.clamp(0.1, 0.9);
        if let Some((last, parent_path)) = path.split_last()
            && let Some(parent) = self.node_mut(parent_path)
            && parent.dir == dir
            && parent.children.get(*last).is_some_and(ZoneNode::is_leaf)
        {
            let size = parent.children[*last].size;
            parent.children[*last].size = size * at;
            parent
                .children
                .insert(last + 1, ZoneNode::leaf(size * (1.0 - at)));
            return;
        }
        if let Some(node) = self.node_mut(path)
            && node.is_leaf()
        {
            node.dir = dir;
            node.children = vec![ZoneNode::leaf(at), ZoneNode::leaf(1.0 - at)];
        }
        self.normalize();
    }

    /// Remove the zone at `path`; its neighbour takes over the space.
    pub fn remove(&mut self, path: &[usize]) {
        let Some((last, parent_path)) = path.split_last() else {
            return;
        };
        let Some(parent) = self.node_mut(parent_path) else {
            return;
        };
        if *last >= parent.children.len() {
            return;
        }
        let removed = parent.children.remove(*last);
        if parent.children.is_empty() {
            return;
        }
        let neighbour = if *last > 0 { last - 1 } else { 0 };
        parent.children[neighbour].size += removed.size;
        self.normalize();
    }

    /// Move a divider to `pos`, in the same coordinates as `Divider::span`.
    pub fn move_divider(&mut self, divider: &Divider, pos: f64) {
        let (start, length) = divider.span;
        if length <= 0.0 {
            return;
        }
        let Some(node) = self.node_mut(&divider.path) else {
            return;
        };
        let i = divider.index;
        if i + 1 >= node.children.len() {
            return;
        }
        let total: f64 = node.children.iter().map(|c| c.size).sum::<f64>().max(1e-9);
        let before: f64 = node.children[..i].iter().map(|c| c.size / total).sum();
        let a = node.children[i].size / total;
        let b = node.children[i + 1].size / total;
        let fraction =
            ((pos - start) / length).clamp(before + MIN_SHARE, before + a + b - MIN_SHARE);
        node.children[i].size = (fraction - before) * total;
        node.children[i + 1].size = (before + a + b - fraction) * total;
    }

    /// Remove single-child containers, merge a container into its parent when
    /// both run the same way, and make shares add up to one.
    ///
    /// KWin's `split` only nests a new layout when the direction changes, so a
    /// tree must be in this form before the script can rebuild it.
    pub fn normalize(&mut self) {
        for child in &mut self.children {
            child.normalize();
        }
        let mut flat = Vec::with_capacity(self.children.len());
        for child in std::mem::take(&mut self.children) {
            if !child.is_leaf() && child.dir == self.dir {
                let total: f64 = child.children.iter().map(|c| c.size).sum::<f64>().max(1e-9);
                for mut grandchild in child.children {
                    grandchild.size = grandchild.size / total * child.size;
                    flat.push(grandchild);
                }
            } else {
                flat.push(child);
            }
        }
        self.children = flat;
        if self.children.len() == 1 {
            let only = self.children.remove(0);
            self.dir = only.dir;
            self.children = only.children;
        }
        let total: f64 = self.children.iter().map(|c| c.size).sum();
        if total > 0.0 {
            for child in &mut self.children {
                child.size /= total;
            }
        }
        if self.is_leaf() {
            self.dir = COLUMNS;
        }
    }

    /// Same structure and shares within a small tolerance.
    pub fn same_as(&self, other: &ZoneNode) -> bool {
        (self.is_leaf() && other.is_leaf())
            || (self.dir == other.dir
                && self.children.len() == other.children.len()
                && self
                    .children
                    .iter()
                    .zip(&other.children)
                    .all(|(a, b)| (a.size - b.size).abs() < 0.01 && a.same_as(b)))
    }
}

/// Ready-made layouts, as in the gallery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Template {
    Single,
    Columns,
    Rows,
    Grid,
    Priority,
    MainSide,
}

impl Template {
    pub const ALL: [Template; 6] = [
        Template::Columns,
        Template::Rows,
        Template::Grid,
        Template::Priority,
        Template::MainSide,
        Template::Single,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Template::Single => "No zones",
            Template::Columns => "Columns",
            Template::Rows => "Rows",
            Template::Grid => "Grid",
            Template::Priority => "Priority",
            Template::MainSide => "Main + stack",
        }
    }

    /// Default zone count and whether it can be changed.
    pub fn default_count(self) -> usize {
        match self {
            Template::Single => 1,
            Template::Columns | Template::Priority => 3,
            Template::Rows => 2,
            Template::Grid => 4,
            Template::MainSide => 3,
        }
    }

    pub fn count_range(self) -> (usize, usize) {
        match self {
            Template::Single => (1, 1),
            Template::Columns | Template::Rows => (2, 8),
            Template::Grid => (4, 16),
            Template::Priority => (3, 5),
            Template::MainSide => (2, 6),
        }
    }

    pub fn build(self, count: usize) -> ZoneNode {
        let (lo, hi) = self.count_range();
        let count = count.clamp(lo, hi);
        let mut tree = match self {
            Template::Single => ZoneNode::leaf(1.0),
            Template::Columns => ZoneNode::split_even(COLUMNS, count),
            Template::Rows => ZoneNode::split_even(ROWS, count),
            Template::Grid => {
                // Columns first, as wide screens want; extra zones go to the
                // last columns so the left ones stay larger.
                let cols = (count as f64).sqrt().ceil() as usize;
                let base = count / cols;
                let extra = count % cols;
                ZoneNode {
                    size: 1.0,
                    dir: COLUMNS,
                    children: (0..cols)
                        .map(|i| {
                            let rows = base + usize::from(i >= cols - extra);
                            let mut column = ZoneNode::split_even(ROWS, rows);
                            column.size = 1.0 / cols as f64;
                            column
                        })
                        .collect(),
                }
            }
            Template::Priority => match count {
                3 => ZoneNode::with_shares(COLUMNS, &[0.25, 0.5, 0.25]),
                4 => {
                    let mut right = ZoneNode::split_even(ROWS, 2);
                    right.size = 0.25;
                    ZoneNode {
                        size: 1.0,
                        dir: COLUMNS,
                        children: vec![ZoneNode::leaf(0.25), ZoneNode::leaf(0.5), right],
                    }
                }
                _ => {
                    let mut left = ZoneNode::split_even(ROWS, 2);
                    left.size = 0.25;
                    let mut right = ZoneNode::split_even(ROWS, 2);
                    right.size = 0.25;
                    ZoneNode {
                        size: 1.0,
                        dir: COLUMNS,
                        children: vec![left, ZoneNode::leaf(0.5), right],
                    }
                }
            },
            Template::MainSide => {
                let mut side = ZoneNode::split_even(ROWS, count - 1);
                side.size = 0.4;
                ZoneNode {
                    size: 1.0,
                    dir: COLUMNS,
                    children: vec![ZoneNode::leaf(0.6), side],
                }
            }
        };
        tree.normalize();
        tree
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UNIT: Rect = [0.0, 0.0, 1.0, 1.0];

    fn shares_sum_to_one(node: &ZoneNode) -> bool {
        node.is_leaf()
            || ((node.children.iter().map(|c| c.size).sum::<f64>() - 1.0).abs() < 1e-9
                && node.children.iter().all(shares_sum_to_one))
    }

    #[test]
    fn templates_have_the_requested_zone_count() {
        for template in Template::ALL {
            let (lo, hi) = template.count_range();
            for count in lo..=hi {
                let tree = template.build(count);
                assert_eq!(tree.zone_count(), count, "{template:?} {count}");
                assert!(shares_sum_to_one(&tree));
            }
        }
    }

    #[test]
    fn zones_cover_the_area() {
        let tree = Template::Grid.build(6);
        let area: f64 = tree.zones(UNIT).iter().map(|(_, r)| r[2] * r[3]).sum();
        assert!((area - 1.0).abs() < 1e-9);
    }

    #[test]
    fn split_in_same_direction_adds_a_sibling() {
        let mut tree = ZoneNode::split_even(COLUMNS, 2);
        tree.split(&[1], COLUMNS, 0.5);
        assert_eq!(tree.children.len(), 3);
        assert!(tree.children.iter().all(ZoneNode::is_leaf));
        assert!(shares_sum_to_one(&tree));
    }

    #[test]
    fn split_across_nests() {
        let mut tree = ZoneNode::split_even(COLUMNS, 2);
        tree.split(&[0], ROWS, 0.5);
        assert_eq!(tree.zone_count(), 3);
        assert_eq!(tree.children[0].dir, ROWS);
    }

    #[test]
    fn splitting_the_single_zone() {
        let mut tree = ZoneNode::leaf(1.0);
        tree.split(&[], ROWS, 0.3);
        assert_eq!(tree.dir, ROWS);
        assert!((tree.children[0].size - 0.3).abs() < 1e-9);
    }

    #[test]
    fn removing_collapses_containers() {
        let mut tree = ZoneNode::split_even(COLUMNS, 2);
        tree.split(&[0], ROWS, 0.5);
        tree.remove(&[0, 1]);
        assert_eq!(tree.zone_count(), 2);
        assert!(tree.children.iter().all(ZoneNode::is_leaf));
        tree.remove(&[1]);
        assert!(tree.is_leaf());
    }

    #[test]
    fn normalize_flattens_same_direction() {
        let mut tree = ZoneNode {
            size: 1.0,
            dir: COLUMNS,
            children: vec![ZoneNode::leaf(0.5), ZoneNode::split_even(COLUMNS, 2)],
        };
        tree.children[1].size = 0.5;
        tree.normalize();
        assert_eq!(tree.children.len(), 3);
        assert!((tree.children[2].size - 0.25).abs() < 1e-9);
    }

    #[test]
    fn dividers_move_and_clamp() {
        let mut tree = ZoneNode::split_even(COLUMNS, 2);
        let divider = tree.dividers([0.0, 0.0, 100.0, 50.0]).remove(0);
        assert_eq!(divider.rect[0], 50.0);
        tree.move_divider(&divider, 70.0);
        assert!((tree.children[0].size - 0.7).abs() < 1e-9);
        tree.move_divider(&divider, 100.0);
        assert!((tree.children[1].size - MIN_SHARE).abs() < 1e-9);
    }

    #[test]
    fn serialises_compactly_for_the_script() {
        let tree = ZoneNode::with_shares(COLUMNS, &[0.5, 0.5]);
        let text = serde_json::to_string(&tree).unwrap();
        assert_eq!(
            text,
            r#"{"s":1.0,"d":1,"c":[{"s":0.5,"d":1},{"s":0.5,"d":1}]}"#
        );
        let back: ZoneNode = serde_json::from_str(r#"{"c":[{"s":0.5},{"s":0.5}],"d":2}"#).unwrap();
        assert_eq!(back.dir, ROWS);
        assert_eq!(back.size, 1.0);
    }
}
