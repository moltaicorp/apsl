#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;

use apsl_core::ast::{
    AuditReq, AuthLevel, Decl, Graph, Node, Program, ScopeConstraint, StateDecl, Type,
};
use apsl_core::canon::{write_int, write_null, write_str, ArrayWriter, ObjectWriter};
use apsl_core::hash::sha256_hex;
use apsl_core::Canon;
use apsl_types::{node_placement, state_kind, NodePlacement, StateKind};

const SCHEMA: &str = "apsl.compiled-graph-types.v1";
const CANON_VERSION: &str = "apsl-canon-utf8.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Check {
    State,
    StringStrict,
}

impl Check {
    fn name(self) -> &'static str {
        match self {
            Self::State => "state",
            Self::StringStrict => "string-strict",
        }
    }
}

#[derive(Debug, Clone)]
pub struct CheckedProgram {
    program: Program,
    checks: BTreeSet<Check>,
}

#[derive(Debug, Clone)]
pub struct CanonicalArtifact {
    canonical_utf8: String,
    sha256_hex: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckError {
    message: String,
}

impl CheckError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for CheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CheckError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactError {
    message: String,
}

impl ArtifactError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for ArtifactError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ArtifactError {}

pub fn check(program: &Program, checks: &[Check]) -> Result<CheckedProgram, Vec<CheckError>> {
    let selected: BTreeSet<Check> = checks.iter().copied().collect();
    let mut errors = check_declaration_uniqueness(program);

    if let Err(type_errors) = apsl_types::type_check(program) {
        errors.extend(type_errors.into_iter().map(|error| {
            CheckError::new(format!(
                "type at {}:{}: {}",
                error.span.line, error.span.col, error.msg
            ))
        }));
    }
    if selected.contains(&Check::StringStrict) {
        errors.extend(
            apsl_types::check_string_strict(program)
                .into_iter()
                .map(CheckError::new),
        );
    }
    if selected.contains(&Check::State) {
        errors.extend(
            apsl_types::check_state_defaults(program)
                .into_iter()
                .map(CheckError::new),
        );
        errors.extend(check_state_ownership(program));
    }

    if errors.is_empty() {
        Ok(CheckedProgram {
            program: program.clone(),
            checks: selected,
        })
    } else {
        Err(errors)
    }
}

fn check_declaration_uniqueness(program: &Program) -> Vec<CheckError> {
    let mut errors = Vec::new();
    let mut names = HashSet::new();
    for declaration in &program.decls {
        let name = match declaration {
            Decl::Type(alias) => alias.name.as_str(),
            Decl::Node(node) => node.name.as_str(),
            Decl::Graph(graph) => graph.name.as_str(),
        };
        if !names.insert(name) {
            errors.push(CheckError::new(format!(
                "artifact: duplicate declaration `{name}` has ambiguous canonical identity"
            )));
        }
    }
    errors
}

fn check_state_ownership(program: &Program) -> Vec<CheckError> {
    let mut errors = Vec::new();
    for declaration in &program.decls {
        let (owner_kind, owner_name, states) = match declaration {
            Decl::Node(node) => ("node", node.name.as_str(), node.state.as_slice()),
            Decl::Graph(graph) => ("graph", graph.name.as_str(), graph.state.as_slice()),
            Decl::Type(_) => continue,
        };
        let mut keys = HashSet::new();
        for state in states {
            if !keys.insert(state.key.as_str()) {
                errors.push(CheckError::new(format!(
                    "state: {owner_kind} `{owner_name}`: duplicate key `{}` would produce the same canonical state path",
                    state.key.as_str()
                )));
            }
        }
    }
    errors
}

pub fn compile(checked: &CheckedProgram) -> Result<CanonicalArtifact, ArtifactError> {
    let document = ArtifactDocument::build(checked)?;
    let canonical_utf8 = document.canon();
    let sha256_hex = sha256_hex(canonical_utf8.as_bytes());
    Ok(CanonicalArtifact {
        canonical_utf8,
        sha256_hex,
    })
}

impl CanonicalArtifact {
    pub fn canonical_utf8(&self) -> &str {
        &self.canonical_utf8
    }

    pub fn sha256_hex(&self) -> &str {
        &self.sha256_hex
    }
}

#[derive(Debug, Clone)]
struct TypeEntry {
    canonical: String,
    hash: String,
}

#[derive(Debug, Clone)]
struct ContractEntry {
    node: Node,
    signature_hash: String,
    contract_hash: String,
}

#[derive(Debug, Clone)]
struct Occurrence {
    contract: usize,
    first_position: [usize; 3],
}

#[derive(Debug, Clone, Copy)]
enum FlowReference {
    In,
    Out,
    Occurrence(usize),
}

#[derive(Debug, Clone)]
struct GraphEntry {
    graph: Graph,
    hash: String,
    occurrences: Vec<Occurrence>,
    flow_occurrences: Vec<Vec<Vec<FlowReference>>>,
}

#[derive(Debug, Clone)]
struct ArtifactDocument {
    source_hash: String,
    checks: Vec<String>,
    types: Vec<TypeEntry>,
    type_ordinals: BTreeMap<String, usize>,
    aliases: Vec<(String, usize)>,
    contracts: Vec<ContractEntry>,
    graphs: Vec<GraphEntry>,
}

impl ArtifactDocument {
    fn build(checked: &CheckedProgram) -> Result<Self, ArtifactError> {
        let program = &checked.program;
        let mut canonical_types = BTreeSet::new();
        collect_program_types(program, &mut canonical_types);
        let types: Vec<TypeEntry> = canonical_types
            .into_iter()
            .map(|canonical| TypeEntry {
                hash: domain_hash("apsl.type.v1", &canonical),
                canonical,
            })
            .collect();
        let type_ordinals: BTreeMap<String, usize> = types
            .iter()
            .enumerate()
            .map(|(ordinal, entry)| (entry.canonical.clone(), ordinal))
            .collect();

        let aliases = program
            .decls
            .iter()
            .filter_map(|declaration| match declaration {
                Decl::Type(alias) => Some((
                    alias.name.as_str().to_string(),
                    *type_ordinals
                        .get(&alias.rhs.canon())
                        .expect("collected alias type"),
                )),
                _ => None,
            })
            .collect();

        let contracts: Vec<ContractEntry> = program
            .decls
            .iter()
            .filter_map(|declaration| match declaration {
                Decl::Node(node) => {
                    let signature = signature_canon(&node.sig);
                    let contract = contract_canon(node);
                    Some(ContractEntry {
                        node: (**node).clone(),
                        signature_hash: domain_hash("apsl.signature.v1", &signature),
                        contract_hash: domain_hash("apsl.contract.v1", &contract),
                    })
                }
                _ => None,
            })
            .collect();
        let contract_by_name: HashMap<&str, usize> = contracts
            .iter()
            .enumerate()
            .map(|(ordinal, contract)| (contract.node.name.as_str(), ordinal))
            .collect();

        let mut graphs = Vec::new();
        for declaration in &program.decls {
            let Decl::Graph(graph) = declaration else {
                continue;
            };
            graphs.push(build_graph(graph, &contract_by_name, &contracts)?);
        }

        let mut checks: Vec<String> = checked
            .checks
            .iter()
            .map(|check| check.name().to_string())
            .chain(std::iter::once("types".to_string()))
            .collect();
        checks.sort();
        checks.dedup();

        Ok(Self {
            source_hash: sha256_hex(program.canon().as_bytes()),
            checks,
            types,
            type_ordinals,
            aliases,
            contracts,
            graphs,
        })
    }
}

impl Canon for ArtifactDocument {
    fn write_canon(&self, out: &mut String) {
        let mut object = ObjectWriter::new(out);
        object.field("aliases", |out| self.write_aliases(out));
        object.field("canon", |out| write_str(out, CANON_VERSION));
        object.field("checks", |out| write_string_array(out, &self.checks));
        object.field("contracts", |out| self.write_contracts(out));
        object.field("graphs", |out| self.write_graphs(out));
        object.field("schema", |out| write_str(out, SCHEMA));
        object.field("source_hash", |out| write_str(out, &self.source_hash));
        object.field("types", |out| self.write_types(out));
        object.finish();
    }
}

impl ArtifactDocument {
    fn write_types(&self, out: &mut String) {
        let mut array = ArrayWriter::new(out);
        for (ordinal, entry) in self.types.iter().enumerate() {
            array.item(|out| {
                let mut object = ObjectWriter::new(out);
                object.field("hash", |out| write_str(out, &entry.hash));
                object.field("ordinal", |out| write_usize(out, ordinal));
                object.field("structure", |out| out.push_str(&entry.canonical));
                object.finish();
            });
        }
        array.finish();
    }

    fn write_aliases(&self, out: &mut String) {
        let mut array = ArrayWriter::new(out);
        for (name, representation) in &self.aliases {
            array.item(|out| {
                let mut object = ObjectWriter::new(out);
                object.field("name", |out| write_str(out, name));
                object.field("representation", |out| write_usize(out, *representation));
                object.finish();
            });
        }
        array.finish();
    }

    fn write_contracts(&self, out: &mut String) {
        let mut array = ArrayWriter::new(out);
        for (ordinal, contract) in self.contracts.iter().enumerate() {
            array.item(|out| {
                let node = &contract.node;
                let mut object = ObjectWriter::new(out);
                object.field("contract_hash", |out| {
                    write_str(out, &contract.contract_hash)
                });
                object.field("inputs", |out| {
                    let mut inputs = ArrayWriter::new(out);
                    for parameter in &node.sig.params {
                        inputs.item(|out| self.write_type_reference(out, &parameter.ty));
                    }
                    inputs.finish();
                });
                object.field("name", |out| write_str(out, node.name.as_str()));
                object.field("ordinal", |out| write_usize(out, ordinal));
                object.field("output", |out| {
                    self.write_type_reference(out, &node.sig.ret)
                });
                object.field("placement", |out| {
                    write_placement(out, node_placement(node))
                });
                object.field("signature_hash", |out| {
                    write_str(out, &contract.signature_hash)
                });
                object.field("states", |out| self.write_states(out, &node.state));
                object.finish();
            });
        }
        array.finish();
    }

    fn write_graphs(&self, out: &mut String) {
        let mut array = ArrayWriter::new(out);
        for (graph_ordinal, entry) in self.graphs.iter().enumerate() {
            array.item(|out| self.write_graph(out, graph_ordinal, entry));
        }
        array.finish();
    }

    fn write_graph(&self, out: &mut String, graph_ordinal: usize, entry: &GraphEntry) {
        let mut object = ObjectWriter::new(out);
        object.field("flow", |out| write_flow(out, &entry.flow_occurrences));
        object.field("graph_hash", |out| write_str(out, &entry.hash));
        object.field("name", |out| write_str(out, entry.graph.name.as_str()));
        object.field("occurrences", |out| {
            let mut occurrences = ArrayWriter::new(out);
            for (ordinal, occurrence) in entry.occurrences.iter().enumerate() {
                occurrences.item(|out| {
                    let mut occurrence_object = ObjectWriter::new(out);
                    occurrence_object
                        .field("contract", |out| write_usize(out, occurrence.contract));
                    occurrence_object.field("first_position", |out| {
                        write_usize_array(out, &occurrence.first_position)
                    });
                    occurrence_object.field("ordinal", |out| write_usize(out, ordinal));
                    occurrence_object.field("owner", |out| {
                        write_usize_array(out, &[graph_ordinal, ordinal])
                    });
                    occurrence_object.finish();
                });
            }
            occurrences.finish();
        });
        object.field("ordinal", |out| write_usize(out, graph_ordinal));
        object.field("root_states", |out| {
            self.write_states(out, &entry.graph.state)
        });
        object.field("state_addresses", |out| {
            self.write_state_addresses(out, graph_ordinal, entry)
        });
        object.finish();
    }

    fn write_state_addresses(&self, out: &mut String, graph_ordinal: usize, entry: &GraphEntry) {
        let mut addresses = ArrayWriter::new(out);
        for (state_ordinal, state) in entry.graph.state.iter().enumerate() {
            addresses
                .item(|out| write_state_address(out, None, &[graph_ordinal], state_ordinal, state));
        }
        for (occurrence_ordinal, occurrence) in entry.occurrences.iter().enumerate() {
            for (state_ordinal, state) in self.contracts[occurrence.contract]
                .node
                .state
                .iter()
                .enumerate()
            {
                addresses.item(|out| {
                    write_state_address(
                        out,
                        Some(occurrence.contract),
                        &[graph_ordinal, occurrence_ordinal],
                        state_ordinal,
                        state,
                    )
                });
            }
        }
        addresses.finish();
    }

    fn write_states(&self, out: &mut String, states: &[StateDecl]) {
        let mut array = ArrayWriter::new(out);
        for (ordinal, state) in states.iter().enumerate() {
            array.item(|out| {
                let mut object = ObjectWriter::new(out);
                object.field("default", |out| match &state.default {
                    Some(default) => default.write_canon(out),
                    None => write_null(out),
                });
                object.field("key", |out| write_str(out, state.key.as_str()));
                object.field("kind", |out| write_state_kind(out, state_kind(state)));
                object.field("ordinal", |out| write_usize(out, ordinal));
                object.field("type", |out| self.write_type_reference(out, &state.ty));
                object.finish();
            });
        }
        array.finish();
    }

    fn write_type_reference(&self, out: &mut String, ty: &Type) {
        let canonical = ty.canon();
        let ordinal = self.type_ordinals[&canonical];
        let mut object = ObjectWriter::new(out);
        object.field("hash", |out| write_str(out, &self.types[ordinal].hash));
        object.field("ordinal", |out| write_usize(out, ordinal));
        object.finish();
    }
}

fn collect_program_types(program: &Program, types: &mut BTreeSet<String>) {
    for declaration in &program.decls {
        match declaration {
            Decl::Type(alias) => {
                collect_type(&Type::Base(alias.name.clone()), types);
                collect_type(&alias.rhs, types);
            }
            Decl::Node(node) => {
                collect_signature_types(&node.sig, types);
                for state in &node.state {
                    collect_type(&state.ty, types);
                }
            }
            Decl::Graph(graph) => {
                collect_signature_types(&graph.sig, types);
                for state in &graph.state {
                    collect_type(&state.ty, types);
                }
            }
        }
    }
}

fn collect_signature_types(signature: &apsl_core::ast::TypeSig, types: &mut BTreeSet<String>) {
    for parameter in &signature.params {
        collect_type(&parameter.ty, types);
    }
    collect_type(&signature.ret, types);
}

fn collect_type(ty: &Type, types: &mut BTreeSet<String>) {
    types.insert(ty.canon());
    match ty {
        Type::Parameterized(_, arguments) | Type::Tuple(arguments) => {
            for argument in arguments {
                collect_type(argument, types);
            }
        }
        Type::Record(fields) => {
            for (_, field_type) in fields {
                collect_type(field_type, types);
            }
        }
        Type::List(inner) | Type::Result(inner) => collect_type(inner, types),
        Type::Base(_) | Type::Var(_) => {}
    }
}

fn build_graph(
    graph: &Graph,
    contract_by_name: &HashMap<&str, usize>,
    contracts: &[ContractEntry],
) -> Result<GraphEntry, ArtifactError> {
    let mut occurrence_by_name: HashMap<&str, usize> = HashMap::new();
    let mut occurrences = Vec::new();
    let mut flow_occurrences = Vec::new();

    for (chain_ordinal, chain) in graph.flow.iter().enumerate() {
        let mut compiled_chain = Vec::new();
        for (step_ordinal, step) in chain.iter().enumerate() {
            let mut compiled_step = Vec::new();
            for (member_ordinal, name) in step.nodes.iter().enumerate() {
                let name = name.as_str();
                if name == "in" {
                    compiled_step.push(FlowReference::In);
                    continue;
                }
                if name == "out" {
                    compiled_step.push(FlowReference::Out);
                    continue;
                }
                let contract = *contract_by_name.get(name).ok_or_else(|| {
                    ArtifactError::new(format!(
                        "graph `{}` references unknown node `{name}`",
                        graph.name.as_str()
                    ))
                })?;
                let occurrence = if let Some(existing) = occurrence_by_name.get(name) {
                    *existing
                } else {
                    let ordinal = occurrences.len();
                    occurrences.push(Occurrence {
                        contract,
                        first_position: [chain_ordinal, step_ordinal, member_ordinal],
                    });
                    occurrence_by_name.insert(name, ordinal);
                    ordinal
                };
                compiled_step.push(FlowReference::Occurrence(occurrence));
            }
            compiled_chain.push(compiled_step);
        }
        flow_occurrences.push(compiled_chain);
    }

    let graph_semantics = graph_semantic_canon(graph, &flow_occurrences, &occurrences, contracts);
    Ok(GraphEntry {
        graph: graph.clone(),
        hash: domain_hash("apsl.graph.v1", &graph_semantics),
        occurrences,
        flow_occurrences,
    })
}

fn signature_canon(signature: &apsl_core::ast::TypeSig) -> String {
    let mut out = String::new();
    let mut object = ObjectWriter::new(&mut out);
    object.field("inputs", |out| {
        let mut array = ArrayWriter::new(out);
        for parameter in &signature.params {
            array.item(|out| parameter.ty.write_canon(out));
        }
        array.finish();
    });
    object.field("output", |out| signature.ret.write_canon(out));
    object.finish();
    out
}

fn contract_canon(node: &Node) -> String {
    let mut out = String::new();
    let mut object = ObjectWriter::new(&mut out);
    object.field("audit", |out| write_str(out, audit_name(node.audit_req)));
    object.field("auth", |out| write_str(out, auth_name(node.auth)));
    object.field("cx", |out| node.cx.write_canon(out));
    object.field("deploy", |out| match &node.deploy {
        Some(deploy) if !deploy.is_empty() => deploy.write_canon(out),
        _ => write_null(out),
    });
    object.field("post", |out| write_canon_array(out, &node.post));
    object.field("pre", |out| write_canon_array(out, &node.pre));
    object.field("scope", |out| {
        write_str(out, scope_name(&node.scope_constraint))
    });
    object.field("signature", |out| out.push_str(&signature_canon(&node.sig)));
    object.field("sla", |out| match &node.sla {
        Some(sla) => sla.write_canon(out),
        None => write_null(out),
    });
    object.field("state", |out| {
        let mut states = ArrayWriter::new(out);
        for state in &node.state {
            states.item(|out| {
                let mut state_object = ObjectWriter::new(out);
                state_object.field("default", |out| match &state.default {
                    Some(default) => default.write_canon(out),
                    None => write_null(out),
                });
                state_object.field("type", |out| state.ty.write_canon(out));
                state_object.finish();
            });
        }
        states.finish();
    });
    object.field("via", |out| match &node.via {
        Some(via) => via.write_canon(out),
        None => write_null(out),
    });
    object.finish();
    out
}

fn graph_semantic_canon(
    graph: &Graph,
    flow: &[Vec<Vec<FlowReference>>],
    occurrences: &[Occurrence],
    contracts: &[ContractEntry],
) -> String {
    let mut out = String::new();
    let mut object = ObjectWriter::new(&mut out);
    object.field("flow", |out| write_flow(out, flow));
    object.field("occurrence_contracts", |out| {
        let mut array = ArrayWriter::new(out);
        for occurrence in occurrences {
            array.item(|out| write_str(out, &contracts[occurrence.contract].contract_hash));
        }
        array.finish();
    });
    object.field("post", |out| write_canon_array(out, &graph.post));
    object.field("signature", |out| {
        out.push_str(&signature_canon(&graph.sig))
    });
    object.field("state", |out| write_canon_array(out, &graph.state));
    object.finish();
    out
}

fn domain_hash(domain: &str, canonical: &str) -> String {
    let mut preimage = String::with_capacity(domain.len() + canonical.len() + 1);
    preimage.push_str(domain);
    preimage.push('\0');
    preimage.push_str(canonical);
    sha256_hex(preimage.as_bytes())
}

fn write_state_address(
    out: &mut String,
    contract: Option<usize>,
    owner: &[usize],
    state_ordinal: usize,
    state: &StateDecl,
) {
    let mut object = ObjectWriter::new(out);
    object.field("contract", |out| match contract {
        Some(contract) => write_usize(out, contract),
        None => write_null(out),
    });
    object.field("kind", |out| write_state_kind(out, state_kind(state)));
    object.field("owner", |out| write_usize_array(out, owner));
    object.field("state", |out| write_usize(out, state_ordinal));
    object.finish();
}

fn write_flow(out: &mut String, flow: &[Vec<Vec<FlowReference>>]) {
    let mut chains = ArrayWriter::new(out);
    for chain in flow {
        chains.item(|out| {
            let mut steps = ArrayWriter::new(out);
            for step in chain {
                steps.item(|out| {
                    let mut members = ArrayWriter::new(out);
                    for member in step {
                        members.item(|out| {
                            let mut object = ObjectWriter::new(out);
                            match member {
                                FlowReference::In => {
                                    object.field("port", |out| write_str(out, "in"));
                                }
                                FlowReference::Out => {
                                    object.field("port", |out| write_str(out, "out"));
                                }
                                FlowReference::Occurrence(ordinal) => {
                                    object.field("occurrence", |out| write_usize(out, *ordinal));
                                }
                            }
                            object.finish();
                        });
                    }
                    members.finish();
                });
            }
            steps.finish();
        });
    }
    chains.finish();
}

fn write_canon_array<T: Canon>(out: &mut String, values: &[T]) {
    let mut array = ArrayWriter::new(out);
    for value in values {
        array.item(|out| value.write_canon(out));
    }
    array.finish();
}

fn write_string_array(out: &mut String, values: &[String]) {
    let mut array = ArrayWriter::new(out);
    for value in values {
        array.item(|out| write_str(out, value));
    }
    array.finish();
}

fn write_usize_array(out: &mut String, values: &[usize]) {
    let mut array = ArrayWriter::new(out);
    for value in values {
        array.item(|out| write_usize(out, *value));
    }
    array.finish();
}

fn write_usize(out: &mut String, value: usize) {
    write_int(out, value as i128);
}

fn write_state_kind(out: &mut String, kind: StateKind) {
    write_str(
        out,
        match kind {
            StateKind::Abstract => "abstract",
            StateKind::Fixed => "fixed",
        },
    );
}

fn write_placement(out: &mut String, placement: NodePlacement) {
    write_str(
        out,
        match placement {
            NodePlacement::Fungible => "fungible",
            NodePlacement::Positional => "positional",
        },
    );
}

fn auth_name(auth: AuthLevel) -> &'static str {
    match auth {
        AuthLevel::None => "none",
        AuthLevel::Bearer => "bearer",
        AuthLevel::Session => "session",
        AuthLevel::Passkey => "passkey",
    }
}

fn scope_name(scope: &ScopeConstraint) -> &'static str {
    match scope {
        ScopeConstraint::Any => "any",
        ScopeConstraint::Narrowing => "narrowing",
        ScopeConstraint::Admitted => "admitted",
    }
}

fn audit_name(audit: AuditReq) -> &'static str {
    match audit {
        AuditReq::None => "none",
        AuditReq::Before => "before",
        AuditReq::After => "after",
        AuditReq::Both => "both",
    }
}
