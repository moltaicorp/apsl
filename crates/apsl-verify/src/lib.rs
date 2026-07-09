
pub mod node;
pub use node::{verify_numeric_node, ProcessImpl, BoxSpec};

pub trait Impl {
    fn eval(&self, x: &[f64]) -> Vec<f64>;
}
impl<F: Fn(&[f64]) -> Vec<f64>> Impl for F {
    fn eval(&self, x: &[f64]) -> Vec<f64> { self(x) }
}

#[derive(Debug, Clone)]
pub struct Verdict {
    pub satisfies: bool,
    pub witness: Option<Vec<f64>>,
    pub folds: Vec<Vec<f64>>,
    pub refactor_suggested: bool,
    pub samples: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct Params {
    pub grid: usize,
    pub fold_ratio: f64,
    pub refine_depth: usize,
    pub eps: f64,
}
impl Default for Params {
    fn default() -> Self { Self { grid: 7, fold_ratio: 8.0, refine_depth: 2, eps: 1e-5 } }
}

fn jacobian(f: &dyn Impl, x: &[f64], eps: f64) -> (Vec<f64>, usize, usize) {
    let fx = f.eval(x);
    let (m, n) = (fx.len(), x.len());
    let mut j = vec![0.0; m * n];
    let mut dx = x.to_vec();
    for c in 0..n {
        let save = dx[c];
        dx[c] = save + eps;
        let fd = f.eval(&dx);
        dx[c] = save;
        for r in 0..m {
            j[r * n + c] = (fd[r] - fx[r]) / eps;
        }
    }
    (j, m, n)
}

fn max_singular_value(j: &[f64], m: usize, n: usize) -> f64 {
    if m == 0 || n == 0 { return 0.0; }
    let mut a = vec![0.0; n * n];
    for i in 0..n {
        for k in 0..n {
            let mut s = 0.0;
            for r in 0..m { s += j[r * n + i] * j[r * n + k]; }
            a[i * n + k] = s;
        }
    }
    let mut v = vec![1.0 / (n as f64).sqrt(); n];
    let mut lambda = 0.0;
    for _ in 0..64 {
        let mut av = vec![0.0; n];
        for i in 0..n {
            let mut s = 0.0;
            for k in 0..n { s += a[i * n + k] * v[k]; }
            av[i] = s;
        }
        let norm = av.iter().map(|x| x * x).sum::<f64>().sqrt();
        if norm < 1e-300 { return 0.0; }
        for i in 0..n { v[i] = av[i] / norm; }
        lambda = norm; // Rayleigh-ish: ||A v|| with unit v → dominant eigenvalue
    }
    lambda.max(0.0).sqrt()
}

pub fn verify(
    f: &dyn Impl,
    pre: &[(f64, f64)],
    post: &dyn Fn(&[f64]) -> bool,
    p: Params,
) -> Verdict {
    let n = pre.len();
    let axes: Vec<Vec<f64>> = pre.iter().map(|&(lo, hi)| {
        (0..p.grid).map(|i| lo + (hi - lo) * i as f64 / (p.grid - 1).max(1) as f64).collect()
    }).collect();
    let base = cartesian(&axes);

    let mut witness: Option<Vec<f64>> = None;
    let mut samples = 0usize;
    let mut sigmas = Vec::with_capacity(base.len());

    let check = |x: &[f64], witness: &mut Option<Vec<f64>>, samples: &mut usize| -> f64 {
        *samples += 1;
        let y = f.eval(x);
        if witness.is_none() && !post(&y) { *witness = Some(x.to_vec()); }
        let (j, m, nn) = jacobian(f, x, p.eps);
        max_singular_value(&j, m, nn)
    };

    for x in &base {
        sigmas.push(check(x, &mut witness, &mut samples));
    }
    let mut sorted = sigmas.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let med = if sorted.is_empty() { 0.0 } else { sorted[sorted.len() / 2] };
    let thresh = (med * p.fold_ratio).max(1e-9);

    let mut folds = Vec::new();
    let mut persistent = 0usize;
    for (x, &s) in base.iter().zip(&sigmas) {
        if s > thresh {
            folds.push(x.clone());
            let step: Vec<f64> = pre.iter().map(|&(lo, hi)|
                (hi - lo) / (p.grid - 1).max(1) as f64 / (p.refine_depth + 1) as f64).collect();
            let mut still = false;
            for k in 1..=p.refine_depth {
                for sign in [-1.0, 1.0] {
                    let xr: Vec<f64> = x.iter().zip(pre).enumerate().map(|(i, (xi, &(lo, hi)))|
                        (xi + sign * k as f64 * step[i]).clamp(lo, hi)).collect();
                    if check(&xr, &mut witness, &mut samples) > thresh { still = true; }
                }
            }
            if still { persistent += 1; }
        }
    }
    let _ = n;
    Verdict {
        satisfies: witness.is_none(),
        witness,
        folds,
        refactor_suggested: persistent > 0,
        samples,
    }
}

fn cartesian(axes: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let mut out = vec![vec![]];
    for ax in axes {
        let mut next = Vec::new();
        for prefix in &out {
            for &v in ax {
                let mut p = prefix.clone();
                p.push(v);
                next.push(p);
            }
        }
        out = next;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn satisfies_smooth_monotone() {
        let v = verify(&|x: &[f64]| vec![2.0 * x[0]], &[(0.0, 1.0)],
                       &|y: &[f64]| (0.0..=2.0).contains(&y[0]), Params::default());
        assert!(v.satisfies && v.witness.is_none() && !v.refactor_suggested, "{v:?}");
    }

    #[test]
    fn violates_with_witness() {
        let v = verify(&|x: &[f64]| vec![2.0 * x[0]], &[(0.0, 1.0)],
                       &|y: &[f64]| (0.0..=1.0).contains(&y[0]), Params::default());
        assert!(!v.satisfies, "{v:?}");
        assert!(v.witness.as_ref().unwrap()[0] > 0.5 - 1e-6, "{v:?}");
    }

    #[test]
    fn fold_suggests_refactor() {
        let spike = |x: &[f64]| vec![1.0 / ((x[0] - 0.5).abs() + 1e-3)];
        let v = verify(&spike, &[(0.0, 1.0)], &|_y: &[f64]| true,
                       Params { grid: 11, ..Params::default() });
        assert!(!v.folds.is_empty(), "expected fold near 0.5: {v:?}");
        assert!(v.refactor_suggested, "{v:?}");
    }

    #[test]
    fn two_d_satisfies() {
        let f = |v: &[f64]| vec![v[0] + v[1], v[0] - v[1]];
        let v = verify(&f, &[(0.0, 1.0), (0.0, 1.0)],
                       &|y: &[f64]| (-2.0..=2.0).contains(&y[0]) && (-2.0..=2.0).contains(&y[1]),
                       Params::default());
        assert!(v.satisfies && !v.refactor_suggested, "{v:?}");
    }
}
