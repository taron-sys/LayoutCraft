//! NURBS curves of Visio's `NURBSTo` row (step 285; probe: probe-nurbs.ps1 + analyze-nurbs.py + check-outline-in-png.py).
//!
//! `NURBSTo` is `MoveTo` (the start) plus ONE row whose `E` cell is `NURBS(knotLast, degree, xType, yType, x1, y1, k1, w1, ...)`. Measured on Visio's own curves
//! (DrawBezier through 4 points, DrawSpline through 4, 6 and 9 points; Visio's exported PNG has a drawn pixel at every sampled point of the curve evaluated as below):
//!  * points  = [start, the listed control points, the row's own X / Y]; a control point is a fraction of Width / Height when xType / yType is 0, inches when 1;
//!  * weights = [D, w1 .. wm, B] (the cells of the row: D the first, B the last weight);
//!  * knots   = [0 x (degree + 1), the listed knots that are > 0 in order, then A when it is > 0, then knotLast x (degree + 1)] (the other listed knots are the zeros of
//!    the clamped start). That is a clamped B-spline with one interior knot per chord-length parameter of the data points 2 .. n - 3.
//!  * `DrawSpline(x0, y0, x1, y1, ...)` is the interpolating cubic with those knots (the "not-a-knot" ends): parameter t_i = the chord length up to point i, knotLast = the
//!    total chord length, interior knots t_2 .. t_(n-3); the controls solve N(t_i) P = Q_i (`interpolate`; checked against Visio's own numbers in the tests).
//! Visio's box of a curve is slightly smaller than the true extent (it flattens the curve coarsely: up to about 2 percent on the probe curves); here the box can be the true
//! extent of the curve.

/// The clamped knot vector of a curve: `degree + 1` zeros, the interior knots, `degree + 1` times `knot_last`.
pub fn knot_vector(degree: usize, interior: &[f64], knot_last: f64) -> Vec<f64> {
    let mut knots = vec![0.0; degree + 1];
    knots.extend_from_slice(interior);
    knots.extend(std::iter::repeat(knot_last).take(degree + 1));
    knots
}

/// The knot span index of `u` (the largest i with knots[i] <= u < knots[i + 1], the last non-empty span for u = the end).
fn find_span(knots: &[f64], degree: usize, n_points: usize, u: f64) -> usize {
    let last = n_points - 1;
    if u >= knots[last + 1] {
        return last;
    }
    if u <= knots[degree] {
        return degree;
    }
    let (mut lo, mut hi) = (degree, last + 1);
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if u < knots[mid] { hi = mid } else { lo = mid }
    }
    lo
}

/// The `degree + 1` non-zero basis functions of the span `span` at `u` (the NURBS book, algorithm A2.2).
fn basis(knots: &[f64], degree: usize, span: usize, u: f64) -> Vec<f64> {
    let mut n = vec![0.0; degree + 1];
    let mut left = vec![0.0; degree + 1];
    let mut right = vec![0.0; degree + 1];
    n[0] = 1.0;
    for j in 1..=degree {
        left[j] = u - knots[span + 1 - j];
        right[j] = knots[span + j] - u;
        let mut saved = 0.0;
        for r in 0..j {
            let denom = right[r + 1] + left[j - r];
            let temp = if denom.abs() < 1e-300 { 0.0 } else { n[r] / denom };
            n[r] = saved + right[r + 1] * temp;
            saved = left[j - r] * temp;
        }
        n[j] = saved;
    }
    n
}

/// The point of a rational B-spline at parameter `u`; None when the data do not fit together (knots != points + degree + 1) or the weights vanish.
pub fn eval(points: &[(f64, f64)], weights: &[f64], knots: &[f64], degree: usize, u: f64) -> Option<(f64, f64)> {
    if points.len() < degree + 1 || weights.len() != points.len() || knots.len() != points.len() + degree + 1 {
        return None;
    }
    let u = u.clamp(knots[0], knots[knots.len() - 1]);
    let span = find_span(knots, degree, points.len(), u);
    let n = basis(knots, degree, span, u);
    let (mut x, mut y, mut w) = (0.0, 0.0, 0.0);
    for (j, b) in n.iter().enumerate() {
        let i = span - degree + j;
        let bw = b * weights[i];
        x += bw * points[i].0;
        y += bw * points[i].1;
        w += bw;
    }
    if w.abs() < 1e-12 { None } else { Some((x / w, y / w)) }
}

/// `steps + 1` points along the curve from its start to its end.
pub fn sample(points: &[(f64, f64)], weights: &[f64], knots: &[f64], degree: usize, steps: usize) -> Option<Vec<(f64, f64)>> {
    let (a, b) = (knots.first().copied()?, knots.last().copied()?);
    (0..=steps).map(|i| eval(points, weights, knots, degree, a + (b - a) * i as f64 / steps as f64)).collect()
}

/// The result of `interpolate`: all control points (the first and the last are the data's), the interior knots and the last knot.
#[derive(Debug, Clone, PartialEq)]
pub struct Fit {
    pub controls: Vec<(f64, f64)>,
    pub interior: Vec<f64>,
    pub knot_last: f64,
}

/// Visio's DrawSpline: the cubic through `data` (4 to 200 points, no two equal in a row) with the chord length parameters and the interior knots t_2 .. t_(n-3).
pub fn interpolate(data: &[(f64, f64)]) -> Option<Fit> {
    let n = data.len();
    if !(4..=200).contains(&n) {
        return None;
    }
    let mut t = vec![0.0; n];
    for i in 1..n {
        t[i] = t[i - 1] + ((data[i].0 - data[i - 1].0).powi(2) + (data[i].1 - data[i - 1].1).powi(2)).sqrt();
        if t[i] - t[i - 1] < 1e-9 {
            return None;
        }
    }
    let knot_last = t[n - 1];
    let interior: Vec<f64> = (2..=n - 3).map(|i| t[i]).collect();
    let knots = knot_vector(3, &interior, knot_last);
    // the matrix A[i][j] = N_j(t_i); solve A * P = Q for x and y together
    let mut a = vec![vec![0.0; n + 2]; n];
    for i in 0..n {
        let u = t[i];
        let span = find_span(&knots, 3, n, u);
        let b = basis(&knots, 3, span, u);
        for (j, v) in b.iter().enumerate() {
            a[i][span - 3 + j] = *v;
        }
        a[i][n] = data[i].0;
        a[i][n + 1] = data[i].1;
    }
    for col in 0..n {
        let pivot = (col..n).max_by(|&p, &q| a[p][col].abs().partial_cmp(&a[q][col].abs()).unwrap())?;
        if a[pivot][col].abs() < 1e-12 {
            return None;
        }
        a.swap(col, pivot);
        for row in 0..n {
            if row != col {
                let f = a[row][col] / a[col][col];
                if f != 0.0 {
                    for k in col..n + 2 {
                        let v = a[col][k];
                        a[row][k] -= f * v;
                    }
                }
            }
        }
    }
    let controls = (0..n).map(|i| (a[i][n] / a[i][i], a[i][n + 1] / a[i][i])).collect();
    Some(Fit { controls, interior, knot_last })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() < tol
    }

    #[test]
    fn a_bezier_is_the_cubic_of_its_four_points() {
        // Visio's DrawBezier(1,5, 2,7, 3,4, 4,6): points relative to the box (3 x 1) at page (1, 5)
        let pts = [(0.0, 0.0), (1.0, 2.0), (2.0, -1.0), (3.0, 1.0)];
        let knots = knot_vector(3, &[], 1.0);
        let mid = eval(&pts, &[1.0; 4], &knots, 3, 0.5).unwrap();
        // B(0.5) = (P0 + 3 P1 + 3 P2 + P3) / 8
        assert!(near(mid.0, 1.5, 1e-9) && near(mid.1, (0.0 + 6.0 - 3.0 + 1.0) / 8.0, 1e-9), "{mid:?}");
        let s = sample(&pts, &[1.0; 4], &knots, 3, 10).unwrap();
        assert!(near(s[0].0, 0.0, 1e-12) && near(s[10].0, 3.0, 1e-12) && near(s[10].1, 1.0, 1e-12));
    }

    #[test]
    fn the_interpolation_reproduces_the_controls_visio_wrote_for_a_six_point_spline() {
        // DrawSpline(1,1, 2,3, 3,1.2, 4,3.4, 5,1.1, 6,3): Visio's box is 5 x 2.660603392887615 at page (1, 0.7680083191620808); its listed controls are fractions of the box
        let data = [(1.0, 1.0), (2.0, 3.0), (3.0, 1.2), (4.0, 3.4), (5.0, 1.1), (6.0, 3.0)];
        let fit = interpolate(&data).unwrap();
        assert!(near(fit.knot_last, 11.366881496571, 1e-9), "{}", fit.knot_last);
        assert!(fit.interior.len() == 2 && near(fit.interior[0], 4.2951940056972, 1e-9) && near(fit.interior[1], 6.711803200416105, 1e-9), "{:?}", fit.interior);
        let (w, h, ox, oy) = (4.999999999999998, 2.660603392887615, 1.0, 0.7680083191620808);
        let visio = [(0.10750522230459, 2.1528102400315), (0.36035192306465, -1.0763151392742), (0.65986385524773, 2.3149486169582), (0.83865313383354, -1.2086272626759)];
        for (i, (fx, fy)) in visio.iter().enumerate() {
            let (px, py) = (ox + fx * w, oy + fy * h);
            assert!(near(fit.controls[i + 1].0, px, 1e-6) && near(fit.controls[i + 1].1, py, 1e-6), "control {}: ours {:?} vs Visio ({px}, {py})", i + 1, fit.controls[i + 1]);
        }
        // the curve passes through the data
        let knots = knot_vector(3, &fit.interior, fit.knot_last);
        let mut t = 0.0;
        for i in 0..data.len() {
            if i > 0 {
                t += ((data[i].0 - data[i - 1].0).powi(2) + (data[i].1 - data[i - 1].1).powi(2)).sqrt();
            }
            let p = eval(&fit.controls, &vec![1.0; fit.controls.len()], &knots, 3, t).unwrap();
            assert!(near(p.0, data[i].0, 1e-9) && near(p.1, data[i].1, 1e-9), "data {i}: {p:?}");
        }
    }

    #[test]
    fn the_nine_point_spline_matches_visios_knots() {
        let data = [(1.0, 8.0), (1.8, 9.5), (2.6, 8.2), (3.4, 9.4), (4.2, 8.1), (5.0, 9.6), (5.8, 8.3), (6.6, 9.2), (7.4, 8.4)];
        let fit = interpolate(&data).unwrap();
        assert!(near(fit.knot_last, 11.757052074705, 1e-9));
        let visio = [3.2264337522474, 4.668654262433, 6.1950880146803, 7.8950880146803, 9.421521766927722];
        assert!(fit.interior.len() == 5 && fit.interior.iter().zip(visio).all(|(a, b)| near(*a, b, 1e-9)), "{:?}", fit.interior);
    }

    #[test]
    fn unusable_data_is_refused() {
        assert!(interpolate(&[(0.0, 0.0), (1.0, 1.0), (2.0, 0.0)]).is_none());
        assert!(interpolate(&[(0.0, 0.0), (0.0, 0.0), (1.0, 1.0), (2.0, 0.0)]).is_none());
        assert!(eval(&[(0.0, 0.0), (1.0, 1.0)], &[1.0, 1.0], &[0.0, 0.0, 1.0, 1.0], 3, 0.5).is_none());
    }
}
