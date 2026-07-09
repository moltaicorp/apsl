
use std::path::PathBuf;
use std::process::ExitCode;

use apsl_core::Canon;
use apsl_core::hash::sha256_hex;
use apsl_parse::parse_str;

mod attest;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        usage();
        return ExitCode::from(2);
    }
    let cmd = args[1].as_str();
    match cmd {
        "help" | "--help" | "-h" => { usage(); ExitCode::SUCCESS }
        "check" if args[2..].iter().any(|a| a == "--attest") => {
            attest::run(&attest::AttestOpts::parse(&args[2..]))
        }
        "parse" | "canon" | "hash" | "check" => {
            if args.len() < 3 {
                eprintln!("apslc {}: missing <file>", cmd);
                return ExitCode::from(2);
            }
            let path = &args[2];
            let flags = parse_flags(&args[3..]);
            match run(cmd, path, &flags) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => { eprintln!("{}", e); ExitCode::FAILURE }
            }
        }
        "deploy" => {
            if args.len() < 3 {
                eprintln!("apslc deploy: missing <file>");
                return ExitCode::from(2);
            }
            let path = &args[2];
            let flags = parse_flags(&args[3..]);
            match run_deploy(path, &flags) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => { eprintln!("{}", e); ExitCode::FAILURE }
            }
        }
        other => {
            eprintln!("apslc: unknown subcommand `{}`", other);
            usage();
            ExitCode::from(2)
        }
    }
}

struct Flags {
    search_path: Vec<PathBuf>,
    no_resolve: bool,
    show_deps: bool,
    state_check: bool,
    nominal: bool,
    restricted: bool,
    migrate: bool,
    strict: bool,
    rooted: bool,
}

fn parse_flags(args: &[String]) -> Flags {
    let mut flags = Flags {
        search_path: Vec::new(),
        no_resolve: false,
        show_deps: false,
        migrate: false,
        state_check: false,
        nominal: false,
        restricted: false,
        strict: false,
        rooted: false,
    };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--search-path" => {
                if i + 1 < args.len() {
                    i += 1;
                    for p in args[i].split(':') {
                        flags.search_path.push(PathBuf::from(p));
                    }
                }
            }
            "--no-resolve" => flags.no_resolve = true,
            "--show-deps" => flags.show_deps = true,
            "--state" => flags.state_check = true,
            "--nominal" => flags.nominal = true,
            "--restricted" => { flags.restricted = true; flags.nominal = true; }
            "--migrate" => flags.migrate = true,
            "--strict" => flags.strict = true,
            "--rooted" => flags.rooted = true,
            _ => {}
        }
        i += 1;
    }
    flags
}

fn usage() {
    eprintln!("apslc — APSL compiler\n");
    eprintln!("usage:");
    eprintln!("  apslc parse <file>   print canonical AST to stdout");
    eprintln!("  apslc canon <file>   same — canonical form IS the serialization");
    eprintln!("  apslc hash  <file>   print sha256 hex of canonical form");
    eprintln!("  apslc check <file>   parse + link + type-check, exit 0 if clean");
    eprintln!("  apslc deploy <file>  --deploy mode: emit the GitLab child-pipeline YAML");
    eprintln!("                       (gen.yml) from the CI/CD-definition graph, to stdout");
    eprintln!();
    eprintln!("flags:");
    eprintln!("  --search-path <dirs>  colon-separated directories to search for symbols");
    eprintln!("  --no-resolve          disable linker (error on unresolved symbols)");
    eprintln!("  --show-deps           print resolved dependencies");
    eprintln!("  --state               enforce state clause validation");
    eprintln!("  --nominal             enforce nominal type equality (no structural aliasing)");
    eprintln!("  --restricted          enforce capability narrowing (implies --nominal)");
    eprintln!("  --strict              reject coarse types: every type alias must resolve to a unique structure");
    eprintln!("  --rooted              reject bare World (must use World<S>) and enforce single-root connectedness");
    eprintln!("  --migrate             strip unknown syntax for backward-compatible validation");
    eprintln!();
    eprintln!("  --attest [path]       NO-STRINGS LAW: scan IMPLEMENTATION source under [path]");
    eprintln!("                        (default: git repo root / cwd) for bare/unattested string");
    eprintln!("                        literals; print a per-file remediation worklist; exit 1 on any.");
    eprintln!("    with --attest:");
    eprintln!("      --count              print only the offender count (query mode, exit 0)");
    eprintln!("      --ratchet <file>     fault only on an INCREASE vs the baseline count in <file>");
    eprintln!("      --bless              (with --ratchet) lower the baseline ceiling to the current count");
}

fn run(cmd: &str, path: &str, flags: &Flags) -> Result<(), String> {
    let raw_src = std::fs::read_to_string(path)
        .map_err(|e| format!("apslc: cannot read {}: {}", path, e))?;
    let src = if flags.migrate { migrate_source(&raw_src) } else { raw_src };
    let prog = parse_str(&src).map_err(|e| render_parse_error(&src, &e))?;

    let mut linked_prog = if flags.no_resolve {
        prog
    } else {
        let source_path = std::path::Path::new(path);
        match apsl_link::link(&prog, source_path, &flags.search_path) {
            Ok(result) => {
                if flags.show_deps && !result.resolved.is_empty() {
                    eprintln!("resolved {} external symbol(s):", result.resolved.len());
                    for dep in &result.resolved {
                        eprintln!("  {:<24} <- {}:{}",
                            dep.symbol, dep.file.display(), dep.line);
                    }
                    eprintln!();
                }
                result.program
            }
            Err(e) => return Err(format!("{}", e)),
        }
    };

    if cmd == "check" {
        match apsl_types::type_check(&linked_prog) {
            Ok(_) => {}
            Err(errs) => {
                let mut msg = String::new();
                for e in errs {
                    msg.push_str(&render_type_error(&src, &e));
                    msg.push('\n');
                }
                return Err(msg);
            }
        }

        if flags.state_check {
            let state_result = check_state(&mut linked_prog);
            for h in &state_result.hoists {
                eprintln!("{}", h);
            }
            if !state_result.errors.is_empty() {
                let mut msg = String::new();
                for e in &state_result.errors {
                    msg.push_str(e);
                    msg.push('\n');
                }
                return Err(msg);
            }
            if !state_result.warnings.is_empty() {
                for w in &state_result.warnings {
                    eprintln!("warning: {}", w);
                }
            }
            if !state_result.info.is_empty() {
                for i in &state_result.info {
                    eprintln!("state: {}", i);
                }
            }
        }

        if flags.nominal {
            let nominal_errors = check_nominal(&linked_prog, flags.restricted);
            if !nominal_errors.is_empty() {
                let mut msg = String::new();
                for e in &nominal_errors {
                    msg.push_str(e);
                    msg.push('\n');
                }
                return Err(msg);
            }
        }

        if flags.rooted {
            let rooted_errors = check_rooted(&linked_prog);
            if !rooted_errors.is_empty() {
                let mut msg = String::new();
                for e in &rooted_errors {
                    msg.push_str(e);
                    msg.push('\n');
                }
                return Err(msg);
            }
        }

        if flags.strict {
            use std::collections::HashMap;
            let mut alias_to_structure: HashMap<String, String> = HashMap::new();
            let mut structure_to_aliases: HashMap<String, Vec<String>> = HashMap::new();
            let base_types = ["Int", "Rat", "Bool", "String", "Float", "Real"];

            for d in &linked_prog.decls {
                if let apsl_core::ast::Decl::Type(ta) = d {
                    let name = ta.name.as_str().to_string();
                    let structure = ta.rhs.canon();
                    alias_to_structure.insert(name.clone(), structure.clone());
                    structure_to_aliases.entry(structure).or_default().push(name);
                }
            }

            let mut collisions = Vec::new();
            for (structure, aliases) in &structure_to_aliases {
                if aliases.len() > 1 {
                    let non_trivial: Vec<&String> = aliases.iter()
                        .filter(|a| !base_types.contains(&a.as_str()))
                        .collect();
                    if non_trivial.len() > 1 {
                        collisions.push((structure.clone(), non_trivial.iter().map(|s| s.to_string()).collect::<Vec<_>>()));
                    }
                }
            }

            if !collisions.is_empty() {
                let mut msg = format!("apslc --strict: {} type collision(s) — different names resolve to same structure:\n", collisions.len());
                for (structure, names) in &collisions {
                    msg.push_str(&format!("  {} all resolve to {}\n", names.join(", "), structure));
                }
                msg.push_str("\nhint: types are too coarse. decompose further until each proposition has a unique structural type.\n");
                return Err(msg);
            }

            let total_aliases = alias_to_structure.len();
            println!("ok (strict: {}/{} type aliases structurally unique)", total_aliases, total_aliases);
            return Ok(());
        }

        println!("ok");
        return Ok(());
    }

    let canon = linked_prog.canon();
    match cmd {
        "parse" | "canon" => { println!("{}", canon); }
        "hash" => { println!("{}", sha256_hex(canon.as_bytes())); }
        _ => unreachable!(),
    }
    Ok(())
}

fn render_parse_error(src: &str, e: &apsl_parse::ParseError) -> String {
    let mut s = String::new();
    s.push_str(&format!("apslc: parse error at line {} col {}\n  {}\n",
        e.span.line, e.span.col, e.msg));
    if let Some(line) = src.lines().nth(e.span.line.saturating_sub(1) as usize) {
        s.push_str(&format!("  | {}\n", line));
        let pad = " ".repeat(e.span.col.saturating_sub(1) as usize);
        s.push_str(&format!("  | {}^\n", pad));
    }
    s
}

fn render_type_error(src: &str, e: &apsl_types::TypeError) -> String {
    let mut s = String::new();
    s.push_str(&format!("apslc: type error at line {} col {}\n  {}\n",
        e.span.line, e.span.col, e.msg));
    if let Some(line) = src.lines().nth(e.span.line.saturating_sub(1) as usize) {
        s.push_str(&format!("  | {}\n", line));
        let pad = " ".repeat(e.span.col.saturating_sub(1) as usize);
        s.push_str(&format!("  | {}^\n", pad));
    }
    s
}

use apsl_core::ast::{Decl, Node, Graph, StateDecl};
use std::collections::{HashMap, HashSet};

struct StateCheckResult {
    errors: Vec<String>,
    warnings: Vec<String>,
    info: Vec<String>,
    hoists: Vec<String>,
}

enum HoistTarget {
    Node(String),
    GraphRoot(String),
}

struct HoistAction {
    key: String,
    decl: StateDecl,
    target: HoistTarget,
    sharing: Vec<String>,
    removed_from: Vec<String>,
}

fn check_state(prog: &mut apsl_core::Program) -> StateCheckResult {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let mut info = Vec::new();
    let mut hoists = Vec::new();

    let mut node_states: HashMap<String, Vec<StateDecl>> = HashMap::new();
    let mut graphs: Vec<(String, Vec<Vec<Vec<String>>>)> = Vec::new();
    for decl in &prog.decls {
        match decl {
            Decl::Node(n) => { node_states.insert(n.name.as_str().to_string(), n.state.clone()); }
            Decl::Graph(g) => {
                let chains: Vec<Vec<Vec<String>>> = g.flow.iter().map(|chain| {
                    chain.iter()
                        .map(|step| step.nodes.iter().map(|id| id.as_str().to_string()).collect())
                        .collect()
                }).collect();
                graphs.push((g.name.as_str().to_string(), chains));
            }
            _ => {}
        }
    }

    let mut actions: Vec<HoistAction> = Vec::new();
    for (graph_name, chains) in &graphs {
        let (preds, all_nodes) = flow_dag(chains);
        let dom = dominators(&preds, &all_nodes);

        let mut graph_nodes: HashSet<String> = HashSet::new();
        for chain in chains {
            for step in chain {
                for n in step {
                    if n != "in" && n != "out" { graph_nodes.insert(n.clone()); }
                }
            }
        }
        let mut key_nodes: HashMap<String, Vec<String>> = HashMap::new();
        for node_name in &graph_nodes {
            if let Some(states) = node_states.get(node_name) {
                for sd in states {
                    key_nodes.entry(sd.key.as_str().to_string())
                        .or_default().push(node_name.clone());
                }
            }
        }

        let mut keys: Vec<&String> = key_nodes.keys().collect();
        keys.sort();
        for key in keys {
            let mut sharing = key_nodes[key].clone();
            sharing.sort();
            sharing.dedup();
            if sharing.len() < 2 { continue; }

            let decls: Vec<StateDecl> = sharing.iter().filter_map(|n| {
                node_states.get(n)
                    .and_then(|s| s.iter().find(|sd| sd.key.as_str() == key).cloned())
            }).collect();
            if decls.is_empty() { continue; }
            let first = decls[0].clone();
            let incoherent = decls.iter().any(|d| d.ty != first.ty || d.default != first.default);
            if incoherent {
                let mut kinds: Vec<String> = sharing.iter().zip(decls.iter())
                    .map(|(n, d)| format!("{}: {}", n, type_to_nominal_name(&d.ty)))
                    .collect();
                kinds.sort();
                errors.push(format!(
                    "state: graph `{}`: key `{}` is declared with incompatible types across siblings ({}).\n\
                     \x20 No coherent lowest-common-ancestor placement exists — reconcile the declared types, then re-run.",
                    graph_name, key, kinds.join(", ")
                ));
                continue;
            }

            let lca = lca_of(&dom, &sharing);
            let target = if lca == "in" {
                HoistTarget::GraphRoot(graph_name.clone())
            } else {
                HoistTarget::Node(lca)
            };
            let removed_from: Vec<String> = match &target {
                HoistTarget::Node(t) => sharing.iter().filter(|n| *n != t).cloned().collect(),
                HoistTarget::GraphRoot(_) => sharing.clone(),
            };
            actions.push(HoistAction { key: key.clone(), decl: first, target, sharing, removed_from });
        }
    }

    for act in &actions {
        for d in prog.decls.iter_mut() {
            if let Decl::Node(n) = d {
                if act.removed_from.iter().any(|r| r == n.name.as_str()) {
                    n.state.retain(|sd| sd.key.as_str() != act.key);
                }
            }
        }
        match &act.target {
            HoistTarget::Node(name) => {
                for d in prog.decls.iter_mut() {
                    if let Decl::Node(n) = d {
                        if n.name.as_str() == name
                            && !n.state.iter().any(|sd| sd.key.as_str() == act.key) {
                            n.state.push(act.decl.clone());
                        }
                    }
                }
            }
            HoistTarget::GraphRoot(gname) => {
                for d in prog.decls.iter_mut() {
                    if let Decl::Graph(g) = d {
                        if g.name.as_str() == gname
                            && !g.state.iter().any(|sd| sd.key.as_str() == act.key) {
                            g.state.push(act.decl.clone());
                        }
                    }
                }
            }
        }

        let target_label = match &act.target {
            HoistTarget::Node(t) => format!("`{}`", t),
            HoistTarget::GraphRoot(g) => format!("graph root `{}`", g),
        };
        let removed_label = if act.removed_from.len() == 2 {
            "both".to_string()
        } else {
            act.removed_from.join(", ")
        };
        hoists.push(format!(
            "apslc: hoisted `state {} : {}` to {} (LCA of {}); removed from {}",
            act.key, type_to_nominal_name(&act.decl.ty), target_label,
            act.sharing.join(", "), removed_label
        ));
    }

    let mut node_map: HashMap<String, &Node> = HashMap::new();
    let mut graph_refs: Vec<&Graph> = Vec::new();
    for decl in &prog.decls {
        match decl {
            Decl::Node(n) => { node_map.insert(n.name.as_str().to_string(), n); }
            Decl::Graph(g) => { graph_refs.push(g); }
            _ => {}
        }
    }

    for graph in &graph_refs {
        let mut seen: HashSet<String> = HashSet::new();
        for chain in &graph.flow {
            for step in chain {
                for id in &step.nodes {
                    let name = id.as_str();
                    if name == "in" || name == "out" { continue; }
                    if !seen.insert(name.to_string()) { continue; }
                    if let Some(node) = node_map.get(name) {
                        let has_external = node.via.as_ref()
                            .map_or(false, |v| v.tag.as_str().contains("external"));
                        if has_external && node.state.is_empty() {
                            warnings.push(format!(
                                "state: graph `{}`: node `{}` has `via @external` but no state declarations.\n\
                                 \x20 External service nodes typically depend on configuration (endpoints, credentials).\n\
                                 \x20 Add `state <key> : <Type>` clauses for each config dependency.",
                                graph.name.as_str(), name
                            ));
                        }
                    }
                }
            }
        }
    }

    let mut global_key_nodes: HashMap<String, Vec<String>> = HashMap::new();
    for decl in &prog.decls {
        match decl {
            Decl::Node(n) => for sd in &n.state {
                global_key_nodes.entry(sd.key.as_str().to_string())
                    .or_default().push(n.name.as_str().to_string());
            },
            Decl::Graph(g) => for sd in &g.state {
                global_key_nodes.entry(sd.key.as_str().to_string())
                    .or_default().push(format!("{} (root)", g.name.as_str()));
            },
            _ => {}
        }
    }

    let mut shared_keys: Vec<(&String, &Vec<String>)> = global_key_nodes.iter()
        .filter(|(_, nodes)| nodes.len() > 1)
        .collect();
    shared_keys.sort_by_key(|(k, _)| (*k).clone());

    let mut unique_keys: Vec<(&String, &Vec<String>)> = global_key_nodes.iter()
        .filter(|(_, nodes)| nodes.len() == 1)
        .collect();
    unique_keys.sort_by_key(|(k, _)| (*k).clone());

    if !shared_keys.is_empty() || !unique_keys.is_empty() {
        info.push(format!("state: {} total keys ({} shared, {} unique)",
            global_key_nodes.len(), shared_keys.len(), unique_keys.len()));
    }

    for (key, nodes) in &shared_keys {
        let mut sorted = (*nodes).clone();
        sorted.sort();
        info.push(format!("state: shared `{}` across: {}", key, sorted.join(", ")));
    }

    for (key, nodes) in &unique_keys {
        info.push(format!("state: unique `{}` in: {}", key, &nodes[0]));
    }

    StateCheckResult { errors, warnings, info, hoists }
}

fn flow_dag(chains: &[Vec<Vec<String>>]) -> (HashMap<String, HashSet<String>>, HashSet<String>) {
    let mut preds: HashMap<String, HashSet<String>> = HashMap::new();
    let mut all: HashSet<String> = HashSet::new();
    all.insert("in".to_string());
    for chain in chains {
        for n in chain.iter().flatten() {
            if n != "out" { all.insert(n.clone()); }
        }
        for w in chain.windows(2) {
            for a in &w[0] {
                if a == "out" { continue; }
                for b in &w[1] {
                    if b == "out" { continue; }
                    preds.entry(b.clone()).or_default().insert(a.clone());
                }
            }
        }
    }
    (preds, all)
}

fn dominators(
    preds: &HashMap<String, HashSet<String>>,
    all: &HashSet<String>,
) -> HashMap<String, HashSet<String>> {
    let source = "in";
    let mut dom: HashMap<String, HashSet<String>> = HashMap::new();
    for n in all { dom.insert(n.clone(), all.clone()); }
    dom.insert(source.to_string(), HashSet::from([source.to_string()]));
    let mut changed = true;
    while changed {
        changed = false;
        for n in all {
            if n.as_str() == source { continue; }
            let new = match preds.get(n) {
                Some(ps) if !ps.is_empty() => {
                    let mut it = ps.iter();
                    let mut acc = dom[it.next().unwrap()].clone();
                    for p in it {
                        acc = acc.intersection(&dom[p]).cloned().collect();
                    }
                    acc.insert(n.clone());
                    acc
                }
                _ => HashSet::from([n.clone()]),
            };
            if new != dom[n] { dom.insert(n.clone(), new); changed = true; }
        }
    }
    dom
}

fn lca_of(dom: &HashMap<String, HashSet<String>>, sharing: &[String]) -> String {
    let mut it = sharing.iter();
    let mut common = match it.next().and_then(|s| dom.get(s)) {
        Some(d) => d.clone(),
        None => return "in".to_string(),
    };
    for s in it {
        if let Some(d) = dom.get(s) {
            common = common.intersection(d).cloned().collect();
        }
    }
    common.into_iter().max_by_key(|c| dom[c].len()).unwrap_or_else(|| "in".to_string())
}




fn check_rooted(prog: &apsl_core::Program) -> Vec<String> {
    let mut errors = Vec::new();
    use apsl_core::ast::Type;

   
    fn type_has_bare_world(ty: &Type) -> bool {
        match ty {
            Type::Base(id) => id.as_str() == "World",
            Type::Parameterized(name, _) => name.as_str() == "World",
            Type::List(inner) => type_has_bare_world(inner),
            Type::Tuple(ts) => ts.iter().any(type_has_bare_world),
            Type::Record(fs) => fs.iter().any(|(_, t)| type_has_bare_world(t)),
            Type::Result(inner) => type_has_bare_world(inner),
            Type::Var(_) => false,
        }
    }

    fn type_uses_bare_world_not_param(ty: &Type) -> bool {
        match ty {
            Type::Base(id) => id.as_str() == "World",
            Type::Parameterized(name, args) => {
                name.as_str() == "World" && args.iter().any(type_uses_bare_world_not_param)
                    || args.iter().any(type_uses_bare_world_not_param)
            }
            Type::List(inner) => type_uses_bare_world_not_param(inner),
            Type::Tuple(ts) => ts.iter().any(type_uses_bare_world_not_param),
            Type::Record(fs) => fs.iter().any(|(_, t)| type_uses_bare_world_not_param(t)),
            Type::Result(inner) => type_uses_bare_world_not_param(inner),
            Type::Var(_) => false,
        }
    }

    for decl in &prog.decls {
        if let apsl_core::ast::Decl::Node(n) = decl {
            for p in &n.sig.params {
                if type_uses_bare_world_not_param(&p.ty) {
                    errors.push(format!(
                        "node {} threads bare World without RootState \
                         \x20 — annotate as World<Filename> or it cannot compose with any rooted spec.\
                         \x20 Fix all bare World in your link path, not just this one.",
                        n.name.as_str()
                    ));
                }
            }
            if type_uses_bare_world_not_param(&n.sig.ret) {
                errors.push(format!(
                    "node {} threads bare World without RootState \
                     \x20 — annotate as World<Filename> or it cannot compose with any rooted spec.\
                     \x20 Fix all bare World in your link path, not just this one.",
                    n.name.as_str()
                ));
            }
        }
    }

   
    let mut node_names: HashSet<String> = HashSet::new();
    let mut edges: HashMap<String, HashSet<String>> = HashMap::new();
    let mut all_nodes: HashSet<String> = HashSet::new();

    for decl in &prog.decls {
        if let apsl_core::ast::Decl::Node(n) = decl {
            let name = n.name.as_str().to_string();
            node_names.insert(name.clone());
            all_nodes.insert(name.clone());
        }
    }

    for decl in &prog.decls {
        if let apsl_core::ast::Decl::Graph(g) = decl {
            for chain in &g.flow {
                for w in chain.windows(2) {
                    for a in &w[0].nodes {
                        let a_name = a.as_str();
                        if a_name == "in" || a_name == "out" { continue; }
                        for b in &w[1].nodes {
                            let b_name = b.as_str();
                            if b_name == "in" || b_name == "out" { continue; }
                            if !all_nodes.contains(a_name) { all_nodes.insert(a_name.to_string()); }
                            if !all_nodes.contains(b_name) { all_nodes.insert(b_name.to_string()); }
                            edges.entry(a_name.to_string()).or_default().insert(b_name.to_string());
                        }
                    }
                }
            }
        }
    }

   
    let mut visited: HashSet<String> = HashSet::new();
    let mut components: Vec<Vec<String>> = Vec::new();
    let mut undirected: HashMap<String, HashSet<String>> = HashMap::new();
    for (a, bs) in &edges {
        undirected.entry(a.clone()).or_default().extend(bs.iter().cloned());
        for b in bs {
            undirected.entry(b.clone()).or_default().insert(a.clone());
        }
    }
    for node in &all_nodes {
        undirected.entry(node.clone()).or_default();
    }
    for node in &all_nodes {
        if visited.contains(node) { continue; }
        let mut comp: Vec<String> = Vec::new();
        let mut stack = vec![node.clone()];
        while let Some(n) = stack.pop() {
            if !visited.insert(n.clone()) { continue; }
            comp.push(n.clone());
            if let Some(neighbors) = undirected.get(&n) {
                for nb in neighbors {
                    if !visited.contains(nb) { stack.push(nb.clone()); }
                }
            }
        }
        comp.sort();
        components.push(comp);
    }

    if components.len() > 1 {
        let mut msg = format!(
            "apslc --rooted: {} disconnected component(s) found — nodes must form a single weakly-connected DAG:\n",
            components.len()
        );
        for (i, comp) in components.iter().enumerate() {
            msg.push_str(&format!("  component {}: {}\n", i + 1, comp.join(", ")));
        }
        errors.push(msg);
    }

    errors
}

fn check_nominal(prog: &apsl_core::Program, restricted: bool) -> Vec<String> {
    let mut errors = Vec::new();

    let mut alias_rhs: HashMap<String, String> = HashMap::new();
    let mut subtype_parents: HashMap<String, HashSet<String>> = HashMap::new();
    let mut alias_names: HashSet<String> = HashSet::new();

    for decl in &prog.decls {
        if let Decl::Type(ta) = decl {
            alias_names.insert(ta.name.as_str().to_string());
            for sup in &ta.supertypes {
                subtype_parents
                    .entry(ta.name.as_str().to_string())
                    .or_default()
                    .insert(sup.as_str().to_string());
            }
            if let apsl_core::ast::Type::Base(rhs_ident) = &ta.rhs {
                if rhs_ident.as_str() != ta.name.as_str() {
                    alias_rhs.insert(ta.name.as_str().to_string(), rhs_ident.as_str().to_string());
                }
            }
        }
    }

    let is_subtype_of = |child: &str, parent: &str| -> bool {
        if child == parent { return true; }
        let mut visited = HashSet::new();
        let mut queue = vec![child.to_string()];
        while let Some(current) = queue.pop() {
            if current == parent { return true; }
            if visited.contains(&current) { continue; }
            visited.insert(current.clone());
            if let Some(parents) = subtype_parents.get(&current) {
                for p in parents {
                    queue.push(p.clone());
                }
            }
        }
        false
    };

    let mut node_sigs: HashMap<String, (Vec<String>, String)> = HashMap::new();
    for decl in &prog.decls {
        if let Decl::Node(n) = decl {
            let input_names: Vec<String> = n.sig.params.iter().map(|p| {
                type_to_nominal_name(&p.ty)
            }).collect();
            let output_name = type_to_nominal_name(&n.sig.ret);
            node_sigs.insert(n.name.as_str().to_string(), (input_names, output_name));
        }
    }

    for decl in &prog.decls {
        if let Decl::Graph(g) = decl {
            for chain in &g.flow {
                for i in 0..chain.len().saturating_sub(1) {
                    let from_step = &chain[i];
                    let to_step = &chain[i + 1];

                    for from_node_id in &from_step.nodes {
                        let from_name = from_node_id.as_str();
                        if from_name == "in" || from_name == "out" { continue; }

                        for to_node_id in &to_step.nodes {
                            let to_name = to_node_id.as_str();
                            if to_name == "in" || to_name == "out" { continue; }

                            let from_output = node_sigs.get(from_name).map(|(_, o)| o.clone());
                            let to_input = node_sigs.get(to_name).map(|(inputs, _)| {
                                inputs.first().cloned().unwrap_or_default()
                            });

                            if let (Some(out_ty), Some(in_ty)) = (from_output, to_input) {
                                let out_is_alias = alias_names.contains(&out_ty);
                                let in_is_alias = alias_names.contains(&in_ty);

                                if out_is_alias && in_is_alias && out_ty != in_ty {
                                    if restricted {
                                        if !is_subtype_of(&out_ty, &in_ty) {
                                            errors.push(format!(
                                                "apslc: nominal type error in graph `{}`\n                                                   flow edge {} -> {}: capability widening:                                                  step outputs `{}` but next step expects `{}`\n                                                   `{}` is not a subtype of `{}`",
                                                g.name.as_str(), from_name, to_name,
                                                out_ty, in_ty, out_ty, in_ty
                                            ));
                                        }
                                    } else {
                                        errors.push(format!(
                                            "apslc: nominal type error in graph `{}`\n                                               flow edge {} -> {}: nominal type mismatch:                                              step outputs `{}` but next step expects `{}`",
                                            g.name.as_str(), from_name, to_name,
                                            out_ty, in_ty
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    errors
}

fn type_to_nominal_name(ty: &apsl_core::ast::Type) -> String {
    match ty {
        apsl_core::ast::Type::Base(id) => id.as_str().to_string(),
        apsl_core::ast::Type::Parameterized(name, args) => {
            let parts: Vec<String> = args.iter().map(|t| type_to_nominal_name(t)).collect();
            format!("{}<{}>", name.as_str(), parts.join(", "))
        }
        apsl_core::ast::Type::List(inner) => format!("{}[]", type_to_nominal_name(inner)),
        apsl_core::ast::Type::Tuple(ts) => {
            let parts: Vec<String> = ts.iter().map(|t| type_to_nominal_name(t)).collect();
            format!("({})", parts.join(", "))
        }
        apsl_core::ast::Type::Var(v) => format!("?{}", v),
        apsl_core::ast::Type::Result(inner) => format!("Result<{}>", type_to_nominal_name(inner)),
        apsl_core::ast::Type::Record(fields) => {
            let parts: Vec<String> = fields.iter()
                .map(|(name, ty)| format!("{}: {}", name.as_str(), type_to_nominal_name(ty)))
                .collect();
            format!("{{ {} }}", parts.join(", "))
        }
    }
}


fn run_deploy(path: &str, flags: &Flags) -> Result<(), String> {
    let src = std::fs::read_to_string(path)
        .map_err(|e| format!("apslc: cannot read {}: {}", path, e))?;
    let prog = parse_str(&src).map_err(|e| render_parse_error(&src, &e))?;
    let source_path = std::path::Path::new(path);
    let linked = if flags.no_resolve {
        prog
    } else {
        match apsl_link::link(&prog, source_path, &flags.search_path) {
            Ok(r) => r.program,
            Err(e) => return Err(format!("{}", e)),
        }
    };
    apsl_types::type_check(&linked).map_err(|errs| {
        let mut msg = String::new();
        for e in errs { msg.push_str(&render_type_error(&src, &e)); msg.push('\n'); }
        msg
    })?;
    let hash = sha256_hex(linked.canon().as_bytes());
    let yaml = emit_gitlab_child(&linked, path, &hash)?;
    print!("{}", yaml);
    Ok(())
}

fn load_state_manifest(apsl_path: &str) -> Result<HashMap<String, String>, String> {
    let dir = std::path::Path::new(apsl_path).parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let mpath = dir.join("cicd-state.yaml");
    let txt = std::fs::read_to_string(&mpath).map_err(|e| format!(
        "apslc deploy: cannot read state manifest {}: {} (strings are state — provide it)",
        mpath.display(), e))?;
    let mut m = HashMap::new();
    for line in txt.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') { continue; }
        if let Some(idx) = t.find(':') {
            let k = t[..idx].trim().to_string();
            let v = t[idx + 1..].trim().to_string();
            if !k.is_empty() && !v.is_empty() { m.insert(k, v); }
        }
    }
    Ok(m)
}

fn via_service(n: &Node) -> Option<String> {
    n.via.as_ref().and_then(|v| {
        v.attrs.iter().find(|(k, _)| k.as_str() == "service").map(|(_, val)| val.as_str().to_string())
    })
}

fn expr_to_str(e: &apsl_core::ast::Expr) -> String {
    use apsl_core::ast::{BinOp, Expr, Lit, UnOp};
    match e {
        Expr::Var(id, _) => id.as_str().to_string(),
        Expr::Lit(Lit::Bool(b), _) => b.to_string(),
        Expr::Lit(Lit::Int(n), _) => n.to_string(),
        Expr::Lit(Lit::Str(s), _) => s.clone(),
        Expr::Lit(Lit::Rat(p, q), _) => format!("{}/{}", p, q),
        Expr::Un(UnOp::Not, x, _) => format!("not {}", expr_to_str(x)),
        Expr::Un(UnOp::Neg, x, _) => format!("-{}", expr_to_str(x)),
        Expr::Bin(op, l, r, _) => {
            let o = match op {
                BinOp::And => "and", BinOp::Or => "or",
                BinOp::Eq => "=", BinOp::Ne => "!=",
                BinOp::Lt => "<", BinOp::Le => "<=", BinOp::Gt => ">", BinOp::Ge => ">=",
                BinOp::Add => "+", BinOp::Sub => "-", BinOp::Mul => "*", BinOp::Div => "/",
                BinOp::Mod => "mod", BinOp::Subset => "subset",
                BinOp::Union => "union", BinOp::Intersect => "intersect",
            };
            format!("{} {} {}", expr_to_str(l), o, expr_to_str(r))
        }
        Expr::Apply(f, args, _) => {
            let a: Vec<String> = args.iter().map(expr_to_str).collect();
            format!("{} {}", f.as_str(), a.join(" "))
        }
        _ => "<expr>".to_string(),
    }
}

fn emit_gitlab_child(prog: &apsl_core::Program, apsl_path: &str, hash: &str) -> Result<String, String> {
    let mut nodes: HashMap<String, &Node> = HashMap::new();
    let mut graph: Option<&Graph> = None;
    for d in &prog.decls {
        match d {
            Decl::Node(n) => { nodes.insert(n.name.as_str().to_string(), n); }
            Decl::Graph(g) => { if graph.is_none() { graph = Some(g); } }
            _ => {}
        }
    }
    let graph = graph.ok_or("apslc deploy: no graph in spec")?;

    let mut order: Vec<String> = Vec::new();
    if let Some(chain) = graph.flow.first() {
        for step in chain {
            for nid in &step.nodes {
                let nm = nid.as_str();
                if nm == "in" || nm == "out" { continue; }
                order.push(nm.to_string());
            }
        }
    }
    if order.is_empty() {
        return Err("apslc deploy: graph has no flow nodes".into());
    }

    let mut stages: Vec<String> = Vec::new();
    for nm in &order {
        let n = nodes.get(nm).ok_or_else(|| format!("flow references unknown node `{}`", nm))?;
        let dep = n.deploy.as_ref().ok_or_else(|| format!("node `{}` has no --deploy clauses", nm))?;
        let stage = dep.stage.as_ref().ok_or_else(|| format!("node `{}` has no `stage` clause", nm))?.as_str().to_string();
        if !stages.contains(&stage) { stages.push(stage); }
    }

    let mut y = String::new();
    let bar = "# ---------------------------------------------------------------------------";
    y.push_str(bar); y.push('\n');
    y.push_str(&format!("# GENERATED by `apslc deploy` from {}\n", apsl_path));
    y.push_str(&format!("# graph: {}   source hash: {}\n", graph.name.as_str(), hash));
    y.push_str("# DO NOT EDIT. Source of truth is the APSL graph; regenerated every run from\n");
    y.push_str("# the delegation-locked toolchain. Editing this file is a NO-FAKES violation.\n");
    y.push_str(bar); y.push('\n');
    y.push_str("\ninclude:\n  - local: ci/impls.yml\n\n");
    y.push_str("stages:\n");
    for s in &stages { y.push_str(&format!("  - {}\n", s)); }
    y.push('\n');

    eprintln!("[apslc deploy] graph={} hash={}", graph.name.as_str(), hash);
    eprintln!("[apslc deploy] stages: {}", stages.join(" -> "));

    for nm in &order {
        let n = nodes[nm];
        let dep = n.deploy.as_ref().unwrap();
        let stage = dep.stage.as_ref().unwrap().as_str();
        let needs: Vec<String> = dep.needs.iter().map(|i| i.as_str().to_string()).collect();
        let service = via_service(n).unwrap_or_else(|| "-".to_string());

        y.push_str(&format!("{}:\n", nm));
        y.push_str(&format!("  stage: {}\n", stage));
        if needs.is_empty() {
            y.push_str("  needs: []\n");
        } else {
            y.push_str(&format!("  needs: [{}]\n", needs.join(", ")));
        }
        y.push_str(&format!("  extends: .{}_impl\n", nm));
        y.push('\n');

        eprintln!("[apslc deploy] job {} -> stage={} needs=[{}] extends=.{}_impl via={}",
            nm, stage, needs.join(","), nm, service);
    }
    eprintln!("[apslc deploy] {} jobs emitted; root proof {}",
        order.len(), type_to_nominal_name(&graph.sig.ret));
    Ok(y)
}

fn migrate_source(src: &str) -> String {
    const KNOWN_CLAUSES: &[&str] = &[
        "pre", "post", "cx", "sla", "via", "auth", "scope", "audit", "state", "flow",
    ];
    let mut out = String::with_capacity(src.len());
    for line in src.lines() {
        let trimmed = line.trim_start();
        let is_indented = trimmed.len() < line.len();
        if line.starts_with("type ") && line.contains("<:") {
            if let Some(eq_pos) = line.find('=') {
                let name_end = line.find("<:").unwrap();
                let name = line[5..name_end].trim();
                let rhs = line[eq_pos+1..].trim();
                out.push_str(&format!("type {} = {}", name, rhs));
            } else {
                out.push_str(&format!("# [migrate] stripped: {}", line));
            }
        } else if is_indented
            && !trimmed.is_empty()
            && !trimmed.starts_with('#')
            && !trimmed.starts_with("->")
        {
            let head = trimmed
                .split(|c: char| c.is_whitespace() || c == '(' || c == ':')
                .next()
                .unwrap_or("");
            if KNOWN_CLAUSES.contains(&head) {
                out.push_str(line);
            } else {
                out.push_str(&format!("# [migrate] stripped: {}", line));
            }
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use apsl_core::ast::{
        AuditReq, AuthLevel, CxExpr, CxSpec, FlowStep, Ident, Param, RuntimeClass,
        ScopeConstraint, Span, Type, TypeSig,
    };
    use apsl_core::Canon;

    fn world_a_sig() -> TypeSig {
        TypeSig {
            params: vec![
                Param { name: Ident::new("w"), ty: Type::Base(Ident::new("World")) },
                Param { name: Ident::new("x"), ty: Type::Base(Ident::new("A")) },
            ],
            ret: Type::Tuple(vec![Type::Base(Ident::new("World")), Type::Base(Ident::new("A"))]),
        }
    }

    fn node(name: &str, state: Vec<StateDecl>) -> Node {
        Node {
            name: Ident::new(name),
            sig: world_a_sig(),
            pre: vec![], post: vec![],
            cx: CxSpec { bigo: CxExpr::Const, class: RuntimeClass::Idem },
            sla: None, via: None,
            auth: AuthLevel::None,
            scope_constraint: ScopeConstraint::Any,
            audit_req: AuditReq::None,
            state,
            deploy: None,
            span: Span::NONE,
        }
    }

    fn budget() -> StateDecl {
        StateDecl {
            key: Ident::new("budget"),
            ty: Type::Base(Ident::new("Config")),
            default: None,
            span: Span::NONE,
        }
    }

    fn step(name: &str) -> FlowStep { FlowStep::single(Ident::new(name), Span::NONE) }

    #[test]
    fn hoists_sibling_state_to_lca() {
        let mut prog = apsl_core::Program::new();
        prog.decls.push(Decl::Node(node("parent", vec![])));
        prog.decls.push(Decl::Node(node("child_left", vec![budget()])));
        prog.decls.push(Decl::Node(node("child_right", vec![budget()])));
        prog.decls.push(Decl::Graph(Graph {
            name: Ident::new("pipeline"),
            sig: world_a_sig(),
            post: vec![],
            flow: vec![
                vec![step("in"), step("parent"), step("child_left"), step("out")],
                vec![step("parent"), step("child_right"), step("out")],
            ],
            state: vec![],
            span: Span::NONE,
        }));

        let res = check_state(&mut prog);

        assert!(res.errors.is_empty(), "unexpected errors: {:?}", res.errors);
        assert_eq!(res.hoists.len(), 1, "expected exactly one hoist");
        assert_eq!(
            res.hoists[0],
            "apslc: hoisted `state budget : Config` to `parent` (LCA of child_left, child_right); removed from both"
        );

        for d in &prog.decls {
            if let Decl::Node(n) = d {
                let has = n.state.iter().any(|s| s.key.as_str() == "budget");
                match n.name.as_str() {
                    "parent" => assert!(has, "parent should own budget after hoist"),
                    other => assert!(!has, "{} should have lost budget", other),
                }
            }
        }
    }

    #[test]
    fn no_sibling_dup_is_byte_identical() {
        let mut prog = apsl_core::Program::new();
        prog.decls.push(Decl::Node(node("solo", vec![budget()])));
        prog.decls.push(Decl::Graph(Graph {
            name: Ident::new("g"),
            sig: world_a_sig(),
            post: vec![],
            flow: vec![vec![step("in"), step("solo"), step("out")]],
            state: vec![],
            span: Span::NONE,
        }));

        let before = prog.canon();
        let res = check_state(&mut prog);
        let after = prog.canon();

        assert!(res.hoists.is_empty(), "no hoist expected");
        assert!(res.errors.is_empty(), "no error expected");
        assert_eq!(before, after, "canonical form (and hash) must be unchanged");
    }

    #[test]
    fn incompatible_types_fault() {
        let mut prog = apsl_core::Program::new();
        let cfg = StateDecl {
            key: Ident::new("budget"), ty: Type::Base(Ident::new("Config")),
            default: None, span: Span::NONE,
        };
        let money = StateDecl {
            key: Ident::new("budget"), ty: Type::Base(Ident::new("Money")),
            default: None, span: Span::NONE,
        };
        prog.decls.push(Decl::Node(node("parent", vec![])));
        prog.decls.push(Decl::Node(node("child_left", vec![cfg])));
        prog.decls.push(Decl::Node(node("child_right", vec![money])));
        prog.decls.push(Decl::Graph(Graph {
            name: Ident::new("pipeline"),
            sig: world_a_sig(),
            post: vec![],
            flow: vec![
                vec![step("in"), step("parent"), step("child_left"), step("out")],
                vec![step("parent"), step("child_right"), step("out")],
            ],
            state: vec![],
            span: Span::NONE,
        }));

        let res = check_state(&mut prog);
        assert!(res.hoists.is_empty(), "must not hoist an incoherent key");
        assert_eq!(res.errors.len(), 1, "expected exactly one fault");
        assert!(res.errors[0].contains("incompatible types"));
    }
}
