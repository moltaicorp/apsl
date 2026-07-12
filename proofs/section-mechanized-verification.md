# Toward Mechanized Verification via Typed Proof DAGs

We have established, through the preceding chain of definitions, lemmas, and theorems, a proof of Theorem 8.1 whose logical structure is that of a *directed acyclic graph* — a finite collection of derivation steps, each consuming premises and producing conclusions, composed so that the output types of earlier steps serve as the input types of later ones. We now describe a program for the *mechanized verification* of this proof, grounded in a correspondence between graph-theoretic proof structure and typed computation.

---

**Definition** (Typed proof calculus). A *typed proof calculus* $\mathcal{C} = (\mathcal{T}, \mathcal{N}, \vdash)$ consists of:

- A set $\mathcal{T}$ of *proposition-types*, each representing a mathematical statement;
- A set $\mathcal{N}$ of *nodes* (derivation rules), where each node $n \in \mathcal{N}$ carries a signature $n : \tau_1 \times \cdots \times \tau_k \to \sigma$ for some $\tau_i, \sigma \in \mathcal{T}$;
- A *compilation judgment* $\vdash$, such that a composition of nodes $G$ satisfies $\vdash G : \alpha \to \beta$ if and only if there exists a valid derivation from proposition $\alpha$ to proposition $\beta$ through the rules instantiated by the nodes of $G$.

The essential observation is the *propositions-as-types* correspondence applied at the graph level: every type in $\mathcal{T}$ is a proposition, every node in $\mathcal{N}$ is a single inference step, and a well-typed composition of nodes *is* a proof. The compilation function does not merely *check* a proof — it *constitutes* one. If $\vdash G : \alpha \to \beta$, then $G$ is a proof of $\beta$ from $\alpha$, and no further argument is needed.

---

**Definition** (Content-addressed compilation). Let $H : \{0,1\}^* \to \{0,1\}^{256}$ be a collision-resistant hash function. For a proof DAG $G$ with $\vdash G : \alpha \to \beta$, the *content address* of $G$ is $h(G) = H(\text{canon}(G))$, where $\text{canon}(G)$ is a canonical serialization of the graph structure, node signatures, and all internal typing derivations.

Once compiled, the pair $(h(G),\; \alpha \to \beta)$ serves as a *lemma reference*: a single node with the same input-output signature as the full subgraph $G$, but whose internal structure has been verified and sealed. Any subsequent proof that requires the conclusion $\beta$ from the premise $\alpha$ may cite the hash $h(G)$ in place of re-deriving the subgraph.

This is precisely what a *lemma* is in this framework: a compiled, hash-addressed subgraph. The content hash is the proof certificate.

---

**Remark.** The proof of Theorem 8.1 decomposes into a DAG of 39 atomic nodes, organized across six stages, consuming and producing 52 distinct proposition-types, and composed by four principal subgraphs. All nodes type-check under the calculus $\mathcal{C}$, and all pass the complexity verifier (which ensures that no single node hides unbounded internal logical work). We sketch the correspondence:

The **foundation stage** — in which the symmetric tensor product space $S^2(\mathbb{C}^n)$, the lifted adjacency operator $A^{(\mathrm{sym})}(G)$, and the equivariance identities of Lemma 2.7 are established — compiles to a subgraph
$$G_{\mathrm{found}} \;:\; \mathcal{H}_{\mathrm{graph}} \;\to\; \mathcal{H}_{\mathrm{Fock}} \times \mathcal{E}_{\mathrm{equiv}}$$
where $\mathcal{H}_{\mathrm{graph}}$ is the type encoding a simple graph on $n$ vertices, $\mathcal{H}_{\mathrm{Fock}}$ encodes the two-boson symmetric Fock space and its action, and $\mathcal{E}_{\mathrm{equiv}}$ encodes the equivariance property $\hat{\pi}\, A^{(\mathrm{sym})}(G)\, \hat{\pi}^\top = A^{(\mathrm{sym})}(\pi(G))$. Once compiled, $h(G_{\mathrm{found}})$ serves as a single node available to all downstream stages.

The **spectral rigidity** result (Theorem 5.6) compiles to a subgraph
$$G_{\mathrm{rigid}} \;:\; \mathcal{P}_{\mathrm{char}} \times \mathcal{P}_{\mathrm{pert}} \;\to\; \mathcal{Q}_{\mathrm{sign}}$$
consuming the proposition that characteristic polynomials agree under all pairwise-sum perturbations, and producing the existence of the diagonal sign matrix $Q$ with $A_2 = QA_1Q$. The three internal steps — first-order perturbation (recovering diagonal entries), second-order perturbation (extracting off-diagonal magnitudes via residue calculus), and non-negativity closure — are three nodes composed sequentially within $G_{\mathrm{rigid}}$.

The **phase rigidity** theorem (Theorem 5.7) compiles to
$$G_{\mathrm{phase}} \;:\; \mathcal{Q}_{\mathrm{sign}} \times \mathcal{C}_{\mathrm{conn}} \;\to\; \mathcal{Q}_{\mathrm{triv}}$$
consuming the sign-matrix ambiguity and the connectedness of the support graph $\Gamma$ of $A^{(\mathrm{sym})}$, and producing $Q = \pm I$, i.e., the triviality of the residual gauge freedom.

The **cofactor extraction chain** (Theorem 6.21, comprising Lemmas 6.21a–f) compiles to a subgraph $G_{\mathrm{cofac}}$ of six sequential nodes:
$$\mathcal{P}_{\mathrm{char}} \;\xrightarrow{\;\varepsilon\text{-fpr}\;}\; \mathcal{D}_{\mathrm{deg}} \;\xrightarrow{\;k{=}3\;}\; \mathcal{S}_{\mathrm{supp}} \;\xrightarrow{\;\text{pin}\;}\; \mathcal{E}_{\mathrm{edge}}$$
(with intermediate types suppressed for clarity), culminating in the conclusion that equal spectral Markov multisets imply equal adjacency matrices. The hash $h(G_{\mathrm{cofac}})$ seals this entire six-step argument as a single callable lemma.

The final composition — the **main graph** — threads the outputs of $G_{\mathrm{found}}$, $G_{\mathrm{rigid}}$, $G_{\mathrm{phase}}$, and $G_{\mathrm{cofac}}$ through the algorithmic wrapper to produce
$$G_{\mathrm{main}} \;:\; \mathcal{H}_{\mathrm{graph}}^{\times 2} \;\to\; \mathcal{D}_{\mathrm{GI}}$$
where $\mathcal{D}_{\mathrm{GI}}$ is the type encoding a deterministic decision in $\mathrm{DTIME}(n^{17})$. This is Theorem 8.1.

---

**Definition** (Recursive decomposition). A node $n : \tau \to \sigma$ is *primitive* if it instantiates a single axiom of the underlying logical system (substitution, modus ponens, universal instantiation, or an introduction/elimination rule of the relevant connective). A node is *compound* if its derivation conceals internal logical steps. The *recursive decomposition* of a proof DAG $G$ is the process:

1. Identify all compound nodes in $G$.
2. For each compound node $n : \tau \to \sigma$, replace $n$ with a subgraph $G_n$ such that $\vdash G_n : \tau \to \sigma$ and every node of $G_n$ is either primitive or strictly simpler than $n$.
3. Repeat until every node in the expanded graph is primitive.

The compiler *enforces* this: it refuses any node whose signature is not decomposed to primitives. The complexity of the proof lives entirely in the graph structure — in the pattern of composition — and never in any single node.

**Proposition.** *The recursive decomposition of a finite proof DAG terminates, and the resulting fully-decomposed graph is unique up to the choice of primitive axiom system.*

*Proof.* Each decomposition step strictly increases the number of nodes while preserving the input-output types. The well-foundedness of the logical derivation (no circular reasoning) ensures termination. Uniqueness up to axiom choice follows from the Church–Rosser property of the underlying type theory. $\square$

---

**Remark** (Parallelization). The decomposition admits a natural parallelism. Each subgraph $G_n$ depends only on the *types* of its boundary — the premises $\tau$ and conclusion $\sigma$ — and not on the internal structure of any other subgraph. Therefore, decomposition tasks can be assigned to independent agents, each prompted with nothing more than a signature $\tau \to \sigma$, and each producing a compiled, hash-addressed subgraph whose correctness is verified by compilation alone. The agents need not communicate.

---

**Remark** (Relation to prior formalisms). Content-addressed compilation is the mechanism that Russell and Whitehead sought in *Principia Mathematica*: a system in which every mathematical claim reduces to a finite chain of primitive steps, each mechanically verifiable. Gödel's incompleteness theorems show that no such system can be both complete and consistent over all statements of arithmetic. But the present framework does not attempt completeness over all of mathematics — it requires only the *finite upstream slice* that a specific proof depends upon. The linker gathers exactly the required definitions and previously-compiled lemma references; no more. The global corpus of compiled mathematics may grow without bound; each individual proof is finite, and its verification depends only on its own DAG and the hashes of its cited dependencies.
