use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use apsl_core::hash::sha256_hex;
use apsl_core::Canon;
use apsl_parse::parse_str;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        usage();
        return ExitCode::from(2);
    }
    let cmd = args[1].as_str();
    match cmd {
        "help" | "--help" | "-h" => {
            usage();
            ExitCode::SUCCESS
        }
        "parse" | "canon" | "hash" | "check" | "compile" => {
            if args.len() < 3 {
                eprintln!("apslc {}: missing <file>", cmd);
                return ExitCode::from(2);
            }
            let path = &args[2];
            let flags = match parse_flags(&args[3..]) {
                Ok(flags) => flags,
                Err(error) => {
                    eprintln!("apslc {cmd}: {error}");
                    return ExitCode::from(2);
                }
            };
            match run(cmd, path, &flags) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("{}", e);
                    ExitCode::FAILURE
                }
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
    string_strict: bool,
    rooted: bool,
}

fn parse_flags(args: &[String]) -> Result<Flags, String> {
    let mut flags = Flags {
        search_path: Vec::new(),
        no_resolve: false,
        show_deps: false,
        migrate: false,
        state_check: false,
        nominal: false,
        restricted: false,
        strict: false,
        string_strict: false,
        rooted: false,
    };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--search-path" => {
                let paths = args
                    .get(i + 1)
                    .ok_or_else(|| "--search-path requires a value".to_string())?;
                i += 1;
                for path in paths.split(':') {
                    flags.search_path.push(PathBuf::from(path));
                }
            }
            "--no-resolve" => flags.no_resolve = true,
            "--show-deps" => flags.show_deps = true,
            "--state" => flags.state_check = true,
            "--nominal" => flags.nominal = true,
            "--restricted" => {
                flags.restricted = true;
                flags.nominal = true;
            }
            "--migrate" => flags.migrate = true,
            "--strict" => flags.strict = true,
            "--string-strict" => flags.string_strict = true,
            "--rooted" => flags.rooted = true,
            unknown => return Err(format!("unknown flag `{unknown}`")),
        }
        i += 1;
    }
    Ok(flags)
}

fn usage() {
    eprintln!("apslc — APSL compiler\n");
    eprintln!("usage:");
    eprintln!("  apslc parse <file>   print canonical AST to stdout");
    eprintln!("  apslc canon <file>   same — canonical form IS the serialization");
    eprintln!("  apslc hash  <file>   print sha256 hex of canonical form");
    eprintln!("  apslc check <file>   parse + link + type-check, exit 0 if clean");
    eprintln!("  apslc compile <file> emit the checked canonical graph/type artifact");
    eprintln!();
    eprintln!("flags:");
    eprintln!("  --search-path <dirs>  colon-separated directories to search for symbols");
    eprintln!("  --no-resolve          disable linker (error on unresolved symbols)");
    eprintln!("  --show-deps           print resolved dependencies");
    eprintln!("  --state               enforce state clause validation");
    eprintln!("  --nominal             enforce nominal type equality (no structural aliasing)");
    eprintln!("  --restricted          enforce capability narrowing (implies --nominal)");
    eprintln!("  --strict              reject coarse types: every type alias must resolve to a unique structure");
    eprintln!("  --string-strict       require semantic abstract or fixed state types instead of raw String");
    eprintln!("  --rooted              reject bare World (must use World<S>) and enforce single-root connectedness");
    eprintln!("  --migrate             strip unknown syntax for backward-compatible validation");
}

fn run(cmd: &str, path: &str, flags: &Flags) -> Result<(), String> {
    let raw_src =
        std::fs::read_to_string(path).map_err(|e| format!("apslc: cannot read {}: {}", path, e))?;
    let src = if flags.migrate {
        migrate_source(&raw_src)
    } else {
        raw_src
    };
    let prog = parse_str(&src).map_err(|e| render_parse_error(&src, &e))?;

    let linked_prog = if flags.no_resolve {
        prog
    } else {
        let source_path = std::path::Path::new(path);
        match apsl_link::link(&prog, source_path, &flags.search_path) {
            Ok(result) => {
                if flags.show_deps && !result.resolved.is_empty() {
                    eprintln!("resolved {} external symbol(s):", result.resolved.len());
                    for dep in &result.resolved {
                        eprintln!(
                            "  {:<24} <- {}:{}",
                            dep.symbol,
                            dep.file.display(),
                            dep.line
                        );
                    }
                    eprintln!();
                }
                result.program
            }
            Err(e) => return Err(format!("{}", e)),
        }
    };

    if cmd == "check" || cmd == "compile" {
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

        if flags.string_strict {
            let errors = apsl_types::check_string_strict(&linked_prog);
            if !errors.is_empty() {
                return Err(errors.join("\n"));
            }
        }

        if flags.state_check {
            let state_result = check_state(&linked_prog);
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
                    structure_to_aliases
                        .entry(structure)
                        .or_default()
                        .push(name);
                }
            }

            let mut collisions = Vec::new();
            for (structure, aliases) in &structure_to_aliases {
                if aliases.len() > 1 {
                    let non_trivial: Vec<&String> = aliases
                        .iter()
                        .filter(|a| !base_types.contains(&a.as_str()))
                        .collect();
                    if non_trivial.len() > 1 {
                        collisions.push((
                            structure.clone(),
                            non_trivial
                                .iter()
                                .map(|s| s.to_string())
                                .collect::<Vec<_>>(),
                        ));
                    }
                }
            }

            if !collisions.is_empty() {
                let mut msg = format!("apslc --strict: {} type collision(s) — different names resolve to same structure:\n", collisions.len());
                for (structure, names) in &collisions {
                    msg.push_str(&format!(
                        "  {} all resolve to {}\n",
                        names.join(", "),
                        structure
                    ));
                }
                msg.push_str("\nhint: types are too coarse. decompose further until each proposition has a unique structural type.\n");
                return Err(msg);
            }

            let total_aliases = alias_to_structure.len();
            if cmd == "check" {
                println!(
                    "ok (strict: {}/{} type aliases structurally unique)",
                    total_aliases, total_aliases
                );
                return Ok(());
            }
        }

        if cmd == "compile" {
            let mut selected = Vec::new();
            if flags.state_check {
                selected.push(apsl_artifact::Check::State);
            }
            if flags.string_strict {
                selected.push(apsl_artifact::Check::StringStrict);
            }
            let checked = apsl_artifact::check(&linked_prog, &selected).map_err(|errors| {
                errors
                    .into_iter()
                    .map(|error| error.to_string())
                    .collect::<Vec<_>>()
                    .join("\n")
            })?;
            let artifact = apsl_artifact::compile(&checked).map_err(|error| error.to_string())?;
            std::io::stdout()
                .write_all(artifact.canonical_utf8().as_bytes())
                .map_err(|error| format!("apslc compile: cannot write artifact: {error}"))?;
            return Ok(());
        }

        println!("ok");
        return Ok(());
    }

    let canon = linked_prog.canon();
    match cmd {
        "parse" | "canon" => {
            println!("{}", canon);
        }
        "hash" => {
            println!("{}", sha256_hex(canon.as_bytes()));
        }
        _ => unreachable!(),
    }
    Ok(())
}

fn render_parse_error(src: &str, e: &apsl_parse::ParseError) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "apslc: parse error at line {} col {}\n  {}\n",
        e.span.line, e.span.col, e.msg
    ));
    if let Some(line) = src.lines().nth(e.span.line.saturating_sub(1) as usize) {
        s.push_str(&format!("  | {}\n", line));
        let pad = " ".repeat(e.span.col.saturating_sub(1) as usize);
        s.push_str(&format!("  | {}^\n", pad));
    }
    s
}

fn render_type_error(src: &str, e: &apsl_types::TypeError) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "apslc: type error at line {} col {}\n  {}\n",
        e.span.line, e.span.col, e.msg
    ));
    if let Some(line) = src.lines().nth(e.span.line.saturating_sub(1) as usize) {
        s.push_str(&format!("  | {}\n", line));
        let pad = " ".repeat(e.span.col.saturating_sub(1) as usize);
        s.push_str(&format!("  | {}^\n", pad));
    }
    s
}

use apsl_core::ast::{Decl, Graph, Node};
use std::collections::{HashMap, HashSet};

struct StateCheckResult {
    errors: Vec<String>,
    warnings: Vec<String>,
}

fn check_state(prog: &apsl_core::Program) -> StateCheckResult {
    let mut errors = apsl_types::check_state_defaults(prog);
    let mut warnings = Vec::new();
    let mut node_map: HashMap<String, &Node> = HashMap::new();
    let mut graphs: Vec<&Graph> = Vec::new();

    for declaration in &prog.decls {
        match declaration {
            Decl::Node(node) => {
                let mut keys = HashSet::new();
                for state in &node.state {
                    if !keys.insert(state.key.as_str()) {
                        errors.push(format!(
                            "state: node `{}`: duplicate key `{}` would produce the same canonical state path",
                            node.name.as_str(),
                            state.key.as_str()
                        ));
                    }
                }
                node_map.insert(node.name.as_str().to_string(), node);
            }
            Decl::Graph(graph) => {
                let mut keys = HashSet::new();
                for state in &graph.state {
                    if !keys.insert(state.key.as_str()) {
                        errors.push(format!(
                            "state: graph `{}`: duplicate root key `{}` would produce the same canonical state path",
                            graph.name.as_str(),
                            state.key.as_str()
                        ));
                    }
                }
                graphs.push(graph);
            }
            Decl::Type(_) => {}
        }
    }

    for graph in graphs {
        let mut seen = HashSet::new();
        for chain in &graph.flow {
            for step in chain {
                for identifier in &step.nodes {
                    let name = identifier.as_str();
                    if name == "in" || name == "out" || !seen.insert(name) {
                        continue;
                    }
                    if let Some(node) = node_map.get(name) {
                        let external = node
                            .via
                            .as_ref()
                            .is_some_and(|via| via.tag.as_str().contains("external"));
                        if external && node.state.is_empty() {
                            warnings.push(format!(
                                "state: graph `{}`: node `{}` has `via @external` but no state declarations",
                                graph.name.as_str(),
                                name
                            ));
                        }
                    }
                }
            }
        }
    }

    StateCheckResult { errors, warnings }
}

fn check_rooted(prog: &apsl_core::Program) -> Vec<String> {
    let mut errors = Vec::new();
    use apsl_core::ast::Type;

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
                        if a_name == "in" || a_name == "out" {
                            continue;
                        }
                        for b in &w[1].nodes {
                            let b_name = b.as_str();
                            if b_name == "in" || b_name == "out" {
                                continue;
                            }
                            if !all_nodes.contains(a_name) {
                                all_nodes.insert(a_name.to_string());
                            }
                            if !all_nodes.contains(b_name) {
                                all_nodes.insert(b_name.to_string());
                            }
                            edges
                                .entry(a_name.to_string())
                                .or_default()
                                .insert(b_name.to_string());
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
        undirected
            .entry(a.clone())
            .or_default()
            .extend(bs.iter().cloned());
        for b in bs {
            undirected.entry(b.clone()).or_default().insert(a.clone());
        }
    }
    for node in &all_nodes {
        undirected.entry(node.clone()).or_default();
    }
    for node in &all_nodes {
        if visited.contains(node) {
            continue;
        }
        let mut comp: Vec<String> = Vec::new();
        let mut stack = vec![node.clone()];
        while let Some(n) = stack.pop() {
            if !visited.insert(n.clone()) {
                continue;
            }
            comp.push(n.clone());
            if let Some(neighbors) = undirected.get(&n) {
                for nb in neighbors {
                    if !visited.contains(nb) {
                        stack.push(nb.clone());
                    }
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
        msg.push_str(
            "\nHint: shared types such as `World<S>` do not create graph edges; only explicit `flow` between named nodes does.\n\
             Disconnected components often mean the spec names opaque endpoint or handler nodes instead of the real shared authority and dataflow spine.\n\
             Before changing rootedness, expand opaque endpoint or handler nodes into shared operations such as actor verification, delegation derivation, state access, audit, and commit.\n\
             For agency systems, ask what user, or delegated agent of a user, enters every flow, then model verification and derivation of that authority as shared graph structure.\n\
             Do not add synthetic edges merely to make `--rooted` pass; every edge must describe real composition.\n",
        );
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
        if child == parent {
            return true;
        }
        let mut visited = HashSet::new();
        let mut queue = vec![child.to_string()];
        while let Some(current) = queue.pop() {
            if current == parent {
                return true;
            }
            if visited.contains(&current) {
                continue;
            }
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
            let input_names: Vec<String> = n
                .sig
                .params
                .iter()
                .map(|p| type_to_nominal_name(&p.ty))
                .collect();
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
                        if from_name == "in" || from_name == "out" {
                            continue;
                        }

                        for to_node_id in &to_step.nodes {
                            let to_name = to_node_id.as_str();
                            if to_name == "in" || to_name == "out" {
                                continue;
                            }

                            let from_output = node_sigs.get(from_name).map(|(_, o)| o.clone());
                            let to_input = node_sigs
                                .get(to_name)
                                .map(|(inputs, _)| inputs.first().cloned().unwrap_or_default());

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
            let parts: Vec<String> = args.iter().map(type_to_nominal_name).collect();
            format!("{}<{}>", name.as_str(), parts.join(", "))
        }
        apsl_core::ast::Type::List(inner) => format!("{}[]", type_to_nominal_name(inner)),
        apsl_core::ast::Type::Tuple(ts) => {
            let parts: Vec<String> = ts.iter().map(type_to_nominal_name).collect();
            format!("({})", parts.join(", "))
        }
        apsl_core::ast::Type::Var(v) => format!("?{}", v),
        apsl_core::ast::Type::Result(inner) => format!("Result<{}>", type_to_nominal_name(inner)),
        apsl_core::ast::Type::Record(fields) => {
            let parts: Vec<String> = fields
                .iter()
                .map(|(name, ty)| format!("{}: {}", name.as_str(), type_to_nominal_name(ty)))
                .collect();
            format!("{{ {} }}", parts.join(", "))
        }
    }
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
                let rhs = line[eq_pos + 1..].trim();
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
        AuditReq, AuthLevel, CxExpr, CxSpec, FlowStep, Ident, Param, RuntimeClass, ScopeConstraint,
        Span, StateDecl, Type, TypeSig,
    };
    use apsl_core::Canon;

    fn world_a_sig() -> TypeSig {
        TypeSig {
            params: vec![
                Param {
                    name: Ident::new("w"),
                    ty: Type::Base(Ident::new("World")),
                },
                Param {
                    name: Ident::new("x"),
                    ty: Type::Base(Ident::new("A")),
                },
            ],
            ret: Type::Tuple(vec![
                Type::Base(Ident::new("World")),
                Type::Base(Ident::new("A")),
            ]),
        }
    }

    fn node(name: &str, state: Vec<StateDecl>) -> Box<Node> {
        Box::new(Node {
            name: Ident::new(name),
            sig: world_a_sig(),
            pre: vec![],
            post: vec![],
            cx: CxSpec {
                bigo: CxExpr::Const,
                class: RuntimeClass::Idem,
            },
            sla: None,
            via: None,
            auth: AuthLevel::None,
            scope_constraint: ScopeConstraint::Any,
            audit_req: AuditReq::None,
            state,
            span: Span::NONE,
        })
    }

    fn budget() -> StateDecl {
        StateDecl {
            key: Ident::new("budget"),
            ty: Type::Base(Ident::new("Config")),
            default: None,
            span: Span::NONE,
        }
    }

    fn step(name: &str) -> FlowStep {
        FlowStep::single(Ident::new(name), Span::NONE)
    }

    #[test]
    fn rooted_disconnect_diagnostic_points_to_the_common_authority_spine() {
        let mut prog = apsl_core::Program::new();
        for name in ["login_handler", "dashboard_handler"] {
            let mut isolated = node(name, vec![]);
            let root = Type::Parameterized(
                Ident::new("World"),
                vec![Type::Base(Ident::new("ApplicationState"))],
            );
            isolated.sig.params[0].ty = root.clone();
            if let Type::Tuple(parts) = &mut isolated.sig.ret {
                parts[0] = root;
            }
            prog.decls.push(Decl::Node(isolated));
        }

        let message = check_rooted(&prog).join("\n");

        assert!(message.contains("shared types such as `World<S>` do not create graph edges"));
        assert!(message.contains("expand opaque endpoint or handler nodes"));
        assert!(message.contains("what user, or delegated agent of a user, enters every flow"));
        assert!(message.contains("Do not add synthetic edges"));
    }

    #[test]
    fn preserves_sibling_state_at_each_node_position() {
        let mut prog = apsl_core::Program::new();
        prog.decls.push(Decl::Node(node("parent", vec![])));
        prog.decls
            .push(Decl::Node(node("child_left", vec![budget()])));
        prog.decls
            .push(Decl::Node(node("child_right", vec![budget()])));
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

        let res = check_state(&prog);

        assert!(res.errors.is_empty(), "unexpected errors: {:?}", res.errors);

        for d in &prog.decls {
            if let Decl::Node(n) = d {
                let has = n.state.iter().any(|s| s.key.as_str() == "budget");
                match n.name.as_str() {
                    "parent" => assert!(!has, "parent must not acquire child state"),
                    "child_left" | "child_right" => {
                        assert!(has, "{} must retain its positional state", n.name)
                    }
                    _ => {}
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
        let res = check_state(&prog);
        let after = prog.canon();

        assert!(res.errors.is_empty(), "no error expected");
        assert_eq!(before, after, "canonical form (and hash) must be unchanged");
    }

    #[test]
    fn same_key_with_different_types_is_valid_at_distinct_positions() {
        let mut prog = apsl_core::Program::new();
        let cfg = StateDecl {
            key: Ident::new("budget"),
            ty: Type::Base(Ident::new("Config")),
            default: None,
            span: Span::NONE,
        };
        let money = StateDecl {
            key: Ident::new("budget"),
            ty: Type::Base(Ident::new("Money")),
            default: None,
            span: Span::NONE,
        };
        prog.decls.push(Decl::Node(node("parent", vec![])));
        prog.decls.push(Decl::Node(node("child_left", vec![cfg])));
        prog.decls
            .push(Decl::Node(node("child_right", vec![money])));
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

        let res = check_state(&prog);
        assert!(res.errors.is_empty(), "distinct paths do not conflict");
    }
}
