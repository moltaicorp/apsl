# AGENTS.md — APSL Agent Collaboration Protocol

## Scope

An APSL node is a pure specification unit with typed inputs and outputs. Nodes compose through graph edges. `apslc check` proves that declared edges type-check; it does not inspect or verify a Rust implementation.

Agents may implement independent nodes in parallel when their target files do not overlap. APSL composition removes the need to coordinate node semantics beyond the shared specification, but normal source-control coordination still applies to shared files and build configuration.

## Per-node context

Provide an implementing agent:

1. The node signature and every clause from the APSL source.
2. The implementation target path.
3. The crate or workspace validation commands.
4. The certificate hash only after certificate emission succeeds.

Example node:

```apsl
type SecretValue = String

vault_read : (w: World, args: String[]) -> (World, SecretValue)
  pre   len args >= 2
  cx    O(1) idem-complex
  sla   d <= 1/1000, T <= 200ms
```

Example validation sequence:

```bash
cargo run --bin apslc -- check SPEC_FILE
cargo run -p apsl-lint -- check SPEC_FILE
cargo check -p CRATE
cargo test -p CRATE
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

When all specification gates pass, emit certificates:

```bash
cargo run -p apsl-cert-cli -- emit SPEC_FILE --key KEY_NAME
cargo run -p apsl-cert-cli -- verify HASH --pub KEY_NAME
```

`apsl-cert emit` prints the certificate hash for each certified node. It refuses to store certificates when linking, typing, complexity, or predicate verification fails.

## Tool guarantees

| Stage | Successful result | Command |
|---|---|---|
| Parse | Syntax is valid and the AST is canonicalizable. | `apslc parse FILE` |
| Check | Symbols resolve and graph edges type-check. | `apslc check FILE` |
| Complexity | Derived predicate complexity is within the membership bound. | `apsl-lint complex FILE` |
| Predicates | Every clause receives a `Proved` solver verdict. | `apsl-lint pred FILE` |
| Certificate emission | All preceding specification gates pass and signed records are stored. | `apsl-cert emit FILE --key NAME` |
| Certificate verification | Signature and pinned TCB match. | `apsl-cert verify HASH --pub NAME` |

## Implementation boundary

APSL certificates verify specifications, not arbitrary executable implementations. In particular, APSL does not prove:

- that implementation input validation enforces every `pre` clause;
- that implementation outputs satisfy every `post` clause;
- that implementation runtime matches the declared complexity;
- that an optional implementation hash identifies the artifact being executed.

Implementation tests must exercise preconditions, postconditions, failure behavior, and complexity independently. A successful process exit is execution evidence, not a contract proof.

## Composition

If node A's output type matches node B's input type, `apslc check` accepts their graph edge. Fan-in preserves ordinary tuple boundaries and deduplicates a shared `World<S>` only for world-threaded tuples.

This composition guarantee applies to the APSL graph. It does not replace implementation tests, repository-wide formatting, linting, or integration tests.
