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
    pub _pad: [f32; 2],
}

impl Default for CurveLineUniforms {
    fn default() -> Self {
        Self {
            dash_length: 40.0,
            dash_ratio: 0.55,
            _pad: [0.0; 2],
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
            _pad: [0.0; 2],
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
    0.5 * (
        2.0 * p1
            + (-p0 + p2) * t
            + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
            + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3
    )
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
/// - **uv**       (Float32x2) — x = cumulative arc-length (for dashing), y = 0|1
///   (left/right edge for anti-aliasing in the fragment shader)
/// - **color**    (Float32x4) — per-vertex RGBA (allows per-segment alpha)
pub fn build_line_mesh(smooth_points: &[Vec2], line_width: f32, color: Color) -> Mesh {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    let half_w = line_width * 0.5;
    let [r, g, b, a] = color.to_linear().to_f32_array();

    let mut cum_len: f32 = 0.0;
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
