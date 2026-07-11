# APSL — Abstract Protocol Schema Language

<p align="center">
  <img src="assets/apsl-the-file-is-the-machine.png" alt="APSL — the file is the machine">
</p>

[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![Rust 2021](https://img.shields.io/badge/Rust-2021-orange.svg)](https://www.rust-lang.org/)
[![Crates: 15](https://img.shields.io/badge/crates-15-green.svg)](#workspace)

APSL is a typed specification language for composable protocol contracts. An APSL source file declares types, pure nodes, and graph-shaped flows. The toolchain parses and canonicalizes sources, resolves referenced symbols, checks graph composition, derives predicate complexity, analyzes predicates with an SMT solver, and emits signed certificates for specifications that pass those gates.

APSL verifies specifications. It does not prove that an arbitrary executable implements a specification.

## Guarantees

The tools establish distinct properties:

| Gate | Successful result |
|---|---|
| `apslc parse` | The source is syntactically valid and has a canonical AST. |
| `apslc check` | Referenced symbols resolve and graph edges type-check. |
| `apslc check --string-strict` | Every string-bearing use refers through a named semantic type, and free string literals are rejected in predicates. |
| `apsl-lint complex` | Derived predicate complexity does not exceed O(n log n) per input-size variable. |
| `apsl-lint pred` | Every reported clause is proved by the selected SMT solver. |
| `apsl-cert emit` | Parsing, linking, typing, complexity, and predicate gates pass before any certificate is stored. |
| `apsl-cert verify` | The certificate signature is valid for the supplied public key and its TCB manifest matches the pinned TCB. |

Certificate verification does not compare an executable with the optional implementation hash stored in a certificate. A certificate signature authenticates the certificate contents; it is not an implementation-refinement proof.

The runtime executes graph nodes through adapters. A zero exit status means execution succeeded. It does not prove a postcondition. Execution records remain unverified when any executed node declares a postcondition, and `apsl verify` checks record-hash integrity rather than implementation correctness.

## Installation

Requirements:

- Current stable Rust with Cargo
- Z3 for predicate discharge and certificate emission
- CVC5 as an optional solver fallback

```bash
git clone https://github.com/moltaicorp/apsl.git
cd apsl
cargo build --release
```

The workspace builds five binaries in `target/release`:

| Binary | Purpose |
|---|---|
| `apslc` | Parse, canonicalize, hash, link, type-check, compile artifacts, and deploy. |
| `apsl-lint` | Derive complexity and discharge predicates. |
| `apsl-cert` | Generate keys and emit, inspect, or verify certificates. |
| `apsl` | List and execute graph nodes and inspect execution records. |
| `apsl-workbench` | Serve the HTTP workbench. |

## Language overview

```apsl
type Email = String
type MessageId = String

normalize : String[] -> Email[]
  post  every out valid_email?
  cx    O(n) idem
  sla   e <= 0, d <= 0, T <= 5ms

dedupe : Email[] -> Email[]
  pre   every in valid_email?
  post  unique? out, subseteq? out in
  cx    O(n log n) idem
  sla   e <= 0, d <= 1e-9, T <= 50ms

classify : Email[] -> Email[]
  cx    O(n) idem-complex
  via   @statistical holdout=intents_v3

send : Email[] -> MessageId[]
  cx    O(n) idem-complex

graph email_pipeline : String[] -> MessageId[]
  flow  in -> normalize -> dedupe -> classify -> send -> out
```

Node signatures are pure functions. Graph flow edges compose when the preceding output type unifies with the following input type. Fan-in preserves ordinary tuple outputs as nested tuples. When fan-in combines values shaped as `(World<S>, X)`, it deduplicates the shared `World<S>` and produces `(World<S>, X, ...)`.

APSL is first-order. It has no lambda abstraction or general higher-order functions. A fixed primitive environment supports predicate combinators such as `every`, `some`, `map`, `filter`, `fold`, `unique?`, and `subseteq?`.

## Quick start

Type-check the examples:

```bash
./target/release/apslc check examples/dedupe.apsl
./target/release/apslc check examples/world_threading.apsl
```

Run a complete green lint pass on the world-threading example:

```bash
./target/release/apsl-lint check examples/world_threading.apsl
```

The dedupe example type-checks and passes the complexity gate, but its predicates depend on uninterpreted email and collection semantics. Predicate discharge therefore reports counterexamples and exits nonzero:

```bash
./target/release/apsl-lint complex examples/dedupe.apsl
./target/release/apsl-lint pred examples/dedupe.apsl
```

The first command succeeds. The second is an intentional demonstration of fail-closed predicate analysis.

The quadratic example is an intentional negative fixture:

```bash
./target/release/apslc check examples/bad_n_squared.apsl
./target/release/apsl-lint complex examples/bad_n_squared.apsl
```

It type-checks, then the complexity command rejects its nested quantifiers with a nonzero exit status.

## Compiler

```text
apslc parse <file>
apslc canon <file>
apslc hash <file>
apslc check <file>
apslc compile <file>
apslc deploy <file>
```

`parse` and `canon` print the canonical AST. `hash` prints its SHA-256 digest. `check` links and type-checks. `compile` emits the checked canonical graph/type artifact. `deploy` emits GitLab child-pipeline YAML for a source carrying deployment clauses.

Compiler flags:

| Flag | Behavior |
|---|---|
| `--search-path <dirs>` | Add colon-separated linker search directories. |
| `--no-resolve` | Disable external symbol resolution. |
| `--show-deps` | Print resolved symbols and source locations. |
| `--state` | Validate exact state ownership, fixed defaults, and duplicate keys at one owner. |
| `--nominal` | Require nominal equality between aliased edge types. |
| `--restricted` | Require capability narrowing and enable nominal checking. |
| `--strict` | Reject aliases that resolve to duplicate structural shapes. |
| `--rooted` | Reject bare `World` and require a single weakly connected root. |
| `--migrate` | Strip unsupported clauses during compatibility migration. |
| `--string-strict` | Reject raw string-bearing uses that do not pass through named semantic types. |

There are no import statements. Referencing an unresolved type, node, graph, or predicate invokes the linker. Search paths come from explicit flags, `APSL_PATH`, `.apsl-path`, the source directory, and the workspace root.

The canonical self-spec is `apsl.apsl`. `ouroboros.apsl` is a linker-only graph that resolves its `apsl` node and types from the canonical file.

## Linter

```text
apsl-lint check <file>
apsl-lint complex <file>
apsl-lint pred <file>
apsl-lint explain <file> <node>
```

The linter resolves linked declarations before type-checking. `check` succeeds only when both complexity and predicate gates succeed. `pred` treats counterexamples and encoding errors as failures. Solver `Unknown` results are printed but are not proofs.

The SMT encoder models implementation behavior as an uninterpreted function. A `proved` result means the negated postcondition is unsatisfiable under the encoded assumptions even with that uninterpreted implementation. It does not establish that concrete source code satisfies the node contract.

## Certificates

```text
apsl-cert key new <name>
apsl-cert emit <file> --key <name>
apsl-cert verify <hash> --pub <name>
apsl-cert verify <hash> --key <name>
apsl-cert show <hash>
```

`emit` resolves linked declarations and refuses to write any certificate if the source has a type error, exceeds the complexity gate, or contains a predicate verdict other than `Proved`. Successful certificates are stored under `.apsl-store` by certificate hash.

```bash
./target/release/apsl-cert key new demo
./target/release/apsl-cert emit examples/world_threading.apsl --key demo
```

The emit command prints one hash and node name per certificate. Use any printed hash with `show` and `verify`:

```bash
./target/release/apsl-cert show HASH
./target/release/apsl-cert verify HASH --pub demo
```

Private keys are written with owner-only permissions. Generated keys and `.apsl-store` are ignored by Git.

## Runtime

```text
apsl nodes --graph <graph> <spec>
apsl run --graph <graph> [options] <spec>
apsl verify <record>
```

`nodes` lists graph nodes and their `via` metadata. `run` executes node adapters and emits an execution record. Shell adapters read `APSL_INPUT` and return JSON on standard output. `verify` recomputes each proof hash in a saved record and exits nonzero on a mismatch.

The runtime does not evaluate APSL predicates. A successful process exit and a valid execution-record hash are operational facts, not contract proofs.

The current runtime linearizes named nodes and threads one value through that order. APSL type-checking supports fan-in graphs, but the runtime does not yet materialize branch-specific values or fan-in routing.

## Workbench

```bash
cargo run -p apsl-workbench
```

The workbench listens on `0.0.0.0:8800` by default. Set `WB_ADDR` to override the address. It exposes `/healthz`, `/compile`, `/build`, and `/verify`.

## Strictness

Default checking uses structural alias resolution. Additional flags enforce stronger constraints:

1. `--nominal` distinguishes aliases by name.
2. `--restricted` permits only declared subtype narrowing across graph edges.
3. `--strict` rejects multiple aliases with the same structural representation.
4. `--rooted` requires parameterized worlds and a single rooted graph.
5. `--state` preserves positional ownership, validates fixed defaults, and rejects duplicate keys at one owner.
6. `--string-strict` requires named semantic types for string-bearing values and rejects free predicate strings.

These modes are opt-in and may reject sources that are valid under default structural checking.

### State position and fungibility

A state key is local to its declaring node or graph root. Authority uses the canonical graph hash, the owner's ordered ordinal path, and the state declaration ordinal. The key is a diagnostic label: equal key names at different positions are distinct and are never hoisted or deduplicated.

Under `--string-strict`, `String` is a representation boundary for named semantic types, not a valid type at a node, graph, record field, parameterized argument, or state use site. For example, `type Endpoint = String` followed by `state origin : Endpoint` is valid, while `state origin : String` is not. This names the semantic type without manufacturing a separate type for every state instance.

A state declaration without a default is abstract and makes its node positional. A declaration with a type-correct default is fixed by the canonical contract. Nodes with no abstract state are fungible at the APSL composition boundary: graph edges may substitute contracts with compatible compiled signatures. This is a specification property, not proof that any particular executable implements the contract.

### Compiled graph/type artifact

`apslc compile FILE` emits `apsl.compiled-graph-types.v1` as exact canonical UTF-8. The command accepts the same composable checking flags as `apslc check`; for example:

```bash
./target/release/apslc compile protocol.apsl --state --string-strict > protocol.apsl.canon
sha256sum protocol.apsl.canon
```

The artifact records mandatory typing and the selected artifact-native `state` and `string-strict` checks. Other compiler policy flags may reject emission but are not serialized as artifact check identifiers. The artifact also contains a canonical semantic type table, separate signature and full-contract hashes, ordered state declarations, graph occurrences and typed flow references, derived placement, and state addresses. Its whole-artifact identity is SHA-256 of the emitted bytes. It contains no implementation paths, language bindings, source scan results, or executable-conformance claims.

## Workspace

| Crate | Role |
|---|---|
| `apsl-core` | AST, canonical serialization, and hashing. |
| `apsl-parse` | Lexer and parser. |
| `apsl-types` | Type inference and graph composition. |
| `apsl-complex` | Predicate-complexity derivation. |
| `apsl-smt` | SMT encoding, solver processes, and verdicts. |
| `apsl-cert` | Certificates, keys, TCB manifests, and storage. |
| `apsl-link` | Cross-file symbol discovery and resolution. |
| `apsl-artifact` | Checked canonical graph/type artifact production. |
| `apsl-runtime` | Graph execution and execution records. |
| `apsl-verify` | Numeric sampling verifier library. |
| `apsl-workbench` | HTTP workbench and candidate-resolution experiments. |
| `apslc` | Compiler binary. |
| `apsl-lint` | Linter binary. |
| `apsl-cert-cli` | Certificate binary. |
| `apsl` | Runtime binary. |

## Repository specifications

`apsl.apsl` specifies the public APSL toolchain surface. The `examples` directory contains two positive composition examples and one intentional complexity rejection. The `proofs` directory contains type-checked proof DAGs and supporting prose. The `tests` directory contains nominal and restricted-mode positive and negative fixtures.

The proof DAGs establish that their declared dependency edges compose as typed APSL graphs. They do not establish the mathematical truth of undeclared axioms or prove concrete implementations. `soundness-phase-bound.apsl` is type-valid but intentionally not certifiable: its positive-output postconditions remain implementation obligations, and predicate discharge rejects them.

## Development

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
./scripts/verify-apsl.sh
```

All four commands are required for a clean workspace. The APSL verification script checks every specification and asserts the documented negative fixtures fail at their intended gates.

## License

Apache-2.0
