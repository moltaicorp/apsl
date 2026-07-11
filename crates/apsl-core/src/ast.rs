use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Ident(pub String);

impl Ident {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Ident {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Span {
    pub line: u32,
    pub col: u32,
    pub len: u32,
}

impl Span {
    pub const NONE: Span = Span {
        line: 0,
        col: 0,
        len: 0,
    };
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Type {
    Base(Ident),

    Parameterized(Ident, Vec<Type>),
    Record(Vec<(Ident, Box<Type>)>),
    List(Box<Type>),
    Tuple(Vec<Type>),
    Var(u32),
    Result(Box<Type>),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Param {
    pub name: Ident,
    pub ty: Type,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TypeSig {
    pub params: Vec<Param>,
    pub ret: Type,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Lit {
    Int(i128),
    Rat(i128, u128),
    Bool(bool),
    Str(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BinOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    And,
    Or,
    Subset,
    Union,
    Intersect,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum UnOp {
    Not,
    Neg,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Quant {
    Forall,
    Exists,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Expr {
    Lit(Lit, Span),
    Var(Ident, Span),
    Field(Box<Expr>, Ident, Span),
    Apply(Ident, Vec<Expr>, Span),
    Bin(BinOp, Box<Expr>, Box<Expr>, Span),
    Un(UnOp, Box<Expr>, Span),
    Quant(Quant, Ident, Box<Expr>, Box<Expr>, Span),
    If(Box<Expr>, Box<Expr>, Box<Expr>, Span),
    Let(Ident, Box<Expr>, Box<Expr>, Span),
    Tuple(Vec<Expr>, Span),
    Lam(Vec<Ident>, Box<Expr>, Span),
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::Lit(_, s)
            | Expr::Var(_, s)
            | Expr::Field(_, _, s)
            | Expr::Apply(_, _, s)
            | Expr::Bin(_, _, _, s)
            | Expr::Un(_, _, s)
            | Expr::Quant(_, _, _, _, s)
            | Expr::If(_, _, _, s)
            | Expr::Let(_, _, _, s)
            | Expr::Tuple(_, s)
            | Expr::Lam(_, _, s) => s.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AuthLevel {
    None,
    Bearer,
    Session,
    Passkey,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ScopeConstraint {
    Any,
    Narrowing,
    Admitted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AuditReq {
    None,
    Before,
    After,
    Both,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RuntimeClass {
    Idem,
    IdemComplex,
    AntiIdem,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CxSpec {
    pub bigo: CxExpr,
    pub class: RuntimeClass,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CxExpr {
    Const,
    Size(Ident),
    NLogN(Ident),
    LogN(Ident),
    Sum(Vec<CxExpr>),
    Prod(Vec<CxExpr>),
    Max(Vec<CxExpr>),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Duration {
    pub ns: u128,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Sla {
    pub epsilon: (i128, u128),
    pub delta: (i128, u128),
    pub t: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Via {
    pub tag: Ident,
    pub attrs: Vec<(Ident, Ident)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TypeAlias {
    pub name: Ident,
    pub rhs: Type,
    pub supertypes: Vec<Ident>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StateDecl {
    pub key: Ident,
    pub ty: Type,
    pub default: Option<Lit>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Bind {
    pub name: Ident,
    pub key: Ident,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct DeployClauses {
    pub emits: Option<Ident>,
    pub stage: Option<Ident>,
    pub image: Option<Ident>,
    pub needs: Vec<Ident>,
    pub binds: Vec<Bind>,
    pub gate: Option<Expr>,
    pub proof: Option<Ident>,
    pub delegation: Option<Ident>,
}

impl DeployClauses {
    pub fn is_empty(&self) -> bool {
        self.emits.is_none()
            && self.stage.is_none()
            && self.image.is_none()
            && self.needs.is_empty()
            && self.binds.is_empty()
            && self.gate.is_none()
            && self.proof.is_none()
            && self.delegation.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Node {
    pub name: Ident,
    pub sig: TypeSig,
    pub pre: Vec<Expr>,
    pub post: Vec<Expr>,
    pub cx: CxSpec,
    pub sla: Option<Sla>,
    pub via: Option<Via>,
    pub auth: AuthLevel,
    pub scope_constraint: ScopeConstraint,
    pub audit_req: AuditReq,
    pub state: Vec<StateDecl>,
    pub deploy: Option<DeployClauses>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FlowStep {
    pub nodes: Vec<Ident>,
    pub span: Span,
}

impl FlowStep {
    pub fn single(node: Ident, span: Span) -> Self {
        Self {
            nodes: vec![node],
            span,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Graph {
    pub name: Ident,
    pub sig: TypeSig,
    pub post: Vec<Expr>,
    pub flow: Vec<Vec<FlowStep>>,
    pub state: Vec<StateDecl>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Decl {
    Type(TypeAlias),
    Node(Box<Node>),
    Graph(Graph),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Program {
    pub decls: Vec<Decl>,
}

impl Program {
    pub fn new() -> Self {
        Self::default()
    }
}
