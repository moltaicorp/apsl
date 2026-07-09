
use crate::{verify, Impl, Params, Verdict};
use std::io::Write;
use std::process::{Command, Stdio};

pub type BoxSpec = Vec<(f64, f64)>;

pub struct ProcessImpl {
    pub program: String,
    pub args: Vec<String>,
}

impl ProcessImpl {
    pub fn new(program: impl Into<String>) -> Self {
        Self { program: program.into(), args: Vec::new() }
    }
    pub fn arg(mut self, a: impl Into<String>) -> Self {
        self.args.push(a.into());
        self
    }
}

impl Impl for ProcessImpl {
    fn eval(&self, x: &[f64]) -> Vec<f64> {
        let mut child = Command::new(&self.program)
            .args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap_or_else(|e| panic!("spawn {}: {e}", self.program));
        {
            let stdin = child.stdin.as_mut().expect("child stdin");
            let line: String = x
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(" ");
            writeln!(stdin, "{line}").expect("write child stdin");
        }
        let out = child.wait_with_output().expect("child wait");
        let text = String::from_utf8_lossy(&out.stdout);
        text.split_whitespace()
            .map(|t| t.parse::<f64>().unwrap_or_else(|e| panic!("parse node output {t:?}: {e}")))
            .collect()
    }
}

pub fn verify_numeric_node(impl_fn: &dyn Impl, pre: &BoxSpec, post: &BoxSpec) -> Verdict {
    verify_numeric_node_p(impl_fn, pre, post, Params::default())
}

pub fn verify_numeric_node_p(
    impl_fn: &dyn Impl,
    pre: &BoxSpec,
    post: &BoxSpec,
    p: Params,
) -> Verdict {
    let post = post.clone();
    let post_pred = move |y: &[f64]| {
        if y.len() != post.len() {
            return false;
        }
        y.iter()
            .zip(&post)
            .all(|(&yi, &(lo, hi))| yi >= lo && yi <= hi)
    };
    verify(impl_fn, pre, &post_pred, p)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Script(std::path::PathBuf);
    impl Drop for Script {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    fn script(body: &str) -> (Script, ProcessImpl) {
        use std::os::unix::fs::PermissionsExt;
        let uniq = body.bytes().fold(0u64, |a, b| a.wrapping_mul(31).wrapping_add(b as u64));
        let mut path = std::env::temp_dir();
        path.push(format!("apsl-node-{}-{}.sh", std::process::id(), uniq));
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        let pi = ProcessImpl::new(path.to_string_lossy().to_string());
        (Script(path), pi)
    }

    #[test]
    fn external_binary_satisfies() {
        let (_g, pi) = script("read x; awk -v x=$x 'BEGIN{print 2*x}'");
        let v = verify_numeric_node(&pi, &vec![(0.0, 1.0)], &vec![(0.0, 2.0)]);
        assert!(v.satisfies && v.witness.is_none() && !v.refactor_suggested, "{v:?}");
    }

    #[test]
    fn external_binary_violates_with_witness() {
        let (_g, pi) = script("read x; awk -v x=$x 'BEGIN{print 2*x}'");
        let v = verify_numeric_node(&pi, &vec![(0.0, 1.0)], &vec![(0.0, 1.0)]);
        assert!(!v.satisfies, "{v:?}");
        assert!(v.witness.as_ref().unwrap()[0] > 0.5 - 1e-6, "{v:?}");
    }

    #[test]
    fn external_binary_fold_suggests_refactor() {
        let (_g, pi) = script(
            "read x; awk -v x=$x 'BEGIN{d=x-0.5; if(d<0)d=-d; print 1/(d+0.001)}'",
        );
        let v = verify_numeric_node_p(
            &pi,
            &vec![(0.0, 1.0)],
            &vec![(0.0, 1.0e9)], // wide post: isolate the FOLD signal, not a violation
            Params { grid: 11, ..Params::default() },
        );
        assert!(!v.folds.is_empty(), "expected fold near 0.5: {v:?}");
        assert!(v.refactor_suggested, "{v:?}");
    }

    #[test]
    fn in_process_path_still_works() {
        let f = |v: &[f64]| vec![v[0] + v[1], v[0] - v[1]];
        let v = verify_numeric_node(&f, &vec![(0.0, 1.0), (0.0, 1.0)], &vec![(-2.0, 2.0), (-2.0, 2.0)]);
        assert!(v.satisfies && !v.refactor_suggested, "{v:?}");
    }

    #[test]
    fn post_box_rejects_wrong_arity() {
        let f = |x: &[f64]| vec![x[0]];
        let v = verify_numeric_node(&f, &vec![(0.0, 1.0)], &vec![(0.0, 1.0), (0.0, 1.0)]);
        assert!(!v.satisfies && v.witness.is_some(), "{v:?}");
    }
}
