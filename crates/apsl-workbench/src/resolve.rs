
use std::collections::HashMap;
use std::sync::Mutex;

use apsl_verify::{verify_numeric_node, BoxSpec, Verdict};

pub const EMBED_DIM: usize = 3072;

#[derive(Debug, Clone, PartialEq)]
pub struct Contract {
    pub canonical: String,
    pub pre: BoxSpec,
    pub post: BoxSpec,
}

pub type Embedding = Vec<f32>;

#[derive(Debug, Clone)]
pub struct Candidate {
    pub contract: Contract,
    pub store_path: String,
    pub score: f32,
}

#[derive(Debug, Clone)]
pub struct ResolvedImpl {
    pub store_path: String,
    pub reused: bool,
    pub verdict_satisfies: bool,
}

pub trait MaivecIndex {
    fn search(&self, query: &Embedding, k: usize) -> Vec<Candidate>;

    fn put(&self, embedding: Embedding, contract: Contract, store_path: &str, verdict: &Verdict);
}


pub fn embed_contract(contract: &Contract) -> Embedding {
    let mut v = vec![0.0f32; EMBED_DIM];
    let mut h: u64 = 0xcbf29ce484222325;
    for (i, b) in contract.canonical.bytes().enumerate() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
        let idx = (h as usize ^ i.wrapping_mul(2654435761)) % EMBED_DIM;
        v[idx] += 1.0 + (b as f32) / 255.0;
    }
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in &mut v {
            *x /= norm;
        }
    }
    v
}


pub fn covers(candidate: &Contract, requested: &Contract) -> bool {
    if candidate.pre.len() != requested.pre.len() {
        return false;
    }
    if candidate.post.len() != requested.post.len() {
        return false;
    }
    let pre_ok = requested
        .pre
        .iter()
        .zip(&candidate.pre)
        .all(|(&(rlo, rhi), &(clo, chi))| clo <= rlo && chi >= rhi);
    let post_ok = candidate
        .post
        .iter()
        .zip(&requested.post)
        .all(|(&(clo, chi), &(rlo, rhi))| rlo <= clo && chi <= rhi);
    pre_ok && post_ok
}

pub fn cover_filter(candidates: Vec<Candidate>, requested: &Contract) -> Vec<Candidate> {
    candidates
        .into_iter()
        .filter(|c| covers(&c.contract, requested))
        .collect()
}


pub type ImplFn<'a> = dyn Fn(&[f64]) -> Vec<f64> + 'a;

pub fn verify_candidate(impl_fn: &ImplFn<'_>, requested: &Contract) -> Verdict {
    verify_numeric_node(&impl_fn, &requested.pre, &requested.post)
}


pub fn resolve_impl<'a>(
    index: &dyn MaivecIndex,
    requested: &Contract,
    k: usize,
    eval_for: &dyn Fn(&Candidate) -> Box<ImplFn<'a>>,
    synthesize: &dyn Fn(&Contract) -> (String, Box<ImplFn<'a>>),
) -> ResolvedImpl {
    let query = embed_contract(requested);
    let candidates = index.search(&query, k);
    let covering = cover_filter(candidates, requested);

    for cand in &covering {
        let f = eval_for(cand);
        let verdict = verify_candidate(&*f, requested);
        if verdict.satisfies {
            index.put(query.clone(), requested.clone(), &cand.store_path, &verdict);
            return ResolvedImpl {
                store_path: cand.store_path.clone(),
                reused: true,
                verdict_satisfies: true,
            };
        }
    }

    let (store_path, f) = synthesize(requested);
    let verdict = verify_candidate(&*f, requested);
    index.put(query, requested.clone(), &store_path, &verdict);
    ResolvedImpl {
        store_path,
        reused: false,
        verdict_satisfies: verdict.satisfies,
    }
}


struct Entry {
    embedding: Embedding,
    contract: Contract,
    store_path: String,
}

#[derive(Default)]
pub struct InMemoryIndex {
    entries: Mutex<Vec<Entry>>,
}

impl InMemoryIndex {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn seed(&self, contract: Contract, store_path: &str) {
        let embedding = embed_contract(&contract);
        self.entries.lock().unwrap().push(Entry {
            embedding,
            contract,
            store_path: store_path.to_string(),
        });
    }

    pub fn len(&self) -> usize {
        self.entries.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

fn cosine(a: &Embedding, b: &Embedding) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

impl MaivecIndex for InMemoryIndex {
    fn search(&self, query: &Embedding, k: usize) -> Vec<Candidate> {
        let entries = self.entries.lock().unwrap();
        let mut scored: Vec<Candidate> = entries
            .iter()
            .map(|e| Candidate {
                contract: e.contract.clone(),
                store_path: e.store_path.clone(),
                score: cosine(query, &e.embedding),
            })
            .collect();
        scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(k);
        scored
    }

    fn put(&self, embedding: Embedding, contract: Contract, store_path: &str, _verdict: &Verdict) {
        self.entries.lock().unwrap().push(Entry {
            embedding,
            contract,
            store_path: store_path.to_string(),
        });
    }
}

#[allow(dead_code)]
fn _manifest_marker(_: &HashMap<String, String>) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn contract(canon: &str, pre: BoxSpec, post: BoxSpec) -> Contract {
        Contract { canonical: canon.into(), pre, post }
    }

    #[test]
    fn hit_reuses_verified_candidate() {
        let index = InMemoryIndex::new();
        let stored = contract("double : (x:[0,1]) -> [0,2]", vec![(0.0, 1.0)], vec![(0.0, 2.0)]);
        index.seed(stored, "/nix/store/aaa-double");

        let requested = stored_like();
        let eval = |_c: &Candidate| -> Box<ImplFn<'static>> { Box::new(|x: &[f64]| vec![2.0 * x[0]]) };
        let synth = |_c: &Contract| -> (String, Box<ImplFn<'static>>) {
            (String::from("/nix/store/synth"), Box::new(|x: &[f64]| x.to_vec()))
        };

        let before = index.len();
        let r = resolve_impl(&index, &requested, 5, &eval, &synth);
        assert!(r.reused, "should reuse the seeded artifact: {r:?}");
        assert!(r.verdict_satisfies);
        assert_eq!(r.store_path, "/nix/store/aaa-double");
        assert_eq!(index.len(), before + 1, "writeback should grow corpus");
    }

    fn stored_like() -> Contract {
        contract("double : (x:[0,1]) -> [0,2]", vec![(0.0, 1.0)], vec![(0.0, 2.0)])
    }

    #[test]
    fn miss_falls_through_to_synthesis() {
        let index = InMemoryIndex::new();
        index.seed(stored_like(), "/nix/store/aaa-double");

        let requested = contract(
            "add : (x:[0,1], y:[0,1]) -> [0,2]",
            vec![(0.0, 1.0), (0.0, 1.0)],
            vec![(0.0, 2.0)],
        );
        let eval = |_c: &Candidate| -> Box<ImplFn<'static>> {
            Box::new(|x: &[f64]| vec![2.0 * x[0]])
        };
        let synth = |_c: &Contract| -> (String, Box<ImplFn<'static>>) {
            (String::from("/nix/store/bbb-add"), Box::new(|x: &[f64]| vec![x[0] + x[1]]))
        };

        let r = resolve_impl(&index, &requested, 5, &eval, &synth);
        assert!(!r.reused, "no cover ⇒ must synthesize: {r:?}");
        assert!(r.verdict_satisfies, "synthesized add satisfies [0,2]");
        assert_eq!(r.store_path, "/nix/store/bbb-add");
        assert_eq!(index.len(), 2, "synthesized impl written back to corpus");
    }

    #[test]
    fn similar_but_unverified_is_rejected() {
        let index = InMemoryIndex::new();
        let requested = stored_like();
        index.seed(stored_like(), "/nix/store/ccc-bad");

        let eval = |_c: &Candidate| -> Box<ImplFn<'static>> { Box::new(|x: &[f64]| vec![10.0 * x[0]]) };
        let synth = |_c: &Contract| -> (String, Box<ImplFn<'static>>) {
            (String::from("/nix/store/ddd-good"), Box::new(|x: &[f64]| vec![2.0 * x[0]]))
        };

        let r = resolve_impl(&index, &requested, 5, &eval, &synth);
        assert!(!r.reused, "unverified candidate must not be reused: {r:?}");
        assert_eq!(r.store_path, "/nix/store/ddd-good");
    }

    #[test]
    fn cover_lattice_pre_wider_post_narrower() {
        let cand = contract("c", vec![(0.0, 2.0)], vec![(0.0, 1.0)]);
        let req = contract("r", vec![(0.0, 1.0)], vec![(0.0, 2.0)]);
        assert!(covers(&cand, &req));
        assert!(!covers(&req, &cand));
    }
}
