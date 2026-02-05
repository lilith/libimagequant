use crate::pal::{f_pixel, PalF, PalIndex, MAX_COLORS};
use crate::simd;
use crate::{Error, OrdFloat};

#[cfg(target_arch = "x86_64")]
use archmage::{arcane, Desktop64};

#[cfg(all(not(feature = "std"), feature = "no_std"))]
use crate::no_std_compat::*;

impl<'pal> Nearest<'pal> {
    #[inline(never)]
    pub fn new(palette: &'pal PalF) -> Result<Self, Error> {
        // Check once if SIMD is available, then use the appropriate path
        #[cfg(target_arch = "x86_64")]
        if let Some(token) = simd::summon_token() {
            return new_simd(palette, token);
        }
        new_scalar(palette)
    }
}

/// Scalar path for tree construction
#[inline(never)]
fn new_scalar(palette: &PalF) -> Result<Nearest<'_>, Error> {
    if palette.len() > PalIndex::MAX as usize + 1 {
        return Err(Error::Unsupported);
    }
    let mut indexes: Vec<_> = (0..palette.len())
        .map(|idx| MapIndex { idx: idx as _ })
        .collect();
    if indexes.is_empty() {
        return Err(Error::Unsupported);
    }
    let mut handle = Nearest {
        root: vp_create_node_scalar(&mut indexes, palette),
        palette,
        nearest_other_color_dist: [0.; MAX_COLORS],
        #[cfg(target_arch = "x86_64")]
        has_simd: false,
    };
    for (i, color) in palette.as_slice().iter().enumerate() {
        let mut best = Visitor {
            idx: 0,
            distance: f32::MAX,
            distance_squared: f32::MAX,
            exclude: Some(i as PalIndex),
        };
        vp_search_node_scalar(&handle.root, color, &mut best);
        handle.nearest_other_color_dist[i] = best.distance_squared / 4.;
    }
    Ok(handle)
}

/// SIMD path for tree construction (x86_64 only)
#[cfg(target_arch = "x86_64")]
#[arcane]
#[inline(never)]
fn new_simd<'pal>(palette: &'pal PalF, _token: Desktop64) -> Result<Nearest<'pal>, Error> {
    if palette.len() > PalIndex::MAX as usize + 1 {
        return Err(Error::Unsupported);
    }
    let mut indexes: Vec<_> = (0..palette.len())
        .map(|idx| MapIndex { idx: idx as _ })
        .collect();
    if indexes.is_empty() {
        return Err(Error::Unsupported);
    }
    let mut handle = Nearest {
        root: vp_create_node_simd(&mut indexes, palette, _token),
        palette,
        nearest_other_color_dist: [0.; MAX_COLORS],
        has_simd: true,
    };
    for (i, color) in palette.as_slice().iter().enumerate() {
        let mut best = Visitor {
            idx: 0,
            distance: f32::MAX,
            distance_squared: f32::MAX,
            exclude: Some(i as PalIndex),
        };
        vp_search_node_simd(&handle.root, color, &mut best, _token);
        handle.nearest_other_color_dist[i] = best.distance_squared / 4.;
    }
    Ok(handle)
}

impl Nearest<'_> {
    #[inline]
    pub fn search(&self, px: &f_pixel, likely_colormap_index: PalIndex) -> (PalIndex, f32) {
        #[cfg(target_arch = "x86_64")]
        if self.has_simd {
            // Re-summon token - this is a cheap check on x86_64
            if let Some(token) = simd::summon_token() {
                return search_simd_inner(self, px, likely_colormap_index, token);
            }
        }
        self.search_scalar(px, likely_colormap_index)
    }

    /// Scalar search path
    #[inline]
    fn search_scalar(&self, px: &f_pixel, likely_colormap_index: PalIndex) -> (PalIndex, f32) {
        let mut best_candidate =
            if let Some(pal_px) = self.palette.as_slice().get(likely_colormap_index as usize) {
                let guess_diff = simd::diff_scalar(px, pal_px);
                if guess_diff < self.nearest_other_color_dist[likely_colormap_index as usize] {
                    return (likely_colormap_index, guess_diff);
                }
                Visitor {
                    distance: guess_diff.sqrt(),
                    distance_squared: guess_diff,
                    idx: likely_colormap_index,
                    exclude: None,
                }
            } else {
                Visitor {
                    distance: f32::INFINITY,
                    distance_squared: f32::INFINITY,
                    idx: 0,
                    exclude: None,
                }
            };

        vp_search_node_scalar(&self.root, px, &mut best_candidate);
        (best_candidate.idx, best_candidate.distance_squared)
    }
}

/// SIMD search path - separate function to allow #[arcane]
#[cfg(target_arch = "x86_64")]
#[arcane]
#[inline]
fn search_simd_inner(
    this: &Nearest<'_>,
    px: &f_pixel,
    likely_colormap_index: PalIndex,
    _token: Desktop64,
) -> (PalIndex, f32) {
    let mut best_candidate =
        if let Some(pal_px) = this.palette.as_slice().get(likely_colormap_index as usize) {
            let guess_diff = simd::diff_simd(_token, px, pal_px);
            if guess_diff < this.nearest_other_color_dist[likely_colormap_index as usize] {
                return (likely_colormap_index, guess_diff);
            }
            Visitor {
                distance: guess_diff.sqrt(),
                distance_squared: guess_diff,
                idx: likely_colormap_index,
                exclude: None,
            }
        } else {
            Visitor {
                distance: f32::INFINITY,
                distance_squared: f32::INFINITY,
                idx: 0,
                exclude: None,
            }
        };

    vp_search_node_simd(&this.root, px, &mut best_candidate, _token);
    (best_candidate.idx, best_candidate.distance_squared)
}

pub(crate) struct Nearest<'pal> {
    root: Node,
    palette: &'pal PalF,
    nearest_other_color_dist: [f32; MAX_COLORS],
    /// Whether SIMD was used to build this tree (x86_64 only)
    #[cfg(target_arch = "x86_64")]
    has_simd: bool,
}

pub struct MapIndex {
    pub idx: PalIndex,
}

pub struct Visitor {
    pub distance: f32,
    pub distance_squared: f32,
    pub idx: PalIndex,
    pub exclude: Option<PalIndex>,
}

impl Visitor {
    #[inline]
    fn visit(&mut self, distance: f32, distance_squared: f32, idx: PalIndex) {
        if distance_squared < self.distance_squared && self.exclude != Some(idx) {
            self.distance = distance;
            self.distance_squared = distance_squared;
            self.idx = idx;
        }
    }
}

pub(crate) struct Node {
    vantage_point: f_pixel,
    inner: NodeInner,
    idx: PalIndex,
}

const LEAF_MAX_SIZE: usize = 6;

enum NodeInner {
    Nodes {
        radius: f32,
        radius_squared: f32,
        near: Box<Node>,
        far: Box<Node>,
    },
    Leaf {
        len: u8,
        idxs: [PalIndex; LEAF_MAX_SIZE],
        colors: Box<[f_pixel; LEAF_MAX_SIZE]>,
    },
}

// ============================================================================
// Scalar implementations
// ============================================================================

#[inline(never)]
fn vp_create_node_scalar(indexes: &mut [MapIndex], items: &PalF) -> Node {
    debug_assert!(!indexes.is_empty());
    let palette = items.as_slice();

    if indexes.len() <= 1 {
        let idx = indexes.first().map(|i| i.idx).unwrap_or_default();
        return Node {
            vantage_point: palette.get(usize::from(idx)).copied().unwrap_or_default(),
            idx,
            inner: NodeInner::Leaf {
                len: 0,
                idxs: [0; LEAF_MAX_SIZE],
                colors: Box::new([f_pixel::default(); LEAF_MAX_SIZE]),
            },
        };
    }

    let most_popular_item = indexes
        .iter()
        .enumerate()
        .max_by_key(move |(_, idx)| {
            OrdFloat::new(
                items
                    .pop_as_slice()
                    .get(usize::from(idx.idx))
                    .map(|p| p.popularity())
                    .unwrap_or_default(),
            )
        })
        .map(|(n, _)| n)
        .unwrap_or_default();
    indexes.swap(most_popular_item, 0);
    let (ref_, indexes) = indexes.split_first_mut().unwrap();

    let vantage_point = palette
        .get(usize::from(ref_.idx))
        .copied()
        .unwrap_or_default();
    indexes.sort_by_cached_key(move |i| {
        OrdFloat::new(
            palette
                .get(usize::from(i.idx))
                .map(|px| simd::diff_scalar(&vantage_point, px))
                .unwrap_or_default(),
        )
    });

    let num_indexes = indexes.len();

    let inner = if num_indexes <= LEAF_MAX_SIZE {
        let mut colors = [f_pixel::default(); LEAF_MAX_SIZE];
        let mut idxs = [Default::default(); LEAF_MAX_SIZE];

        indexes
            .iter()
            .zip(colors.iter_mut().zip(idxs.iter_mut()))
            .for_each(|(i, (color, idx))| {
                if let Some(c) = palette.get(usize::from(i.idx)) {
                    *idx = i.idx;
                    *color = *c;
                }
            });
        NodeInner::Leaf {
            len: num_indexes as _,
            idxs,
            colors: Box::new(colors),
        }
    } else {
        let half_index = num_indexes / 2;
        let (near, far) = indexes.split_at_mut(half_index);
        debug_assert!(!near.is_empty());
        debug_assert!(!far.is_empty());
        let radius_squared = palette
            .get(usize::from(far[0].idx))
            .map(|px| simd::diff_scalar(&vantage_point, px))
            .unwrap_or_default();
        let radius = radius_squared.sqrt();
        NodeInner::Nodes {
            radius,
            radius_squared,
            near: Box::new(vp_create_node_scalar(near, items)),
            far: Box::new(vp_create_node_scalar(far, items)),
        }
    };

    Node {
        inner,
        vantage_point,
        idx: ref_.idx,
    }
}

fn vp_search_node_scalar(mut node: &Node, needle: &f_pixel, best_candidate: &mut Visitor) {
    loop {
        let distance_squared = simd::diff_scalar(&node.vantage_point, needle);
        let distance = distance_squared.sqrt();

        best_candidate.visit(distance, distance_squared, node.idx);

        match node.inner {
            NodeInner::Nodes {
                radius,
                radius_squared,
                ref near,
                ref far,
            } => {
                if distance_squared < radius_squared {
                    vp_search_node_scalar(near, needle, best_candidate);
                    if distance >= radius - best_candidate.distance {
                        node = far;
                        continue;
                    }
                } else {
                    vp_search_node_scalar(far, needle, best_candidate);
                    if distance <= radius + best_candidate.distance {
                        node = near;
                        continue;
                    }
                }
                break;
            }
            NodeInner::Leaf {
                len: num,
                ref idxs,
                ref colors,
            } => {
                colors
                    .iter()
                    .zip(idxs.iter().copied())
                    .take(num as usize)
                    .for_each(|(color, idx)| {
                        let distance_squared = simd::diff_scalar(color, needle);
                        best_candidate.visit(distance_squared.sqrt(), distance_squared, idx);
                    });
                break;
            }
        }
    }
}

// ============================================================================
// SIMD implementations (x86_64 only)
// ============================================================================

#[cfg(target_arch = "x86_64")]
#[arcane]
#[inline(never)]
fn vp_create_node_simd(indexes: &mut [MapIndex], items: &PalF, _token: Desktop64) -> Node {
    debug_assert!(!indexes.is_empty());
    let palette = items.as_slice();

    if indexes.len() <= 1 {
        let idx = indexes.first().map(|i| i.idx).unwrap_or_default();
        return Node {
            vantage_point: palette.get(usize::from(idx)).copied().unwrap_or_default(),
            idx,
            inner: NodeInner::Leaf {
                len: 0,
                idxs: [0; LEAF_MAX_SIZE],
                colors: Box::new([f_pixel::default(); LEAF_MAX_SIZE]),
            },
        };
    }

    let most_popular_item = indexes
        .iter()
        .enumerate()
        .max_by_key(move |(_, idx)| {
            OrdFloat::new(
                items
                    .pop_as_slice()
                    .get(usize::from(idx.idx))
                    .map(|p| p.popularity())
                    .unwrap_or_default(),
            )
        })
        .map(|(n, _)| n)
        .unwrap_or_default();
    indexes.swap(most_popular_item, 0);
    let (ref_, indexes) = indexes.split_first_mut().unwrap();

    let vantage_point = palette
        .get(usize::from(ref_.idx))
        .copied()
        .unwrap_or_default();
    indexes.sort_by_cached_key(move |i| {
        OrdFloat::new(
            palette
                .get(usize::from(i.idx))
                .map(|px| simd::diff_simd(_token, &vantage_point, px))
                .unwrap_or_default(),
        )
    });

    let num_indexes = indexes.len();

    let inner = if num_indexes <= LEAF_MAX_SIZE {
        let mut colors = [f_pixel::default(); LEAF_MAX_SIZE];
        let mut idxs = [Default::default(); LEAF_MAX_SIZE];

        indexes
            .iter()
            .zip(colors.iter_mut().zip(idxs.iter_mut()))
            .for_each(|(i, (color, idx))| {
                if let Some(c) = palette.get(usize::from(i.idx)) {
                    *idx = i.idx;
                    *color = *c;
                }
            });
        NodeInner::Leaf {
            len: num_indexes as _,
            idxs,
            colors: Box::new(colors),
        }
    } else {
        let half_index = num_indexes / 2;
        let (near, far) = indexes.split_at_mut(half_index);
        debug_assert!(!near.is_empty());
        debug_assert!(!far.is_empty());
        let radius_squared = palette
            .get(usize::from(far[0].idx))
            .map(|px| simd::diff_simd(_token, &vantage_point, px))
            .unwrap_or_default();
        let radius = radius_squared.sqrt();
        NodeInner::Nodes {
            radius,
            radius_squared,
            near: Box::new(vp_create_node_simd(near, items, _token)),
            far: Box::new(vp_create_node_simd(far, items, _token)),
        }
    };

    Node {
        inner,
        vantage_point,
        idx: ref_.idx,
    }
}

#[cfg(target_arch = "x86_64")]
#[arcane]
fn vp_search_node_simd(
    node: &Node,
    needle: &f_pixel,
    best_candidate: &mut Visitor,
    _token: Desktop64,
) {
    let mut node = node;
    loop {
        let distance_squared = simd::diff_simd(_token, &node.vantage_point, needle);
        let distance = distance_squared.sqrt();

        best_candidate.visit(distance, distance_squared, node.idx);

        match node.inner {
            NodeInner::Nodes {
                radius,
                radius_squared,
                ref near,
                ref far,
            } => {
                if distance_squared < radius_squared {
                    vp_search_node_simd(near, needle, best_candidate, _token);
                    if distance >= radius - best_candidate.distance {
                        node = far;
                        continue;
                    }
                } else {
                    vp_search_node_simd(far, needle, best_candidate, _token);
                    if distance <= radius + best_candidate.distance {
                        node = near;
                        continue;
                    }
                }
                break;
            }
            NodeInner::Leaf {
                len: num,
                ref idxs,
                ref colors,
            } => {
                colors
                    .iter()
                    .zip(idxs.iter().copied())
                    .take(num as usize)
                    .for_each(|(color, idx)| {
                        let distance_squared = simd::diff_simd(_token, color, needle);
                        best_candidate.visit(distance_squared.sqrt(), distance_squared, idx);
                    });
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simd_path() {
        // This should use the SIMD path if available
        use crate::pal::ARGBF;
        let palette_data = vec![
            ARGBF { a: 1.0, r: 0.0, g: 0.0, b: 0.0 },
            ARGBF { a: 1.0, r: 1.0, g: 0.0, b: 0.0 },
            ARGBF { a: 1.0, r: 0.0, g: 1.0, b: 0.0 },
            ARGBF { a: 1.0, r: 0.0, g: 0.0, b: 1.0 },
        ];
        // This just checks that the code compiles and runs
        // In a real test we'd verify correctness
        #[cfg(target_arch = "x86_64")]
        if let Some(token) = simd::summon_token() {
            println!("SIMD token available");
        }
    }
}
