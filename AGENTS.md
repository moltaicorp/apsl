# AGENTS.md — APSL Agent Collaboration Protocol

## How agents implement APSL nodes

Each APSL node is a **pure function** with typed inputs and outputs.
No node has cross-state dependencies on any other node. The APSL
spec is the complete interface contract. If your implementation
compiles against the spec, it composes with every other node in the
flow graph. You do not need to read, understand, or coordinate with
other nodes' implementations.

## Per-node agent context

When spawning an agent to implement a single APSL node, provide exactly:

1. **The node signature and clauses** from the .apsl source file:
   ```
   vault_read : (w: World, args: String[]) -> (World, SecretValue)
     pre   len args >= 2
     cx    O(1) idem-complex
     sla   d <= 1/1000, T <= 200ms
   ```

2. **The target file path** where the implementation goes:
   ```
   crates/your-crate/src/lib.rs  # or a new module file
   ```

3. **The APSL validation commands** to run after implementation:
   ```bash
   # Verify the spec still parses and typechecks with your node
   cargo run --bin apslc -- check <spec-file>

   # Verify the cert is still valid for your node
   cargo run -p apsl-cert-cli -- verify <node-hash> --pub demo

   # Verify the Rust implementation compiles
   cd <your-workspace>
   cargo check -p <your-crate>

   # Run your crate's tests
   cargo test -p <your-crate>
   ```

4. **The node's cert hash** from the .apsl-store:
   ```
   efaf2f34ad956397a9f39dfcb1a801ea0b696467c9689e71302f283e32b21642
   ```

That is the complete context. No other information is required.

## What the APSL pipeline guarantees

| Stage | What it proves | Command |
|-------|---------------|---------|
| parse | Syntax valid, AST well-formed | `apslc parse <file>` |
| check | Types unify, flow graphs compose | `apslc check <file>` |
| lint:complex | Derived complexity ≤ O(n log n) | `apsl-lint complex <file>` |
| lint:pred | Pre/post predicates discharged by Z3 | `apsl-lint pred <file>` |
| cert:emit | All above + Ed25519 signed | `apsl-cert emit <file> --key <name>` |
| cert:verify | Cert valid against pinned TCB | `apsl-cert verify <hash> --key <name>` |

## What the APSL pipeline does NOT guarantee

- That the Rust implementation satisfies the spec's pre/post predicates.
  The spec proves the predicates are *internally consistent*; the
  implementation must be tested against them. Write tests that exercise
  every `pre` as an input validation check and every `post` as an
  output assertion.

- That the Rust implementation runs in the certified complexity bound.
  The spec proves the *specified algorithm* is bounded; it says nothing
  about the implementation's actual runtime. You could implement an
  O(n log n)-certified node with a bubble sort and the cert stays green.
  APSL is a proof system for specifications, not a runtime verifier.
  Complexity testing is your responsibility. Your implementation
  must follow that shape. If the spec says O(n log n) and you write a
  nested loop, the cert is still valid but your code is wrong.

The bridge between certified spec and verified implementation is:
1. Rust's type system (enforces signature match)
2. Unit tests (enforce pre/post predicates)
3. Future: `via @implementation verified_by=<tool>` clause

## Compositional proof = no coordination

If node A's output type matches node B's input type in the APSL spec,
and both Rust implementations match their respective APSL signatures,
then A and B compose correctly. This is proven by the flow graph
typecheck (`apslc check`). Agents implementing A and B never need to
communicate.

This means N nodes can be implemented by N independent agents in
parallel with zero coordination overhead. The APSL cert is the
async collaboration protocol.
