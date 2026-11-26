//! Vector helpers and the triangle queries an `EditableMesh` answers.

pub(crate) type V3 = [f32; 3];

pub(crate) fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub(crate) fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub(crate) fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

pub(crate) fn normalized(v: V3) -> V3 {
    let length = dot(v, v).sqrt();
    match length > 0.0 {
        true => v.map(|c| c / length),
        false => [0.0; 3],
    }
}

/// The square of the sine of the angle at `p` between `a` and `b`.
pub(crate) fn angle_weight(p: V3, a: V3, b: V3) -> f32 {
    let (u, v) = (sub(a, p), sub(b, p));
    let c = cross(u, v);
    let lengths = dot(u, u) * dot(v, v);
    match lengths > 0.0 {
        true => dot(c, c) / lengths,
        false => 0.0,
    }
}

pub(crate) fn blend(corners: [V3; 3], bary: V3) -> V3 {
    std::array::from_fn(|i| {
        corners[0][i] * bary[0] + corners[1][i] * bary[1] + corners[2][i] * bary[2]
    })
}

/// The point of triangle `abc` closest to `p`, as barycentric weights of
/// `a`, `b`, `c` (Ericson, Real-Time Collision Detection, 5.1.5).
pub(crate) fn closest_on_triangle(p: V3, [a, b, c]: [V3; 3]) -> V3 {
    let (ab, ac, ap) = (sub(b, a), sub(c, a), sub(p, a));
    let (d1, d2) = (dot(ab, ap), dot(ac, ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return [1.0, 0.0, 0.0];
    }
    let bp = sub(p, b);
    let (d3, d4) = (dot(ab, bp), dot(ac, bp));
    if d3 >= 0.0 && d4 <= d3 {
        return [0.0, 1.0, 0.0];
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return [1.0 - v, v, 0.0];
    }
    let cp = sub(p, c);
    let (d5, d6) = (dot(ab, cp), dot(ac, cp));
    if d6 >= 0.0 && d5 <= d6 {
        return [0.0, 0.0, 1.0];
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return [1.0 - w, 0.0, w];
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && d4 - d3 >= 0.0 && d5 - d6 >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return [0.0, 1.0 - w, w];
    }
    let denom = 1.0 / (va + vb + vc);
    let (v, w) = (vb * denom, vc * denom);
    [1.0 - v - w, v, w]
}

/// Where the segment `origin`..`origin + direction` first crosses triangle
/// `abc`, from either side, as (distance fraction, barycentric weights).
pub(crate) fn ray_triangle(origin: V3, direction: V3, [a, b, c]: [V3; 3]) -> Option<(f32, V3)> {
    let (e1, e2) = (sub(b, a), sub(c, a));
    let p = cross(direction, e2);
    let det = dot(e1, p);
    if det.abs() < 1e-12 {
        return None;
    }
    let s = sub(origin, a);
    let u = dot(s, p) / det;
    let q = cross(s, e1);
    let v = dot(direction, q) / det;
    let t = dot(e2, q) / det;
    (u >= 0.0 && v >= 0.0 && u + v <= 1.0 && (0.0..=1.0).contains(&t))
        .then_some((t, [1.0 - u - v, u, v]))
}
