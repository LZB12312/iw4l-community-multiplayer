use super::{GeometryType, GrindSurface, V, cross, dot, orientation, sub};

#[derive(Clone, Copy, Debug)]
pub struct LandingOrientation {
    pub kind: GeometryType, // +0
    pub garbage: bool,      // +4
    pub boardslide_dir: V,  // +16
    pub tipslide_dir: V,    // +32
    pub backslash_dir: V,   // +48
    pub high_side: V,       // +64
}

pub fn update_landing_orientation(
    surface: Option<&GrindSurface>,
    takeoff_position: V,
    grind_position: V,
    up: &mut V,
    out: &mut LandingOrientation,
) {
    let Some(surface) = surface else {
        out.kind = GeometryType::Impossible;
        out.garbage = true;
        return;
    };
    if surface.flags & GrindSurface::INVALID != 0 {
        let x = [1., 0., 0., 0.];
        *up = [0., 1., 0., 0.];
        out.kind = GeometryType::ThinRail;
        out.garbage = true;
        out.boardslide_dir = x;
        out.tipslide_dir = x;
        out.backslash_dir = negate(x);
        out.high_side = x;
        return;
    }

    *up = orientation::tilted_normal(
        surface.direction,
        surface.upmost_normal,
        surface.normal_limits,
    );
    out.kind = surface.kind;
    out.garbage = false;
    out.high_side = surface.high_side;
    // Original cross products are not followed by normalization.
    let side = cross(surface.upmost_normal, surface.direction);
    let tipslide_angle = f32::from_bits(0x3eb2_b8c3);
    if surface.kind == GeometryType::ThinRail {
        out.boardslide_dir = side;
        let oriented = if dot(side, sub(takeoff_position, grind_position)) > 0. {
            side
        } else {
            negate(side)
        };
        let axis = cross(oriented, surface.upmost_normal);
        out.tipslide_dir = orientation::rotate(axis, oriented, tipslide_angle);
        out.backslash_dir = orientation::rotate(axis, oriented, f32::from_bits(0x401a_25c2));
    } else {
        let axis = cross(side, surface.upmost_normal);
        out.boardslide_dir = if surface.kind == GeometryType::FatRail {
            side
        } else {
            cross(*up, surface.direction)
        };
        let sign = if dot(side, surface.high_side) > 0. {
            1.
        } else {
            -1.
        };
        // Original sign vectors broadcast across all four lanes. These are
        // multiplies, unlike the XOR sign inversion in the thin-rail branch.
        out.backslash_dir = orientation::rotate(
            axis.map(|v| v * sign),
            side.map(|v| v * sign),
            f32::from_bits(0x3f3b_a866),
        );
        out.tipslide_dir = orientation::rotate(
            axis.map(|v| v * -sign),
            side.map(|v| v * -sign),
            tipslide_angle,
        );
    }
}

/// Native XOR toggles every sign bit, including W and signed zero.
fn negate(v: V) -> V {
    v.map(|v| f32::from_bits(v.to_bits() ^ 0x8000_0000))
}
