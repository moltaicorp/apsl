use std::collections::{BTreeSet, VecDeque};
use std::fmt;
use std::path::{Path, PathBuf};

use apsl_core::ast::{Decl, Program};

use crate::extract::{locally_defined, unresolved_symbols};
use crate::search::{build_search_path, collect_apsl_files, search_symbol, Located};

const MAX_DEPTH: usize = 64;

#[derive(Debug, Clone)]
pub struct ResolvedDep {
    pub symbol: String,
    pub file: PathBuf,
    pub line: u32,
}

#[derive(Debug, Clone)]
pub struct LinkResult {
    pub program: Program,
    pub resolved: Vec<ResolvedDep>,
}

#[derive(Debug, Clone)]
pub enum LinkError {
    NotFound {
        symbol: String,
        search_path: Vec<PathBuf>,
    },
    Collision {
        symbol: String,
        locations: Vec<(PathBuf, u32)>,
    },
    ParseError {
        symbol: String,
        file: PathBuf,
        line: u32,
        msg: String,
    },
    DepthExceeded {
        depth: usize,
        pending: Vec<String>,
    },
}

impl fmt::Display for LinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LinkError::NotFound {
                symbol,
                search_path,
            } => {
                write!(f, "apsl-link: symbol `{}` not found\n  searched:", symbol)?;
                for p in search_path {
                    write!(f, "\n    {}", p.display())?;
                }
                Ok(())
            }
            LinkError::Collision { symbol, locations } => {
                write!(
                    f,
                    "apsl-link: symbol `{}` defined in multiple files:",
                    symbol
                )?;
                for (path, line) in locations {
                    write!(f, "\n  --> {}:{}", path.display(), line)?;
                }
                write!(f, "\nhint: remove one definition or narrow your search path with APSL_PATH or .apsl-path")
            }
            LinkError::ParseError {
                symbol,
                file,
                line,
                msg,
            } => {
                write!(
                    f,
                    "apsl-link: failed to parse definition of `{}` from {}:{}\n  {}",
                    symbol,
                    file.display(),
                    line,
                    msg
                )
            }
            LinkError::DepthExceeded { depth, pending } => {
                write!(
                    f,
                    "apsl-link: resolution depth {} exceeded with {} symbols still pending: {}",
                    depth,
                    pending.len(),
                    pending.join(", ")
                )
            }
        }
    }
}

pub fn link(
    program: &Program,
    source_file: &Path,
    extra_search_dirs: &[PathBuf],
) -> Result<LinkResult, LinkError> {
    let search_dirs = build_search_path(source_file, extra_search_dirs);
    let apsl_files = collect_apsl_files(&search_dirs, source_file);

    let mut merged = program.clone();
    let mut all_defined = locally_defined(program);
    let mut resolved_deps: Vec<ResolvedDep> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();

    let mut queue: VecDeque<String> = unresolved_symbols(program, &all_defined)
        .into_iter()
        .collect();

    let mut depth = 0;

    while !queue.is_empty() {
        if depth >= MAX_DEPTH {
            let pending: Vec<String> = queue.into_iter().collect();
            return Err(LinkError::DepthExceeded { depth, pending });
        }
        depth += 1;

        let mut next_queue: BTreeSet<String> = BTreeSet::new();

        while let Some(symbol) = queue.pop_front() {
            if seen.contains(&symbol) || all_defined.contains(&symbol) {
                continue;
            }
            seen.insert(symbol.clone());

            let hits = search_symbol(&symbol, &apsl_files);

            if hits.is_empty() {
                if apsl_types::is_primitive(&symbol) {
                    continue;
                }
                return Err(LinkError::NotFound {
                    symbol,
                    search_path: search_dirs.clone(),
                });
            }

            let chosen = if hits.len() > 1 {
                check_collision(&symbol, &hits)?
            } else {
                hits[0].clone()
            };

            let block_decls = parse_block(&symbol, &chosen)?;

            for decl in &block_decls {
                let name = decl_name(decl);
                all_defined.insert(name.clone());
                merged.decls.push(decl.clone());
            }

            resolved_deps.push(ResolvedDep {
                symbol: symbol.clone(),
                file: chosen.file.clone(),
                line: chosen.line,
            });

            let new_local = locally_defined(&merged);
            let new_unresolved = unresolved_symbols(&merged, &new_local);
            for s in new_unresolved {
                if !seen.contains(&s) && !all_defined.contains(&s) {
                    next_queue.insert(s);
                }
            }
        }

        queue = next_queue.into_iter().collect();
    }

    Ok(LinkResult {
        program: merged,
        resolved: resolved_deps,
    })
}

fn check_collision(symbol: &str, hits: &[Located]) -> Result<Located, LinkError> {
    let canonical = hits[0].block.trim();
    for hit in &hits[1..] {
        if hit.block.trim() != canonical {
            let locations: Vec<(PathBuf, u32)> =
                hits.iter().map(|h| (h.file.clone(), h.line)).collect();
            return Err(LinkError::Collision {
                symbol: symbol.to_string(),
                locations,
            });
        }
    }
    Ok(hits[0].clone())
}

fn parse_block(symbol: &str, loc: &Located) -> Result<Vec<Decl>, LinkError> {
    match apsl_parse::parse_str(&loc.block) {
        Ok(prog) => {
            if prog.decls.is_empty() {
                return Err(LinkError::ParseError {
                    symbol: symbol.to_string(),
                    file: loc.file.clone(),
                    line: loc.line,
                    msg: "extracted block parsed to zero declarations".to_string(),
                });
            }
            Ok(prog.decls)
        }
        Err(e) => Err(LinkError::ParseError {
            symbol: symbol.to_string(),
            file: loc.file.clone(),
            line: loc.line,
            msg: e.msg,
        }),
    }
}

fn decl_name(d: &Decl) -> String {
    match d {
        Decl::Type(ta) => ta.name.as_str().to_string(),
        Decl::Node(n) => n.name.as_str().to_string(),
        Decl::Graph(g) => g.name.as_str().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::io::Write;

    fn write_temp_apsl(dir: &Path, name: &str, content: &str) -> PathBuf {
        let path = dir.join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        path
    }

    #[test]
    fn link_resolves_type_alias() {
        let dir = std::env::temp_dir().join("apsl_link_test_1");
        let _ = std::fs::create_dir_all(&dir);

        let source_content = "dedupe : Email[] -> Email[]\n  cx    O(1) idem\n";
        let source_path = write_temp_apsl(&dir, "source.apsl", source_content);

        write_temp_apsl(&dir, "types.apsl", "type Email = String\n");

        let prog = apsl_parse::parse_str(source_content).unwrap();
        let result = link(&prog, &source_path, std::slice::from_ref(&dir)).unwrap();

        assert_eq!(result.resolved.len(), 1);
        assert_eq!(result.resolved[0].symbol, "Email");
        assert_eq!(result.program.decls.len(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn link_detects_collision() {
        let dir = std::env::temp_dir().join("apsl_link_test_2");
        let _ = std::fs::create_dir_all(&dir);

        let source_content = "foo : Email -> Email\n  cx    O(1) idem\n";
        let source_path = write_temp_apsl(&dir, "source.apsl", source_content);

        write_temp_apsl(&dir, "a.apsl", "type Email = String\n");
        write_temp_apsl(&dir, "b.apsl", "type Email = Int\n");

        let prog = apsl_parse::parse_str(source_content).unwrap();
        let result = link(&prog, &source_path, std::slice::from_ref(&dir));

        assert!(matches!(result, Err(LinkError::Collision { .. })));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn link_identical_collision_ok() {
        let dir = std::env::temp_dir().join("apsl_link_test_3");
        let _ = std::fs::create_dir_all(&dir);

        let source_content = "foo : Email -> Email\n  cx    O(1) idem\n";
        let source_path = write_temp_apsl(&dir, "source.apsl", source_content);

        write_temp_apsl(&dir, "a.apsl", "type Email = String\n");
        write_temp_apsl(&dir, "b.apsl", "type Email = String\n");

        let prog = apsl_parse::parse_str(source_content).unwrap();
        let result = link(&prog, &source_path, std::slice::from_ref(&dir)).unwrap();

        assert_eq!(result.resolved.len(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
