# APSL — Abstract Protocol Schema Language

<p align="center">
  <img src="assets/apsl-banner-transparent.png" alt="APSL — the file is the machine" width="720">
</p>

[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![Rust 2021](https://img.shields.io/badge/Rust-2021-orange.svg)](https://www.rust-lang.org/)
[![Crates: 14](https://img.shields.io/badge/crates-14-green.svg)](#workspace)

APSL is a typed, certifiable specification language for composable protocol contracts.

If you've ever tried to coordinate 5 AI coding agents on the same codebase, you know the problem: they step on each other, nobody knows what anybody else is doing, and the resulting worktree merge is harder than the implementation was. APSL exists to make that scale to thousands.

The idea is simple. Every unit of work is a **node** — a pure function with typed inputs, typed outputs, pre/post predicates, a proven complexity bound, and an SLA. Nodes compose into **graphs** (flow pipelines). The compiler type-checks every edge. If node A's output type matches node B's input type, they compose — and that's proven, not hoped. An agent implementing node A never needs to talk to the agent implementing node B. The spec is the contract. The certificate is the collaboration protocol.

This means N agents can implement N nodes in parallel with zero coordination overhead — as long as each node's output type matches the next node's input type in the flow graph. No standups, no sync meetings, no merge conflicts on shared state. The APSL compiler is the arbiter: it parses, type-checks, derives complexity bounds from predicate structure, discharges predicates via SMT, and emits Ed25519-signed certificates. Each certificate witnesses that a node's spec is internally consistent, its complexity is ≤ O(n log n), and its pre/post predicates have been discharged via SMT (or flagged for review if the solver returns unknown or a counterexample). Specs are the artifact. Implementations are fungible. Certs prove verification.

## What APSL is, formally

APSL is a first-order, indentation-sensitive DSL with Hindley-Milner type inference. A `.apsl` file declares type aliases, nodes (pure functions with contracts), and graphs (flow compositions of nodes). The compiler pipeline: parse → link (symbol discovery, no imports) → typecheck (HM unification + flow composition) → complexity proof (derived from predicate structure) → SMT predicate discharge (Z3/CVC5) → Ed25519-signed certificate. Certificates are content-addressed (sha256), stored in a flat-file sharded store, and verifiable by anyone with the signer's public key. The membership predicate — every node's derived complexity must be ≤ O(n log n) per input size variable — is the complexity gate that keeps the language honest. Multi-variable terms like O(n·m) across distinct inputs are admitted; O(n²) over a single input is not.

APSL supports parameterized types: `World<S>` where `S` is the RootState identity. `World<A>` does not unify with `World<B>` — different state identities cannot compose. The flow checker supports fan-in: `flow (a, b) -> c` feeds the tuple of a's and b's outputs into c. When nodes thread `World<S>`, tuple sources flatten the World automatically: `((World<S>, X), (World<S>, Y))` becomes `(World<S>, X, Y)`.

## CLI Reference

### `apslc` — compiler

```
apslc parse <file>   print canonical AST to stdout
apslc canon <file>   same — canonical form IS the serialization
apslc hash  <file>   print sha256 hex of canonical form
apslc check <file>   parse + link + type-check, exit 0 if clean
apslc deploy <file>  emit GitLab child-pipeline YAML from CI/CD-definition graph
```

Flags:

| Flag | What it does |
|------|-------------|
| `--search-path <dirs>` | Colon-separated directories to search for symbols |
| `--no-resolve` | Disable linker (error on unresolved symbols) |
| `--show-deps` | Print resolved dependencies |
| `--state` | Enforce state clause validation |
| `--nominal` | Enforce nominal type equality (no structural aliasing) |
| `--restricted` | Enforce capability narrowing (implies --nominal) |
| `--strict` | Reject coarse types: every type alias must resolve to a unique structure |
| `--rooted` | Reject bare `World` (must use `World<S>`) and enforce single-root connectedness |
| `--migrate` | Strip unknown syntax for backward-compatible validation |
| `--attest [path]` | NO-STRINGS LAW: scan implementation source for bare/unattested string literals |

With `--attest`:

| Flag | What it does |
|------|-------------|
| `--count` | Print only the offender count (query mode, exit 0) |
| `--ratchet <file>` | Fault only on an INCREASE vs the baseline count in `<file>` |
| `--bless` | (with --ratchet) lower the baseline ceiling to the current count |

### `apsl-lint` — complexity prover + predicate discharge

```
apsl-lint check <file>            complexity + predicates
apsl-lint complex <file>          complexity only
apsl-lint pred <file>             predicate discharge only
apsl-lint explain <file> <node>   derived cost + reasoning
```

### `apsl-cert` — certificate analyzer

```
apsl-cert key new <name>             generate Ed25519 keypair
apsl-cert emit <file> --key <name>   verify file, emit signed cert per node, store
apsl-cert verify <hash> --pub <name>  verify a stored cert (loads <name>.pub)
apsl-cert verify <hash> --key <name>  verify (alias for --pub)
apsl-cert show <hash>                pretty-print a stored cert
```

Store layout: `./.apsl-store/<aa>/<bb>/<rest>.cert`

### Strictness layers

APSL's strictness flags compose. Each adds a constraint the compiler enforces:

1. **Default** (no flags): parse + link + typecheck. Types unify structurally. `World` is a plain base type.
2. `--nominal`: type aliases don't unify structurally — `Email` ≠ `String` even if `type Email = String`.
3. `--restricted`: `--nominal` + capability narrowing (outputs must be narrower than inputs).
4. `--strict`: every type alias must resolve to a unique structure (no two aliases with the same shape).
5. `--rooted`: `World` must be `World<S>` (parameterized with a RootState). A `.apsl` file must be one weakly-connected DAG with a single entry root. Different `World<S>` identities don't compose.

Flags stack: `apslc check spec.apsl --strict --rooted --state` enforces all five layers.

## Installation

```bash
git clone https://github.com/moltaicorp/apsl.git
cd apsl
cargo build --release
```

### Prerequisites

- **Rust** 1.70+ (2021 edition)
- **Z3** (for SMT predicate discharge) — install via `apt install z3` on Ubuntu, `brew install z3` on macOS, or download from [the Z3 release page](https://github.com/Z3Prover/z3/releases)
- **CVC5** (optional alternative SMT solver)

The build itself needs only Rust. Z3 is required only for `apsl-lint` predicate discharge and `apsl-cert emit`. If Z3 is not installed, the solver falls back to `Unknown` — certs are still emitted but predicate proofs carry no force.

> **Note:** `cargo build` produces warnings (unused imports, dead code in modules not yet wired to CLIs). These are expected in the current development stage and do not affect functionality.

Four binaries land in `target/release/`:

| Binary | What it does |
|--------|-------------|
| `apslc` | Parse, canonicalize, hash, type-check |
| `apsl-lint` | Complexity proof + SMT predicate discharge |
| `apsl-cert` | Key generation, certificate emission, verification |
| `apsl` | Graph runtime: run graphs, verify execution, list nodes |

No `import` or `use` keywords needed. Reference a symbol; the linker discovers it by scanning `.apsl` files on the search path.

## Quick Start

```bash
# Type-check the example pipeline
./target/release/apslc check examples/dedupe.apsl

# Run the complexity prover + SMT predicate discharge
./target/release/apsl-lint check examples/dedupe.apsl

# Generate a signing keypair and emit signed certificates
./target/release/apsl-cert key new demo
./target/release/apsl-cert emit examples/dedupe.apsl --key demo

# Verify a certificate (inspect with `apsl-cert show <hash>` first)
./target/release/apsl-cert show <hash>
./target/release/apsl-cert verify <hash> --pub demo
```

## Tutorials

### 1. Your first spec — `examples/dedupe.apsl`

```bash
cargo run --bin apslc -- check examples/dedupe.apsl
```

This spec defines an email deduplication pipeline. Four nodes compose into a graph:

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
  sla   e <= 0, d <= 1e-6, T <= 200ms

graph email_pipeline : String[] -> MessageId[]
  flow  in -> normalize -> dedupe -> classify -> send -> out
```

Each node has a typed signature, optional pre/post predicates, a complexity claim (`cx`), and an SLA. The graph composes them under `flow`. `apslc check` parses, links, and type-checks every edge. The `classify` node carries `via @statistical holdout=intents_v3` — a cert-provenance annotation saying its correctness is discharged by statistical evaluation, not SMT proof. APSL also supports `via @external service=<id>` for nodes whose correctness depends on a named external service (e.g., a vault, database, or identity provider). See [docs/SPEC.yaml](docs/SPEC.yaml) for the full `via` grammar.

Now run the full verification pipeline:

```bash
# Step 1: Complexity proof + SMT predicate discharge
cargo run --bin apsl-lint -- check examples/dedupe.apsl

# Step 2: Generate a signing keypair (one-time)
cargo run -p apsl-cert-cli -- key new demo

# Step 3: Emit signed certificates for all nodes
cargo run -p apsl-cert-cli -- emit examples/dedupe.apsl --key demo

# Step 4: Verify a certificate by hash (uses only the public key)
cargo run -p apsl-cert-cli -- verify <hash> --pub demo
```

> **Note:** The linter's SMT encoder treats `unique?` and `subseteq?` as uninterpreted predicates. Depending on your Z3 version, the solver may return `Unknown` or produce counterexamples for these. This is expected — the predicates are discharged on a best-effort basis. The structural guarantees (type-checking, complexity proof) always succeed; predicate discharge is best-effort.

> **Note:** `apsl-cert emit` signs certificates for all nodes regardless of SMT predicate discharge status. A certificate records the *verdict* (including `Unknown` or counterexample results), not just passing checks. Inspect the cert with `apsl-cert show <hash>` to see per-clause status.

### 2. World threading — `examples/world_threading.apsl`

APSL has no side-effect category. Anything that "side-effects" — a network call, a crypto signature, a state transition — is encoded as a pure function whose signature names the world explicitly:

```apsl
type World = Int
type Bearer = String
type Token = String
type Url = String
type Response = String

mint_token : (w: World, b: Bearer) -> (World, Token)
  cx    O(1) idem

http_post : (w: World, u: Url, body: Token) -> (World, Response)
  cx    O(1) idem-complex
  sla   d <= 1/1000, T <= 200ms
```

The `(World, X) -> (World, Y)` shape is how state is threaded without side effects. The math stays pure. Composition obligations remain "post of A implies pre of B." There is no `@cryptographic` or `@network` tag — the type signature already carries the obligation.

```bash
cargo run --bin apslc -- check examples/world_threading.apsl
```

### 3. When the linter pushes back — `examples/bad_n_squared.apsl`

```apsl
all_pairs_distinct : Int[] -> Bool
  post  forall x in in. forall y in in. x != y or x = y
  cx    O(n log n) idem
```

This node nests two quantifiers over the same input. The derived complexity is O(n²), which exceeds the O(n log n) membership gate. The linter rejects it and hints toward reformulation:

```bash
cargo run --bin apsl-lint -- check examples/bad_n_squared.apsl
```

This is the complexity gate in action — it catches O(n²) shapes at spec time, before any implementation is written. The gate also flags mismatches: if you declare `cx O(n)` but your predicates derive O(n log n), the linter reports an overpromise. It does not, however, inspect your implementation — a bubble sort behind an O(n log n) spec passes every check. APSL proves the spec is consistent, not that your code matches it.

## Workspace

| Crate | Role |
|---|---|
| `apsl-core` | AST, canonical JSON serialization, sha256 content addressing |
| `apsl-parse` | Indentation-sensitive lexer + recursive-descent parser |
| `apsl-types` | Hindley-Milner type checker, primitive environment |
| `apsl-complex` | O(n log n) complexity prover, parallelism hints |
| `apsl-smt` | SMT-LIB v2 encoder, Z3/CVC5 subprocess, counter-example explainer |
| `apsl-cert` | Ed25519 certificates, content-addressed store, TCB manifest |
| `apsl-link` | Symbol discovery (no imports — ripgrep-style parallel scan) |
| `apsl-runtime` | Graph executor + adapters (shell, vault) |
| `apsl-verify` | Numeric satisfaction solver (library only; not exposed via a CLI binary yet) |
| `apsl-workbench` | Web workbench: `cargo run -p apsl-workbench` starts an HTTP server on `localhost:7878` with routes for `/healthz`, `/compile`, `/build`, `/verify` |
| `apslc` | CLI: parse / canon / hash / check |
| `apsl-lint` | CLI: complex / pred / check |
| `apsl-cert-cli` | CLI: key new / emit / verify / show |
| `apsl` | APSL runtime binary: run graphs, verify execution records, list nodes |

## What APSL does NOT guarantee

The cert proves the **spec** is internally consistent. It does not prove any concrete implementation satisfies the spec. You could implement an O(n log n)-certified node with a bubble sort and the cert stays green. APSL is a proof system for specifications, not a runtime verifier. The bridge — `via @implementation verified_by=<tool>` — is documented as future work and does not exist in code yet.

Testing your implementation against the spec's pre/post predicates is your responsibility. The `apsl-runtime` crate can execute graphs by shelling out to named binaries, but its post-condition check is currently a stub (`// TODO`). The cert's `impl_hash` field is populated only when you pass `--impl-hash <hash>` to `apsl-cert emit`; without it, the field is empty. This lets you opt in to binding a certificate to a specific implementation artifact.

This is stated candidly because it matters: APSL narrows the problem from "is this code correct?" to "is this spec consistent, and does this implementation match this spec's signature?" The first is proven by the compiler. The second is enforced by Rust's type system and your tests.

### Expressiveness limits

APSL is first-order: no higher-order functions, no lambda abstraction, no type-level computation beyond unification. Predicates are restricted to what Z3/CVC5 can encode — if your post-condition requires reasoning about recursive data structures, non-linear arithmetic over unbounded domains, or heap shape, the solver will likely return `Unknown`. The complexity gate is syntactic: it derives Big-O from predicate structure and rejects anything exceeding O(n log n). There is no escape hatch. If your algorithm is genuinely O(n²), APSL is the wrong tool.

## Proofs and whitepapers

The `proofs/` directory contains formal work expressed as APSL specs:

- [**Principia Computia**](proofs/principia-computia.apsl) — A computation graph as a cryptographic object whose nodes are typed operators, edges are keys, and execution traces compose back to a passkey-rooted authority. If it compiles, the logical structure of the provenance framework is sound.
- [**GI ∈ DTIME(n¹⁷)**](proofs/proof-n17-full.apsl) — Complete APSL decomposition of the graph isomorphism proof chain. 39 atomic nodes, each performing exactly one logical step. Types are propositions; if it type-checks, the dependencies hold. Predicates are `post true` by design — the type-level composition IS the proof; SMT discharge confirms well-formedness, not mathematical truth of the underlying theorem.
- [**Spectral Rigidity**](proofs/spectral-rigidity.apsl) / [**Soundness & Phase Bound**](proofs/soundness-phase-bound.apsl) / [**Cofactor Chain**](proofs/cofactor-chain.apsl) — Decomposition of theorems from the n¹⁷ proof into atomic inference steps.

> **Note:** Proof decomposition files use `cx` annotations as structural metadata, not as complexity-gated claims. Some nodes carry `O(n*n)` because the decomposition step's complexity is quadratic in the graph size. The O(n log n) membership gate applies to application specs, not to proof decompositions.

**Decomposition files** (atomic step decompositions of the n¹⁷ proof chain):

- [decomp-cofactor-ab](proofs/decomp-cofactor-ab.apsl) / [decomp-cofactor-cd](proofs/decomp-cofactor-cd.apsl) / [decomp-cofactor-ef](proofs/decomp-cofactor-ef.apsl) — Cofactor extraction lemma decompositions (6.21a–f)
- [decomp-rigidity](proofs/decomp-rigidity.apsl) — Spectral rigidity step decomposition
- [decomp-critical-node](proofs/decomp-critical-node.apsl) / [decomp-false-node](proofs/decomp-false-node.apsl) — Critical and false node identification
- [decomp-round4a](proofs/decomp-round4a.apsl) / [decomp-round4b](proofs/decomp-round4b.apsl) — Round 4 decomposition
- [decomp-discriminant](proofs/decomp-discriminant.apsl) / [decomp-bound-assembly](proofs/decomp-bound-assembly.apsl) — Discriminant and bound assembly
- [axioms-algebra](proofs/axioms-algebra.apsl) / [axioms-analysis](proofs/axioms-analysis.apsl) / [axioms-ift](proofs/axioms-ift.apsl) — Axiom declarations (algebraic, analytic, IFT)
- [**Toward Mechanized Verification via Typed Proof DAGs**](proofs/section-mechanized-verification.md) — The propositions-as-types correspondence applied at the graph level: every type is a proposition, every node is a single inference step, a well-typed composition *is* a proof.

For the complete language specification, see [docs/SPEC.yaml](docs/SPEC.yaml). For a candid, code-level walkthrough of what every crate actually does (and what it doesn't), see [docs/APSL-FULL-IMPLEMENTATION.md](docs/APSL-FULL-IMPLEMENTATION.md). For the agent collaboration protocol, see [AGENTS.md](AGENTS.md).

## License

Apache-2.0
