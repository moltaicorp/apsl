
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Located {
    pub file: PathBuf,
    pub line: u32,
    pub block: String,
}

pub fn build_search_path(source_file: &Path, explicit: &[PathBuf]) -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    dirs.extend(explicit.iter().cloned());

    if let Ok(val) = std::env::var("APSL_PATH") {
        for p in val.split(':') {
            let pb = PathBuf::from(p);
            if pb.is_dir() && !dirs.contains(&pb) {
                dirs.push(pb);
            }
        }
    }

    let ws_root = find_workspace_root(source_file);
    let apsl_path_file = ws_root.join(".apsl-path");
    if apsl_path_file.is_file() {
        if let Ok(content) = std::fs::read_to_string(&apsl_path_file) {
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let pb = if Path::new(line).is_absolute() {
                    PathBuf::from(line)
                } else {
                    ws_root.join(line)
                };
                if pb.is_dir() && !dirs.contains(&pb) {
                    dirs.push(pb);
                }
            }
        }
    }

    if let Some(parent) = source_file.parent() {
        let parent = parent.to_path_buf();
        if parent.is_dir() && !dirs.contains(&parent) {
            dirs.push(parent);
        }
    }

    if ws_root.is_dir() && !dirs.contains(&ws_root) {
        dirs.push(ws_root);
    }

    dirs
}

pub fn collect_apsl_files(dirs: &[PathBuf], exclude_file: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let exclude_canon = exclude_file.canonicalize().ok();
    for dir in dirs {
        walk_dir(dir, &mut files, &exclude_canon);
    }
    let mut seen = std::collections::HashSet::new();
    files.retain(|f| {
        let key = f.canonicalize().unwrap_or_else(|_| f.clone());
        seen.insert(key)
    });
    files
}

fn walk_dir(dir: &Path, out: &mut Vec<PathBuf>, exclude: &Option<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str == "target" || name_str == ".git" || name_str == ".apsl-store" {
                continue;
            }
            walk_dir(&path, out, exclude);
        } else if path.is_file() {
            if let Some(ext) = path.extension() {
                if ext == "apsl" {
                    if let Some(ref ex) = exclude {
                        if path.canonicalize().ok().as_ref() == Some(ex) {
                            continue;
                        }
                    }
                    out.push(path);
                }
            }
        }
    }
}

pub fn search_symbol(symbol: &str, files: &[PathBuf]) -> Vec<Located> {
    let mut results = Vec::new();
    for file in files {
        let content = match std::fs::read_to_string(file) {
            Ok(c) => c,
            Err(_) => continue,
        };
        for (line_idx, line) in content.lines().enumerate() {
            if matches_definition(line, symbol) {
                let block = extract_block(&content, line_idx);
                results.push(Located {
                    file: file.clone(),
                    line: (line_idx + 1) as u32,
                    block,
                });
            }
        }
    }
    results
}

fn matches_definition(line: &str, symbol: &str) -> bool {
    if let Some(rest) = line.strip_prefix("type ") {
        let rest = rest.trim_start();
        if rest.starts_with(symbol) {
            let after = &rest[symbol.len()..];
            let after = after.trim_start();
            if after.starts_with('=') || after.is_empty() {
                return true;
            }
        }
        return false;
    }

    if let Some(rest) = line.strip_prefix("graph ") {
        let rest = rest.trim_start();
        if rest.starts_with(symbol) {
            let after = &rest[symbol.len()..];
            let after = after.trim_start();
            if after.starts_with(':') || after.is_empty() {
                return true;
            }
        }
        return false;
    }

    if line.starts_with(symbol) {
        let after = &line[symbol.len()..];
        let after = after.trim_start();
        if after.starts_with(':') {
            return true;
        }
    }

    false
}

fn extract_block(content: &str, start_line: usize) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let mut end = start_line + 1;
    while end < lines.len() {
        if lines[end].trim().is_empty() {
            break;
        }
        end += 1;
    }
    lines[start_line..end].join("\n")
}

fn find_workspace_root(source: &Path) -> PathBuf {
    let mut dir = if source.is_file() {
        source.parent().unwrap_or(Path::new(".")).to_path_buf()
    } else {
        source.to_path_buf()
    };
    loop {
        if dir.join("Cargo.toml").exists()
            || dir.join(".apsl-path").exists()
            || dir.join(".git").exists()
        {
            return dir;
        }
        match dir.parent() {
            Some(parent) if parent != dir => dir = parent.to_path_buf(),
            _ => return source.parent().unwrap_or(Path::new(".")).to_path_buf(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn match_type_alias() {
        assert!(matches_definition("type Email = String", "Email"));
        assert!(matches_definition("type Email=String", "Email"));
        assert!(!matches_definition("type EmailAddr = String", "Email"));
        assert!(!matches_definition("  type Email = String", "Email")); // indented
    }

    #[test]
    fn match_node() {
        assert!(matches_definition("dedupe : Email[] -> Email[]", "dedupe"));
        assert!(matches_definition("dedupe: Email[] -> Email[]", "dedupe"));
        assert!(!matches_definition("  dedupe : Email[] -> Email[]", "dedupe"));
        assert!(!matches_definition("dedupe_v2 : Email[] -> Email[]", "dedupe"));
    }

    #[test]
    fn match_graph() {
        assert!(matches_definition("graph email_pipeline : String[] -> MessageId[]", "email_pipeline"));
        assert!(!matches_definition("graph email_pipeline : String[] -> MessageId[]", "email"));
    }

    #[test]
    fn match_predicate() {
        assert!(matches_definition("valid_email? : String -> Bool", "valid_email?"));
    }

    #[test]
    fn extract_block_basic() {
        let src = "type Email = String\n\ndedupe : Email[] -> Email[]\n  pre   every in valid_email?\n  cx    O(n log n) idem\n\ngraph p : X -> Y\n";
        let block = extract_block(src, 2);
        assert_eq!(block, "dedupe : Email[] -> Email[]\n  pre   every in valid_email?\n  cx    O(n log n) idem");
    }

    #[test]
    fn extract_block_at_eof() {
        let src = "type A = Int\n\nfoo : Int -> Int\n  cx O(1) idem";
        let block = extract_block(src, 2);
        assert_eq!(block, "foo : Int -> Int\n  cx O(1) idem");
    }
}
