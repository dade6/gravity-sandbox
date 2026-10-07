use bevy::prelude::*;
use bevy::render::mesh::Indices;
use bevy::render::render_resource::{AsBindGroup, PrimitiveTopology, ShaderType};
use bevy::sprite_render::{AlphaMode2d, Material2d, Material2dPlugin};
use bevy_shader::ShaderRef;

// ============================================================
// Material2d for curved dashed lines
// ============================================================

/// GPU-side uniform that the fragment shader reads via `@group(1) @binding(0)`.
#[derive(Clone, Copy, Debug, bevy::reflect::Reflect, bevy::render::render_resource::ShaderType)]
pub struct CurveLineUniforms {
    /// World-space length of one dash + gap cycle.
    pub dash_length: f32,
    /// Fraction of the cycle that is drawn (0.0–1.0).
    pub dash_ratio: f32,
    /// Reserved for future use (padding to 16-byte alignment).
    pub _pad: Vec2,
}

impl Default for CurveLineUniforms {
    fn default() -> Self {
        Self {
            dash_length: 40.0,
            dash_ratio: 0.55,
            _pad: Vec2::ZERO,
        }
    }
}

/// Custom 2D material that renders a Catmull-Rom curved line with
/// GPU-side dashing.  The mesh is a triangle-strip built by
/// [`build_line_mesh`](super::curve_line::build_line_mesh).
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
#[uniform(0, CurveLineUniforms)]
pub struct CurveLineMaterial {
    /// World-space length of one dash + gap cycle.
    pub dash_length: f32,
    /// Fraction of the cycle that is drawn (0.0–1.0).
    pub dash_ratio: f32,
}

impl Default for CurveLineMaterial {
    fn default() -> Self {
        Self {
            dash_length: 40.0,
            dash_ratio: 0.55,
        }
    }
}

impl From<&CurveLineMaterial> for CurveLineUniforms {
    fn from(m: &CurveLineMaterial) -> Self {
        Self {
            dash_length: m.dash_length,
            dash_ratio: m.dash_ratio,
            _pad: Vec2::ZERO,
        }
    }
}

impl Material2d for CurveLineMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/curve_line.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
}

// ============================================================
// Plugin
// ============================================================

pub struct CurveLinePlugin;

impl Plugin for CurveLinePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(Material2dPlugin::<CurveLineMaterial>::default());
    }
}

// ============================================================
// Catmull-Rom spline interpolation
// ============================================================

/// Evaluate a Catmull-Rom spline at parameter `t` ∈ [0, 1] given four control
/// points.  The curve passes through `p1` and `p2` (at t=0 and t=1).
pub fn catmull_rom(p0: Vec2, p1: Vec2, p2: Vec2, p3: Vec2, t: f32) -> Vec2 {
    let t2 = t * t;
    let t3 = t2 * t;
    0.5 * (2.0 * p1
        + (-p0 + p2) * t
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
        + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3)
}

/// Interpolate a trail (slice of `Vec2` points) using Catmull-Rom splines.
///
/// Returns a dense set of points that smoothly pass through every input point.
/// `segments_per_span` controls smoothness: 4–8 is typical.
pub fn interpolate_trail(trail: &[Vec2], segments_per_span: usize) -> Vec<Vec2> {
    if trail.is_empty() {
        return Vec::new();
    }
    if trail.len() == 1 {
        return trail.to_vec();
    }

    // Pad with duplicated endpoints for the spline.
    let mut pts: Vec<Vec2> = Vec::with_capacity(trail.len() + 2);
    pts.push(trail[0]);
    pts.extend_from_slice(trail);
    pts.push(*trail.last().unwrap());

    let mut result = Vec::new();
    // spans go from index 1..len-2 (each span uses 4 consecutive padded points)
    let n_spans = pts.len() - 3;
    for i in 0..n_spans {
        let (p0, p1, p2, p3) = (pts[i], pts[i + 1], pts[i + 2], pts[i + 3]);
        for s in 0..segments_per_span {
            let t = s as f32 / segments_per_span as f32;
            result.push(catmull_rom(p0, p1, p2, p3, t));
        }
    }
    // Append the very last point (t=1 of the final span, already reached above
    // when s == segments_per_span, but we didn't include that).
    result.push(*trail.last().unwrap());
    result
}

// ============================================================
// Mesh generation: triangle-strip line with per-vertex UV & color
// ============================================================

/// Build a `Mesh` representing a thick line (triangle strip) through
/// `smooth_points` with width `line_width` world units.
///
/// Each vertex carries:
/// - **position** (Float32x3, z = 0)
/// - **uv**       (Float32x2) — x = `uv_offset` + cumulative arc-length
///   (for dashing), y = 0|1
///   (left/right edge for anti-aliasing in the fragment shader)
/// - **color**    (Float32x4) — per-vertex RGBA (allows per-segment alpha)
///
/// `uv_offset` anchors the dash pattern in world space: pass the arc-length
/// already consumed by the sliding window so rebuilt meshes keep the same
/// UVs for the same points and dashes don't crawl.
pub fn build_line_mesh(
    smooth_points: &[Vec2],
    line_width: f32,
    color: Color,
    uv_offset: f32,
) -> Mesh {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    let half_w = line_width * 0.5;
    let [r, g, b, a] = color.to_linear().to_f32_array();

    let mut cum_len: f32 = uv_offset;
    let mut prev_pt = smooth_points[0];

    for (i, &pt) in smooth_points.iter().enumerate() {
        if i > 0 {
            cum_len += prev_pt.distance(pt);
        }
        prev_pt = pt;

        // Tangent direction for the perpendicular
        let tangent = if i == 0 {
            (smooth_points[1] - pt).normalize_or_zero()
        } else if i + 1 < smooth_points.len() {
            (smooth_points[i + 1] - smooth_points[i - 1]).normalize_or_zero()
        } else {
            (pt - smooth_points[i - 1]).normalize_or_zero()
        };
        let perp = Vec2::new(-tangent.y, tangent.x);

        let left = pt + perp * half_w;
        let right = pt - perp * half_w;

        let base = (i * 2) as u32;
        positions.push([left.x, left.y, 0.0]);
        positions.push([right.x, right.y, 0.0]);
        uvs.push([cum_len, 0.0]);
        uvs.push([cum_len, 1.0]);
        colors.push([r, g, b, a]);
        colors.push([r, g, b, a]);

        // Triangle strip indices (two triangles per quad)
        if i > 0 {
            let b = base;
            indices.push(b - 2);
            indices.push(b - 1);
            indices.push(b);
            indices.push(b - 1);
            indices.push(b + 1);
            indices.push(b);
        }
    }

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, Default::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

// ============================================================
// Adaptive resampling: constant arc-density + curvature refinement
// ============================================================

/// Resample a raw trail (one point per physics tick) into control points
/// with roughly **constant spatial density**, refined where the path curves.
///
/// Why: decimating by index (`step_by(stride)`) keeps points uniform in
/// *time*, but planets move fast at periapsis and slow at apoapsis — so the
/// control points go sparse exactly where the curve bends most, and
/// Catmull-Rom cuts the corner. Walking by arc-length keeps spatial density
/// constant; the curvature pass then subdivides segments around sharp bends.
///
/// - `len` / `get`: the raw trail (length + indexed accessor, so callers can
///   pass a `VecDeque` without copying it into a contiguous slice).
/// - `base_spacing`: target world-units between control points.
/// - `max_points`: hard budget (mesh cost). If the trail is so long that
///   `base_spacing` would exceed it, spacing scales up gracefully instead of
///   truncating the curve.
/// - `angle_deg`: segments bending more than this get one extra midpoint
///   (sharpest bends first, until the budget is spent).
///
/// Always starts at the first raw point and ends at the last one.
pub fn resample_adaptive(
    len: usize,
    get: impl Fn(usize) -> Vec2,
    base_spacing: f32,
    max_points: usize,
    angle_deg: f32,
) -> Vec<Vec2> {
    if len == 0 {
        return Vec::new();
    }
    let first = get(0);
    if len == 1 {
        return vec![first];
    }
    let base_spacing = base_spacing.max(0.5);
    let max_points = max_points.max(8);

    // Bound the scan cost: stride so we visit at most MAX_SCAN samples.
    // Short trails (the common case) are scanned exactly.
    const MAX_SCAN: usize = 24_000;
    let pre = len.div_ceil(MAX_SCAN).max(1);

    // Estimate total arc-length on the (possibly strided) samples.
    let mut est_len = 0.0f32;
    let mut prev = first;
    let mut last_s = 0usize;
    let mut s = pre;
    while s < len {
        let p = get(s);
        est_len += prev.distance(p);
        prev = p;
        last_s = s;
        s += pre;
    }
    if last_s != len - 1 {
        est_len += prev.distance(get(len - 1));
    }
    if est_len < 1e-6 {
        return vec![first, get(len - 1)];
    }

    // Spacing that fits the budget; constant density while budget allows.
    // (Points = segments + 1, hence `max_points - 1`.)
    let mut spacing = base_spacing;
    if est_len / spacing > (max_points - 1) as f32 {
        spacing = est_len / (max_points - 1) as f32;
    }

    // Pass 1 — arc-length walk: emit a control point every `spacing` units.
    let mut out: Vec<Vec2> = Vec::new();
    let mut idx: Vec<usize> = Vec::new();
    out.push(first);
    idx.push(0);
    let mut acc = 0.0f32;
    let mut prev_p = first;
    let mut j = pre;
    // Always visit the final point even if off-stride.
    while j < len {
        let p = get(j);
        acc += prev_p.distance(p);
        prev_p = p;
        if acc >= spacing {
            out.push(p);
            idx.push(j);
            acc = 0.0;
        }
        j += pre;
    }
    let last = get(len - 1);
    if *out.last().unwrap() != last {
        out.push(last);
        idx.push(len - 1);
    }
    // Safety net: coarse scan granularity can overshoot the budget by a
    // point or two — resample evenly down to exactly `max_points` if so.
    if out.len() > max_points {
        let step = (out.len() - 1) as f32 / (max_points - 1) as f32;
        let mut thinned_out: Vec<Vec2> = Vec::with_capacity(max_points);
        let mut thinned_idx: Vec<usize> = Vec::with_capacity(max_points);
        for i in 0..max_points {
            let k = ((i as f32 * step).round() as usize).min(out.len() - 1);
            thinned_out.push(out[k]);
            thinned_idx.push(idx[k]);
        }
        out = thinned_out;
        idx = thinned_idx;
    }

    // Pass 2 — curvature refinement: subdivide the longer raw segment around
    // each bend sharper than `angle_deg`, sharpest first, within budget.
    if out.len() >= 3 {
        let cos_thresh = angle_deg.to_radians().cos();
        let mut cands: Vec<(f32, usize)> = Vec::new();
        for k in 1..out.len() - 1 {
            let v1 = out[k] - out[k - 1];
            let v2 = out[k + 1] - out[k];
            let n1 = v1.length();
            let n2 = v2.length();
            if n1 < 1e-6 || n2 < 1e-6 {
                continue;
            }
            let cos_a = (v1.dot(v2) / (n1 * n2)).clamp(-1.0, 1.0);
            if cos_a < cos_thresh {
                cands.push((cos_a, k));
            }
        }
        cands.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let mut budget = max_points.saturating_sub(out.len());
        let mut inserts: Vec<(usize, Vec2)> = Vec::new();
        for (_, k) in cands {
            if budget == 0 {
                break;
            }
            let a_len = idx[k] - idx[k - 1];
            let b_len = idx[k + 1] - idx[k];
            let (lo, hi) = if a_len >= b_len {
                (idx[k - 1], idx[k])
            } else {
                (idx[k], idx[k + 1])
            };
            if hi - lo < 2 {
                continue;
            }
            inserts.push((k + 1, get(lo + (hi - lo) / 2)));
            budget -= 1;
        }
        if !inserts.is_empty() {
            inserts.sort_by_key(|(pos, _)| *pos);
            let mut refined: Vec<Vec2> = Vec::with_capacity(out.len() + inserts.len());
            let mut ins = 0;
            for k in 0..out.len() {
                while ins < inserts.len() && inserts[ins].0 == k {
                    let p = inserts[ins].1;
                    let dup = refined
                        .last()
                        .map(|q: &Vec2| q.distance(p) < 1e-3)
                        .unwrap_or(false);
                    if !dup {
                        refined.push(p);
                    }
                    ins += 1;
                }
                refined.push(out[k]);
            }
            while ins < inserts.len() {
                let p = inserts[ins].1;
                let dup = refined
                    .last()
                    .map(|q: &Vec2| q.distance(p) < 1e-3)
                    .unwrap_or(false);
                if !dup {
                    refined.push(p);
                }
                ins += 1;
            }
            out = refined;
        }
    }

    out
}

#[cfg(test)]
mod curve_tests {
    use super::*;

    fn straight_get() -> impl Fn(usize) -> Vec2 {
        move |i| Vec2::new(i as f32, 0.0)
    }

    #[test]
    fn straight_line_has_uniform_spacing() {
        // 1001 raw points over 1000 world units, target spacing 5.
        let out = resample_adaptive(1001, straight_get(), 5.0, 2000, 10.0);
        // ~200 control points, endpoints exact.
        assert!(out.len() >= 195 && out.len() <= 210, "len={}", out.len());
        assert_eq!(out[0], Vec2::new(0.0, 0.0));
        assert_eq!(out[out.len() - 1], Vec2::new(1000.0, 0.0));
        // No gap much larger than the target spacing (straight → no extras).
        for w in out.windows(2) {
            assert!(
                w[0].distance(w[1]) <= 6.5,
                "gap too large: {}",
                w[0].distance(w[1])
            );
        }
    }

    #[test]
    fn sharp_corner_gets_curvature_refinement() {
        // L-shape: 500 units along +x, then 500 along +y. Raw = 1 unit steps.
        let get = |i: usize| {
            if i <= 500 {
                Vec2::new(i as f32, 0.0)
            } else {
                Vec2::new(500.0, (i - 500) as f32)
            }
        };
        let base = resample_adaptive(1001, get, 5.0, 2000, 170.0);
        let refined = resample_adaptive(1001, get, 5.0, 2000, 10.0);
        // 170° threshold ≈ never triggers (only hairpins); 10° catches the corner.
        assert!(
            refined.len() > base.len(),
            "base={} refined={}",
            base.len(),
            refined.len()
        );
        // Some refined point must sit close to the corner apex.
        let corner = Vec2::new(500.0, 0.0);
        let near = refined.iter().any(|p| p.distance(corner) <= 5.0);
        assert!(near, "no refined point near the corner");
    }

    #[test]
    fn budget_scales_spacing_instead_of_truncating() {
        // 100k units long, budget 400 → spacing auto-scales, curve stays whole.
        let out = resample_adaptive(100_001, straight_get(), 5.0, 400, 10.0);
        assert!(out.len() <= 400, "len={}", out.len());
        assert_eq!(out[0], Vec2::new(0.0, 0.0));
        assert_eq!(out[out.len() - 1], Vec2::new(100_000.0, 0.0));
    }

    #[test]
    fn degenerate_inputs_do_not_panic() {
        assert!(resample_adaptive(0, straight_get(), 5.0, 400, 10.0).is_empty());
        assert_eq!(
            resample_adaptive(1, straight_get(), 5.0, 400, 10.0),
            vec![Vec2::new(0.0, 0.0)]
        );
    }
}
