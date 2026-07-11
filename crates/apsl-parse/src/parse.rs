use apsl_core::ast::{
    AuditReq, AuthLevel, BinOp, Bind, CxExpr, CxSpec, Decl, DeployClauses, Duration, Expr,
    FlowStep, Graph, Ident, Lit, Node, Param, Program, Quant, RuntimeClass, ScopeConstraint, Sla,
    Span, StateDecl, Type as AstType, TypeAlias, TypeSig, UnOp, Via,
};

use crate::lex::{Tok, TokKind};

#[derive(Debug, Clone)]
pub struct ParseError {
    pub msg: String,
    pub span: Span,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "parse error at line {} col {}: {}",
            self.span.line, self.span.col, self.msg
        )
    }
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
    allow_dot_postfix: bool,
}

impl Parser {
    fn new(toks: Vec<Tok>) -> Self {
        Self {
            toks,
            pos: 0,
            allow_dot_postfix: true,
        }
    }

    fn peek(&self) -> &TokKind {
        &self.toks[self.pos].kind
    }
    fn span(&self) -> Span {
        self.toks[self.pos].span.clone()
    }

    fn bump(&mut self) -> Tok {
        let t = self.toks[self.pos].clone();
        if !matches!(t.kind, TokKind::Eof) {
            self.pos += 1;
        }
        t
    }

    fn skip_newlines(&mut self) {
        while matches!(self.peek(), TokKind::Newline) {
            self.bump();
        }
    }

    fn expect(&mut self, k: &TokKind, what: &str) -> Result<Tok, ParseError> {
        if std::mem::discriminant(self.peek()) == std::mem::discriminant(k) {
            Ok(self.bump())
        } else {
            Err(self.err(format!("expected {}, got {:?}", what, self.peek())))
        }
    }

    fn err(&self, msg: String) -> ParseError {
        ParseError {
            msg,
            span: self.span(),
        }
    }

    fn parse_program(&mut self) -> Result<Program, ParseError> {
        let mut p = Program::new();
        self.skip_newlines();
        while !matches!(self.peek(), TokKind::Eof) {
            let d = self.parse_decl()?;
            p.decls.push(d);
            self.skip_newlines();
        }
        Ok(p)
    }

    fn parse_decl(&mut self) -> Result<Decl, ParseError> {
        let start = self.span();
        match self.peek().clone() {
            TokKind::Ident(s) if s == "type" => {
                self.bump();
                let name = self.parse_type_name()?;
                let mut supertypes = Vec::new();
                if matches!(self.peek(), TokKind::SubtypeOf) {
                    self.bump();
                    supertypes.push(self.parse_type_name()?);
                    while matches!(self.peek(), TokKind::Comma) {
                        self.bump();
                        supertypes.push(self.parse_type_name()?);
                    }
                }
                let rhs = if matches!(self.peek(), TokKind::Eq) {
                    self.bump();
                    self.parse_type()?
                } else if !supertypes.is_empty() {
                    AstType::Base(name.clone())
                } else {
                    return Err(self.err("expected `=` or `<:` in type declaration".into()));
                };
                self.skip_newlines();
                Ok(Decl::Type(TypeAlias {
                    name,
                    rhs,
                    supertypes,
                    span: start,
                }))
            }
            TokKind::Ident(s) if s == "graph" => {
                self.bump();
                let g = self.parse_graph(start)?;
                Ok(Decl::Graph(g))
            }
            TokKind::Ident(_) => {
                let n = self.parse_node(start)?;
                Ok(Decl::Node(Box::new(n)))
            }
            TokKind::TypeName(_) => {
                let n = self.parse_node(start)?;
                Ok(Decl::Node(Box::new(n)))
            }
            _ => Err(self.err(format!("expected a declaration, got {:?}", self.peek()))),
        }
    }

    fn parse_type_name(&mut self) -> Result<Ident, ParseError> {
        match self.peek().clone() {
            TokKind::TypeName(s) => {
                self.bump();
                Ok(Ident::new(s))
            }
            other => Err(self.err(format!("expected a type name, got {:?}", other))),
        }
    }

    fn parse_ident(&mut self) -> Result<Ident, ParseError> {
        match self.peek().clone() {
            TokKind::Ident(s) => {
                self.bump();
                Ok(Ident::new(s))
            }
            TokKind::TypeName(s) => {
                self.bump();
                Ok(Ident::new(s))
            }
            other => Err(self.err(format!("expected an identifier, got {:?}", other))),
        }
    }

    fn parse_type(&mut self) -> Result<AstType, ParseError> {
        let mut t = self.parse_type_atom()?;
        while matches!(self.peek(), TokKind::BracketPair) {
            self.bump();
            t = AstType::List(Box::new(t));
        }
        Ok(t)
    }

    fn parse_type_atom(&mut self) -> Result<AstType, ParseError> {
        match self.peek().clone() {
            TokKind::TypeName(s) => {
                self.bump();
                let id = Ident::new(s);
                if matches!(self.peek(), TokKind::Lt) {
                    self.bump();
                    let mut args = vec![self.parse_type()?];
                    while matches!(self.peek(), TokKind::Comma) {
                        self.bump();
                        args.push(self.parse_type()?);
                    }
                    self.expect(&TokKind::Gt, "`>`")?;
                    Ok(AstType::Parameterized(id, args))
                } else {
                    Ok(AstType::Base(id))
                }
            }
            TokKind::Ident(s) => {
                self.bump();
                let id = Ident::new(s);
                if matches!(self.peek(), TokKind::Lt) {
                    self.bump();
                    let mut args = vec![self.parse_type()?];
                    while matches!(self.peek(), TokKind::Comma) {
                        self.bump();
                        args.push(self.parse_type()?);
                    }
                    self.expect(&TokKind::Gt, "`>`")?;
                    Ok(AstType::Parameterized(id, args))
                } else {
                    Ok(AstType::Base(id))
                }
            }
            TokKind::LParen => {
                self.bump();
                let mut ts = Vec::new();
                ts.push(self.parse_type()?);
                while matches!(self.peek(), TokKind::Comma) {
                    self.bump();
                    ts.push(self.parse_type()?);
                }
                self.expect(&TokKind::RParen, "`)`")?;
                Ok(if ts.len() == 1 {
                    ts.remove(0)
                } else {
                    AstType::Tuple(ts)
                })
            }
            TokKind::LBrace => {
                self.bump();
                let mut fields = Vec::new();
                if !matches!(self.peek(), TokKind::RBrace) {
                    loop {
                        let fname = self.parse_ident()?;
                        self.expect(&TokKind::Colon, "`:` in record field")?;
                        let fty = self.parse_type()?;
                        fields.push((fname, Box::new(fty)));
                        if matches!(self.peek(), TokKind::Comma) {
                            self.bump();
                        } else {
                            break;
                        }
                    }
                }
                self.expect(&TokKind::RBrace, "`}`")?;
                Ok(AstType::Record(fields))
            }
            other => Err(self.err(format!("expected a type, got {:?}", other))),
        }
    }

    fn parse_typesig(&mut self) -> Result<TypeSig, ParseError> {
        if matches!(self.peek(), TokKind::LParen) {
            let save = self.pos;
            if let Ok((params, ret)) = self.parse_named_params() {
                return Ok(TypeSig { params, ret });
            }
            self.pos = save;
        }
        let t_in = self.parse_type()?;
        self.expect(&TokKind::Arrow, "`->`")?;
        let t_out = self.parse_type()?;
        let params = match t_in {
            AstType::Tuple(ts) => ts
                .into_iter()
                .enumerate()
                .map(|(i, t)| Param {
                    name: Ident::new(if i == 0 {
                        "in".to_string()
                    } else {
                        format!("in{}", i)
                    }),
                    ty: t,
                })
                .collect(),
            other => vec![Param {
                name: Ident::new("in"),
                ty: other,
            }],
        };
        Ok(TypeSig { params, ret: t_out })
    }

    fn parse_named_params(&mut self) -> Result<(Vec<Param>, AstType), ParseError> {
        self.expect(&TokKind::LParen, "`(`")?;
        let mut params = Vec::new();
        if !matches!(self.peek(), TokKind::RParen) {
            loop {
                let save = self.pos;
                if let TokKind::Ident(n) = self.peek().clone() {
                    let nspan = self.span();
                    self.bump();
                    if matches!(self.peek(), TokKind::Colon) {
                        self.bump();
                        let ty = self.parse_type()?;
                        params.push(Param {
                            name: Ident::new(n),
                            ty,
                        });
                        if matches!(self.peek(), TokKind::Comma) {
                            self.bump();
                            continue;
                        } else {
                            break;
                        }
                    } else {
                        self.pos = save;
                        let _ = nspan;
                    }
                }
                let ty = self.parse_type()?;
                let i = params.len();
                params.push(Param {
                    name: Ident::new(if i == 0 {
                        "in".to_string()
                    } else {
                        format!("in{}", i)
                    }),
                    ty,
                });
                if matches!(self.peek(), TokKind::Comma) {
                    self.bump();
                    continue;
                } else {
                    break;
                }
            }
        }
        self.expect(&TokKind::RParen, "`)`")?;
        self.expect(&TokKind::Arrow, "`->`")?;
        let ret = self.parse_type()?;
        Ok((params, ret))
    }

    fn parse_node(&mut self, start: Span) -> Result<Node, ParseError> {
        let name = self.parse_ident()?;
        self.expect(&TokKind::Colon, "`:`")?;
        let sig = self.parse_typesig()?;
        self.expect(&TokKind::Newline, "newline")?;
        self.skip_newlines();
        let mut pre = Vec::new();
        let mut post = Vec::new();
        let mut cx: Option<CxSpec> = None;
        let mut sla: Option<Sla> = None;
        let mut via: Option<Via> = None;
        let mut auth = AuthLevel::None;
        let mut scope_constraint = ScopeConstraint::Any;
        let mut audit_req = AuditReq::None;
        let mut state_decls: Vec<StateDecl> = Vec::new();
        let mut dep = DeployClauses::default();
        if matches!(self.peek(), TokKind::Indent) {
            self.bump();
            loop {
                self.skip_newlines();
                if matches!(self.peek(), TokKind::Dedent | TokKind::Eof) {
                    break;
                }
                match self.peek().clone() {
                    TokKind::Ident(k) if k == "pre" => {
                        self.bump();
                        pre.extend(self.parse_pred_list()?);
                    }
                    TokKind::Ident(k) if k == "post" => {
                        self.bump();
                        post.extend(self.parse_pred_list()?);
                    }
                    TokKind::Ident(k) if k == "cx" => {
                        self.bump();
                        cx = Some(self.parse_cx_spec()?);
                    }
                    TokKind::Ident(k) if k == "sla" => {
                        self.bump();
                        sla = Some(self.parse_sla_spec()?);
                    }
                    TokKind::Ident(k) if k == "via" => {
                        self.bump();
                        via = Some(self.parse_via_spec()?);
                    }
                    TokKind::Ident(k) if k == "emits" => {
                        self.bump();
                        dep.emits = Some(self.parse_ident()?);
                    }
                    TokKind::Ident(k) if k == "stage" => {
                        self.bump();
                        dep.stage = Some(self.parse_dashed_ident()?);
                    }
                    TokKind::Ident(k) if k == "image" => {
                        self.bump();
                        dep.image = Some(self.parse_state_key()?);
                    }
                    TokKind::Ident(k) if k == "needs" => {
                        self.bump();
                        dep.needs = self.parse_needs_list()?;
                    }
                    TokKind::Ident(k) if k == "binds" => {
                        self.bump();
                        let b = self.parse_bind()?;
                        dep.binds.push(b);
                    }
                    TokKind::Ident(k) if k == "gate" => {
                        self.bump();
                        dep.gate = Some(self.parse_expr()?);
                    }
                    TokKind::Ident(k) if k == "proof" => {
                        self.bump();
                        dep.proof = Some(self.parse_ident()?);
                    }
                    TokKind::Ident(k) if k == "delegation" => {
                        self.bump();
                        dep.delegation = Some(self.parse_key_path()?);
                    }
                    TokKind::Ident(k) if k == "auth" => {
                        self.bump();
                        auth = match self.peek().clone() {
                            TokKind::Ident(a) if a == "none" => {
                                self.bump();
                                AuthLevel::None
                            }
                            TokKind::Ident(a) if a == "bearer" => {
                                self.bump();
                                AuthLevel::Bearer
                            }
                            TokKind::Ident(a) if a == "session" => {
                                self.bump();
                                AuthLevel::Session
                            }
                            TokKind::Ident(a) if a == "passkey" => {
                                self.bump();
                                AuthLevel::Passkey
                            }
                            other => {
                                return Err(self.err(format!(
                                    "expected auth level (none|bearer|session|passkey), got {:?}",
                                    other
                                )))
                            }
                        };
                    }
                    TokKind::Ident(k) if k == "scope" => {
                        self.bump();
                        scope_constraint = match self.peek().clone() {
                            TokKind::Ident(s) if s == "any" || s == "*" => {
                                self.bump();
                                ScopeConstraint::Any
                            }
                            TokKind::Ident(s) if s == "narrowing" => {
                                self.bump();
                                ScopeConstraint::Narrowing
                            }
                            TokKind::Ident(s) if s == "admitted" => {
                                self.bump();
                                ScopeConstraint::Admitted
                            }
                            other => {
                                return Err(self.err(format!(
                                    "expected scope (any|narrowing|admitted), got {:?}",
                                    other
                                )))
                            }
                        };
                    }
                    TokKind::Ident(k) if k == "audit" => {
                        self.bump();
                        audit_req = match self.peek().clone() {
                            TokKind::Ident(a) if a == "none" => {
                                self.bump();
                                AuditReq::None
                            }
                            TokKind::Ident(a) if a == "before" => {
                                self.bump();
                                AuditReq::Before
                            }
                            TokKind::Ident(a) if a == "after" => {
                                self.bump();
                                AuditReq::After
                            }
                            TokKind::Ident(a) if a == "both" => {
                                self.bump();
                                AuditReq::Both
                            }
                            other => {
                                return Err(self.err(format!(
                                    "expected audit (none|before|after|both), got {:?}",
                                    other
                                )))
                            }
                        };
                    }
                    TokKind::Ident(k) if k == "state" => {
                        self.bump();
                        let sd = self.parse_state_decl()?;
                        state_decls.push(sd);
                    }
                    other => {
                        return Err(self.err(format!("expected clause keyword, got {:?}", other)))
                    }
                }
                self.skip_newlines();
            }
            if matches!(self.peek(), TokKind::Dedent) {
                self.bump();
            }
        }
        let cx = cx.unwrap_or(CxSpec {
            bigo: CxExpr::Const,
            class: RuntimeClass::Idem,
        });
        let deploy = if dep.is_empty() { None } else { Some(dep) };
        Ok(Node {
            name,
            sig,
            pre,
            post,
            cx,
            sla,
            via,
            auth,
            scope_constraint,
            audit_req,
            state: state_decls,
            deploy,
            span: start,
        })
    }

    fn parse_dashed_ident(&mut self) -> Result<Ident, ParseError> {
        let mut s = match self.peek().clone() {
            TokKind::Ident(x) => {
                self.bump();
                x
            }
            TokKind::TypeName(x) => {
                self.bump();
                x
            }
            other => return Err(self.err(format!("expected a name, got {:?}", other))),
        };
        while matches!(self.peek(), TokKind::Minus) {
            self.bump();
            match self.peek().clone() {
                TokKind::Ident(x) => {
                    self.bump();
                    s.push('-');
                    s.push_str(&x);
                }
                TokKind::TypeName(x) => {
                    self.bump();
                    s.push('-');
                    s.push_str(&x);
                }
                other => {
                    return Err(self.err(format!("expected a name after `-`, got {:?}", other)))
                }
            }
        }
        Ok(Ident::new(s))
    }

    fn parse_key_path(&mut self) -> Result<Ident, ParseError> {
        let mut s = self.parse_dashed_ident()?.as_str().to_string();
        while matches!(self.peek(), TokKind::Slash) {
            self.bump();
            let seg = self.parse_dashed_ident()?;
            s.push('/');
            s.push_str(seg.as_str());
        }
        Ok(Ident::new(s))
    }

    fn parse_state_key(&mut self) -> Result<Ident, ParseError> {
        self.expect(&TokKind::At, "`@`")?;
        match self.peek().clone() {
            TokKind::Ident(s) if s == "state" => {
                self.bump();
            }
            other => return Err(self.err(format!("expected `state` after `@`, got {:?}", other))),
        }
        self.parse_key_path()
    }

    fn parse_bind(&mut self) -> Result<Bind, ParseError> {
        let name = self.parse_ident()?;
        let key = self.parse_state_key()?;
        Ok(Bind { name, key })
    }

    fn parse_needs_list(&mut self) -> Result<Vec<Ident>, ParseError> {
        if matches!(self.peek(), TokKind::BracketPair) {
            self.bump();
            return Ok(Vec::new());
        }
        self.expect(&TokKind::LBracket, "`[`")?;
        let mut out = Vec::new();
        if !matches!(self.peek(), TokKind::RBracket) {
            out.push(self.parse_ident()?);
            while matches!(self.peek(), TokKind::Comma) {
                self.bump();
                out.push(self.parse_ident()?);
            }
        }
        self.expect(&TokKind::RBracket, "`]`")?;
        Ok(out)
    }

    fn parse_state_decl(&mut self) -> Result<StateDecl, ParseError> {
        let sp = self.span();
        let key = self.parse_ident()?;
        self.expect(&TokKind::Colon, "`:` after state key name")?;
        let ty = self.parse_type()?;
        let default = if matches!(self.peek(), TokKind::Eq) {
            self.bump();
            Some(self.parse_lit()?)
        } else {
            None
        };
        Ok(StateDecl {
            key,
            ty,
            default,
            span: sp,
        })
    }

    fn parse_lit(&mut self) -> Result<Lit, ParseError> {
        match self.peek().clone() {
            TokKind::IntLit(n) => {
                self.bump();
                Ok(Lit::Int(n))
            }
            TokKind::RatLit(p, q) => {
                self.bump();
                Ok(Lit::Rat(p, q))
            }
            TokKind::BoolLit(b) => {
                self.bump();
                Ok(Lit::Bool(b))
            }
            TokKind::StrLit(s) => {
                self.bump();
                Ok(Lit::Str(s))
            }
            other => Err(self.err(format!(
                "expected literal value for state default, got {:?}",
                other
            ))),
        }
    }

    fn parse_graph(&mut self, start: Span) -> Result<Graph, ParseError> {
        let name = self.parse_ident()?;
        self.expect(&TokKind::Colon, "`:`")?;
        let sig = self.parse_typesig()?;
        self.expect(&TokKind::Newline, "newline")?;
        self.skip_newlines();
        let mut post = Vec::new();
        let mut flow: Vec<Vec<FlowStep>> = Vec::new();
        if matches!(self.peek(), TokKind::Indent) {
            self.bump();
            loop {
                self.skip_newlines();
                if matches!(self.peek(), TokKind::Dedent | TokKind::Eof) {
                    break;
                }
                match self.peek().clone() {
                    TokKind::Ident(k) if k == "post" => {
                        self.bump();
                        post.extend(self.parse_pred_list()?);
                    }
                    TokKind::Ident(k) if k == "flow" => {
                        self.bump();
                        flow.push(self.parse_flow_chain()?);
                    }
                    other => {
                        return Err(self.err(format!("expected post or flow, got {:?}", other)))
                    }
                }
                self.skip_newlines();
            }
            if matches!(self.peek(), TokKind::Dedent) {
                self.bump();
            }
        }
        Ok(Graph {
            name,
            sig,
            post,
            flow,
            state: Vec::new(),
            span: start,
        })
    }

    fn parse_pred_list(&mut self) -> Result<Vec<Expr>, ParseError> {
        let mut out = Vec::new();
        out.push(self.parse_expr()?);
        while matches!(self.peek(), TokKind::Comma) {
            self.bump();
            out.push(self.parse_expr()?);
        }
        Ok(out)
    }

    fn parse_flow_chain(&mut self) -> Result<Vec<FlowStep>, ParseError> {
        let mut out = Vec::new();
        out.push(self.parse_flow_step()?);
        loop {
            if matches!(self.peek(), TokKind::Arrow) {
                self.bump();
                out.push(self.parse_flow_step()?);
            } else if matches!(self.peek(), TokKind::Newline) {
                let save = self.pos;
                while matches!(self.peek(), TokKind::Newline) {
                    self.bump();
                }
                if matches!(self.peek(), TokKind::Indent) {
                    let save2 = self.pos;
                    self.bump();
                    while matches!(self.peek(), TokKind::Newline) {
                        self.bump();
                    }
                    if matches!(self.peek(), TokKind::Arrow) {
                        self.bump();
                        out.push(self.parse_flow_step()?);
                        loop {
                            if matches!(self.peek(), TokKind::Arrow) {
                                self.bump();
                                out.push(self.parse_flow_step()?);
                            } else if matches!(self.peek(), TokKind::Newline) {
                                let save3 = self.pos;
                                while matches!(self.peek(), TokKind::Newline) {
                                    self.bump();
                                }
                                if matches!(self.peek(), TokKind::Arrow) {
                                    self.bump();
                                    out.push(self.parse_flow_step()?);
                                } else {
                                    self.pos = save3;
                                    break;
                                }
                            } else {
                                break;
                            }
                        }
                        if matches!(self.peek(), TokKind::Newline) {
                            while matches!(self.peek(), TokKind::Newline) {
                                self.bump();
                            }
                        }
                        if matches!(self.peek(), TokKind::Dedent) {
                            self.bump();
                        }
                    } else {
                        self.pos = save2;
                        self.pos = save;
                        break;
                    }
                } else {
                    self.pos = save;
                    break;
                }
            } else {
                break;
            }
        }
        Ok(out)
    }

    fn parse_flow_step(&mut self) -> Result<FlowStep, ParseError> {
        let sp = self.span();
        if matches!(self.peek(), TokKind::LParen) {
            self.bump();
            let mut nodes = vec![self.parse_ident()?];
            while matches!(self.peek(), TokKind::Comma) {
                self.bump();
                nodes.push(self.parse_ident()?);
            }
            self.expect(&TokKind::RParen, "`)`")?;
            Ok(FlowStep { nodes, span: sp })
        } else {
            let id = self.parse_ident()?;
            Ok(FlowStep::single(id, sp))
        }
    }

    fn parse_cx_spec(&mut self) -> Result<CxSpec, ParseError> {
        match self.peek().clone() {
            TokKind::TypeName(s) if s == "O" => {
                self.bump();
            }
            TokKind::Ident(s) if s == "O" => {
                self.bump();
            }
            _ => return Err(self.err("expected `O(...)` in cx clause".into())),
        }
        self.expect(&TokKind::LParen, "`(`")?;
        let bigo = self.parse_cx_expr()?;
        self.expect(&TokKind::RParen, "`)`")?;
        let class = self.parse_runtime_class();
        Ok(CxSpec { bigo, class })
    }

    fn parse_runtime_class(&mut self) -> RuntimeClass {
        let first = match self.peek().clone() {
            TokKind::Ident(s) => s,
            _ => return RuntimeClass::Idem,
        };
        if !matches!(first.as_str(), "idem" | "anti") {
            return RuntimeClass::Idem;
        }
        self.bump();
        let mut composite = first.clone();
        while matches!(self.peek(), TokKind::Minus) {
            self.bump();
            if let TokKind::Ident(s) = self.peek().clone() {
                composite.push('-');
                composite.push_str(&s);
                self.bump();
            } else {
                break;
            }
        }
        match composite.as_str() {
            "idem" => RuntimeClass::Idem,
            "idem-complex" => RuntimeClass::IdemComplex,
            "anti-idem" => RuntimeClass::AntiIdem,
            _ => RuntimeClass::Idem,
        }
    }

    fn parse_cx_expr(&mut self) -> Result<CxExpr, ParseError> {
        let mut term = self.parse_cx_term()?;
        while matches!(self.peek(), TokKind::Plus) {
            self.bump();
            let r = self.parse_cx_term()?;
            term = match term {
                CxExpr::Sum(mut v) => {
                    v.push(r);
                    CxExpr::Sum(v)
                }
                a => CxExpr::Sum(vec![a, r]),
            };
        }
        Ok(term)
    }

    fn parse_cx_term(&mut self) -> Result<CxExpr, ParseError> {
        let mut factor = self.parse_cx_factor()?;
        while matches!(self.peek(), TokKind::Star) {
            self.bump();
            let r = self.parse_cx_factor()?;
            factor = match factor {
                CxExpr::Prod(mut v) => {
                    v.push(r);
                    CxExpr::Prod(v)
                }
                a => CxExpr::Prod(vec![a, r]),
            };
        }
        Ok(factor)
    }

    fn parse_cx_factor(&mut self) -> Result<CxExpr, ParseError> {
        match self.peek().clone() {
            TokKind::IntLit(1) => {
                self.bump();
                Ok(CxExpr::Const)
            }
            TokKind::Ident(s) if s == "log" => {
                self.bump();
                let v = self.parse_ident()?;
                Ok(CxExpr::LogN(v))
            }
            TokKind::Ident(s) if s == "max" => {
                self.bump();
                self.expect(&TokKind::LParen, "`(`")?;
                let mut xs = vec![self.parse_cx_expr()?];
                while matches!(self.peek(), TokKind::Comma) {
                    self.bump();
                    xs.push(self.parse_cx_expr()?);
                }
                self.expect(&TokKind::RParen, "`)`")?;
                Ok(CxExpr::Max(xs))
            }
            TokKind::Ident(s) => {
                self.bump();
                let var = Ident::new(s);
                if let TokKind::Ident(next) = self.peek().clone() {
                    if next == "log" {
                        self.bump();
                        let v2 = self.parse_ident()?;
                        if v2 == var {
                            return Ok(CxExpr::NLogN(var));
                        }
                        return Ok(CxExpr::Prod(vec![CxExpr::Size(var), CxExpr::LogN(v2)]));
                    }
                }
                Ok(CxExpr::Size(var))
            }
            other => Err(self.err(format!("unexpected token in cx expr: {:?}", other))),
        }
    }

    fn parse_sla_spec(&mut self) -> Result<Sla, ParseError> {
        let mut eps = (0i128, 1u128);
        let mut delta = (0i128, 1u128);
        let mut t_ns: u128 = 0;
        let mut first = true;
        loop {
            if !first {
                if !matches!(self.peek(), TokKind::Comma) {
                    break;
                }
                self.bump();
            }
            first = false;
            let field = match self.peek().clone() {
                TokKind::Ident(s) => {
                    self.bump();
                    s
                }
                TokKind::TypeName(s) => {
                    self.bump();
                    s
                }
                _ => break,
            };
            let _le = matches!(self.peek(), TokKind::Le | TokKind::Eq);
            if matches!(self.peek(), TokKind::Le | TokKind::Eq) {
                self.bump();
            }
            let r = self.parse_rat_or_duration()?;
            match field.as_str() {
                "e" => match r {
                    Bound::Rat(p, q) => eps = (p, q),
                    _ => return Err(self.err("e must be a rational".into())),
                },
                "d" => match r {
                    Bound::Rat(p, q) => delta = (p, q),
                    _ => return Err(self.err("d must be a rational".into())),
                },
                "T" => match r {
                    Bound::Dur(ns) => t_ns = ns,
                    _ => return Err(self.err("T must be a duration".into())),
                },
                other => return Err(self.err(format!("unknown sla field `{}`", other))),
            }
        }
        Ok(Sla {
            epsilon: eps,
            delta,
            t: Duration { ns: t_ns },
        })
    }

    fn parse_via_spec(&mut self) -> Result<Via, ParseError> {
        if matches!(self.peek(), TokKind::At) {
            self.bump();
        }
        let tag = match self.peek().clone() {
            TokKind::Ident(s) => {
                self.bump();
                Ident::new(s)
            }
            other => return Err(self.err(format!("expected `@tag`, got {:?}", other))),
        };
        if !is_defined_via_tag(tag.as_str()) {
            return Err(self.err(format!(
                "via tag `@{}` has no defined semantics in APSL v0.1 — refusing to launder undefined obligation. \
                 Defined tags: @statistical and @external. New tags require explicit parser, type, and TCB semantics.",
                tag
            )));
        }
        let required_attrs: &[&str] = match tag.as_str() {
            "statistical" => &["holdout"],
            "external" => &["service"],
            _ => &[],
        };
        let mut attrs = Vec::new();
        loop {
            if matches!(self.peek(), TokKind::Comma) {
                self.bump();
            }
            match self.peek().clone() {
                TokKind::Ident(s) => {
                    let save = self.pos;
                    self.bump();
                    if matches!(self.peek(), TokKind::Eq) {
                        self.bump();
                        let v = self.parse_via_value()?;
                        attrs.push((Ident::new(s), v));
                    } else {
                        self.pos = save;
                        break;
                    }
                }
                _ => break,
            }
        }
        for required in required_attrs {
            if !attrs.iter().any(|(k, _)| k.as_str() == *required) {
                return Err(self.err(format!(
                    "via @{} requires attribute `{}=...`",
                    tag, required
                )));
            }
        }
        Ok(Via { tag, attrs })
    }

    fn parse_via_value(&mut self) -> Result<Ident, ParseError> {
        match self.peek().clone() {
            TokKind::Ident(s) | TokKind::TypeName(s) => {
                self.bump();
                Ok(Ident::new(s))
            }
            TokKind::IntLit(n) => {
                self.bump();
                Ok(Ident::new(n.to_string()))
            }
            TokKind::StrLit(s) => {
                self.bump();
                Ok(Ident::new(s))
            }
            other => Err(self.err(format!("expected via value, got {:?}", other))),
        }
    }

    fn parse_rat_or_duration(&mut self) -> Result<Bound, ParseError> {
        let (mut p, mut q) = self.parse_number()?;
        while matches!(self.peek(), TokKind::Slash) {
            self.bump();
            let (np, nq) = self.parse_number()?;
            if np == 0 {
                return Err(self.err("division by zero in sla value".into()));
            }
            let new_p = p.saturating_mul(nq as i128);
            let new_q = q.saturating_mul(np.unsigned_abs());
            let (rp, rq) = reduce_rat(if np < 0 { -new_p } else { new_p }, new_q);
            p = rp;
            q = rq;
        }
        if let TokKind::Ident(u) = self.peek().clone() {
            if u == "ms" || u == "s" || u == "us" || u == "ns" {
                self.bump();
                let mult = match u.as_str() {
                    "ns" => 1u128,
                    "us" => 1_000,
                    "ms" => 1_000_000,
                    "s" => 1_000_000_000,
                    _ => unreachable!(),
                };
                if q == 0 {
                    return Err(self.err("invalid rational".into()));
                }
                let ns = if p >= 0 {
                    (p.unsigned_abs() * mult) / q
                } else {
                    0
                };
                return Ok(Bound::Dur(ns));
            }
        }
        Ok(Bound::Rat(p, q))
    }

    fn parse_number(&mut self) -> Result<(i128, u128), ParseError> {
        match self.peek().clone() {
            TokKind::IntLit(n) => {
                self.bump();
                Ok((n, 1u128))
            }
            TokKind::RatLit(p, q) => {
                self.bump();
                Ok((p, q))
            }
            other => Err(self.err(format!("expected number, got {:?}", other))),
        }
    }

    fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        self.parse_implies_expr()
    }

    fn parse_implies_expr(&mut self) -> Result<Expr, ParseError> {
        let left = self.parse_or_expr()?;
        if let TokKind::Ident(s) = self.peek().clone() {
            if s == "implies" {
                let sp = self.span();
                self.bump();
                let r = self.parse_implies_expr()?;
                let not_left = Expr::Un(UnOp::Not, Box::new(left), sp.clone());
                return Ok(Expr::Bin(BinOp::Or, Box::new(not_left), Box::new(r), sp));
            }
        }
        Ok(left)
    }

    fn parse_or_expr(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_and_expr()?;
        loop {
            match self.peek().clone() {
                TokKind::Ident(s) if s == "or" => {
                    let sp = self.span();
                    self.bump();
                    let r = self.parse_and_expr()?;
                    left = Expr::Bin(BinOp::Or, Box::new(left), Box::new(r), sp);
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn parse_and_expr(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_not_expr()?;
        loop {
            match self.peek().clone() {
                TokKind::Ident(s) if s == "and" => {
                    let sp = self.span();
                    self.bump();
                    let r = self.parse_not_expr()?;
                    left = Expr::Bin(BinOp::And, Box::new(left), Box::new(r), sp);
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn parse_not_expr(&mut self) -> Result<Expr, ParseError> {
        if let TokKind::Ident(s) = self.peek().clone() {
            if s == "not" {
                let sp = self.span();
                self.bump();
                let e = self.parse_cmp_expr()?;
                return Ok(Expr::Un(UnOp::Not, Box::new(e), sp));
            }
        }
        self.parse_cmp_expr()
    }

    fn parse_cmp_expr(&mut self) -> Result<Expr, ParseError> {
        let left = self.parse_add_expr()?;
        let op = match self.peek().clone() {
            TokKind::Eq => Some(BinOp::Eq),
            TokKind::Ne => Some(BinOp::Ne),
            TokKind::Lt => Some(BinOp::Lt),
            TokKind::Le => Some(BinOp::Le),
            TokKind::Gt => Some(BinOp::Gt),
            TokKind::Ge => Some(BinOp::Ge),
            TokKind::Ident(s) if s == "subset" => Some(BinOp::Subset),
            TokKind::Ident(s) if s == "union" => Some(BinOp::Union),
            TokKind::Ident(s) if s == "intersect" => Some(BinOp::Intersect),
            _ => None,
        };
        if let Some(op) = op {
            let sp = self.span();
            self.bump();
            let r = self.parse_add_expr()?;
            return Ok(Expr::Bin(op, Box::new(left), Box::new(r), sp));
        }
        Ok(left)
    }

    fn parse_add_expr(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_mul_expr()?;
        loop {
            let op = match self.peek().clone() {
                TokKind::Plus => Some(BinOp::Add),
                TokKind::Minus => Some(BinOp::Sub),
                _ => None,
            };
            if let Some(op) = op {
                let sp = self.span();
                self.bump();
                let r = self.parse_mul_expr()?;
                left = Expr::Bin(op, Box::new(left), Box::new(r), sp);
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_mul_expr(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_unary()?;
        loop {
            let op = match self.peek().clone() {
                TokKind::Star => Some(BinOp::Mul),
                TokKind::Slash => Some(BinOp::Div),
                _ => None,
            };
            if let Some(op) = op {
                let sp = self.span();
                self.bump();
                let r = self.parse_unary()?;
                left = Expr::Bin(op, Box::new(left), Box::new(r), sp);
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, ParseError> {
        if matches!(self.peek(), TokKind::Minus) {
            let sp = self.span();
            self.bump();
            let e = self.parse_postfix()?;
            return Ok(Expr::Un(UnOp::Neg, Box::new(e), sp));
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<Expr, ParseError> {
        let mut e = self.parse_primary()?;
        loop {
            match self.peek().clone() {
                TokKind::Dot if self.allow_dot_postfix => {
                    let sp = self.span();
                    self.bump();
                    let field = match self.peek().clone() {
                        TokKind::Ident(s) => {
                            self.bump();
                            Ident::new(s)
                        }
                        TokKind::IntLit(n) => {
                            self.bump();
                            Ident::new(n.to_string())
                        }
                        other => {
                            return Err(
                                self.err(format!("expected field name after `.`, got {:?}", other))
                            )
                        }
                    };
                    e = Expr::Field(Box::new(e), field, sp);
                }
                _ => break,
            }
        }
        Ok(e)
    }

    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        let sp = self.span();
        match self.peek().clone() {
            TokKind::IntLit(n) => {
                self.bump();
                Ok(Expr::Lit(Lit::Int(n), sp))
            }
            TokKind::RatLit(p, q) => {
                self.bump();
                Ok(Expr::Lit(Lit::Rat(p, q), sp))
            }
            TokKind::BoolLit(b) => {
                self.bump();
                Ok(Expr::Lit(Lit::Bool(b), sp))
            }
            TokKind::StrLit(s) => {
                self.bump();
                Ok(Expr::Lit(Lit::Str(s), sp))
            }
            TokKind::LParen => {
                self.bump();
                let first = self.parse_expr()?;
                if matches!(self.peek(), TokKind::Comma) {
                    let mut xs = vec![first];
                    while matches!(self.peek(), TokKind::Comma) {
                        self.bump();
                        xs.push(self.parse_expr()?);
                    }
                    self.expect(&TokKind::RParen, "`)`")?;
                    Ok(Expr::Tuple(xs, sp))
                } else {
                    self.expect(&TokKind::RParen, "`)`")?;
                    Ok(first)
                }
            }
            TokKind::Ident(s) => {
                if s == "forall" || s == "exists" {
                    let q = if s == "forall" {
                        Quant::Forall
                    } else {
                        Quant::Exists
                    };
                    self.bump();
                    let xname = self.parse_ident()?;
                    match self.peek().clone() {
                        TokKind::Ident(k) if k == "in" => {
                            self.bump();
                        }
                        _ => return Err(self.err("expected `in` in quantifier".into())),
                    }
                    let save_quant = self.pos;
                    let prev = self.allow_dot_postfix;
                    self.allow_dot_postfix = true;
                    let dom_result = self.parse_expr();
                    self.allow_dot_postfix = prev;
                    if let Ok(dom) = dom_result {
                        if matches!(self.peek(), TokKind::Colon) {
                            self.bump();
                            let body = self.parse_expr()?;
                            return Ok(Expr::Quant(q, xname, Box::new(dom), Box::new(body), sp));
                        }
                    }
                    self.pos = save_quant;
                    let prev2 = self.allow_dot_postfix;
                    self.allow_dot_postfix = false;
                    let dom = self.parse_expr()?;
                    self.allow_dot_postfix = prev2;
                    if matches!(self.peek(), TokKind::Dot) {
                        self.bump();
                    } else {
                        return Err(self.err("expected `.` or `:` after quantifier domain".into()));
                    }
                    let body = self.parse_expr()?;
                    return Ok(Expr::Quant(q, xname, Box::new(dom), Box::new(body), sp));
                }
                if s == "if" {
                    self.bump();
                    let c = self.parse_expr()?;
                    match self.peek().clone() {
                        TokKind::Ident(k) if k == "then" => {
                            self.bump();
                        }
                        _ => return Err(self.err("expected `then`".into())),
                    }
                    let a = self.parse_expr()?;
                    match self.peek().clone() {
                        TokKind::Ident(k) if k == "else" => {
                            self.bump();
                        }
                        _ => return Err(self.err("expected `else`".into())),
                    }
                    let b = self.parse_expr()?;
                    return Ok(Expr::If(Box::new(c), Box::new(a), Box::new(b), sp));
                }
                if s == "let" {
                    self.bump();
                    let x = self.parse_ident()?;
                    self.expect(&TokKind::Eq, "`=`")?;
                    let e1 = self.parse_expr()?;
                    match self.peek().clone() {
                        TokKind::Ident(k) if k == "in" => {
                            self.bump();
                        }
                        _ => return Err(self.err("expected `in` in let".into())),
                    }
                    let body = self.parse_expr()?;
                    return Ok(Expr::Let(x, Box::new(e1), Box::new(body), sp));
                }
                self.bump();
                let id = Ident::new(s.clone());
                if matches!(self.peek(), TokKind::LParen) {
                    self.bump();
                    let mut args = Vec::new();
                    if !matches!(self.peek(), TokKind::RParen) {
                        args.push(self.parse_expr()?);
                        while matches!(self.peek(), TokKind::Comma) {
                            self.bump();
                            args.push(self.parse_expr()?);
                        }
                    }
                    self.expect(&TokKind::RParen, "`)`")?;
                    return Ok(Expr::Apply(id, args, sp));
                }
                if is_combinator(&s) {
                    let mut args = Vec::new();
                    let arity = combinator_arity(&s);
                    for _ in 0..arity {
                        if can_start_atom(self.peek()) {
                            args.push(self.parse_atom_for_apply()?);
                        } else {
                            break;
                        }
                    }
                    return Ok(Expr::Apply(id, args, sp));
                }
                Ok(Expr::Var(id, sp))
            }
            TokKind::TypeName(s) => {
                self.bump();
                Ok(Expr::Var(Ident::new(s), sp))
            }
            other => Err(self.err(format!("unexpected token `{:?}` in expression", other))),
        }
    }

    fn parse_atom_for_apply(&mut self) -> Result<Expr, ParseError> {
        let sp = self.span();
        match self.peek().clone() {
            TokKind::Ident(s) => {
                self.bump();
                let id = Ident::new(s);
                Ok(Expr::Var(id, sp))
            }
            TokKind::TypeName(s) => {
                self.bump();
                Ok(Expr::Var(Ident::new(s), sp))
            }
            TokKind::IntLit(n) => {
                self.bump();
                Ok(Expr::Lit(Lit::Int(n), sp))
            }
            TokKind::LParen => {
                self.bump();
                let e = self.parse_expr()?;
                self.expect(&TokKind::RParen, "`)`")?;
                Ok(e)
            }
            other => Err(self.err(format!("expected atom, got {:?}", other))),
        }
    }
}

enum Bound {
    Rat(i128, u128),
    Dur(u128),
}

fn reduce_rat(p: i128, q: u128) -> (i128, u128) {
    if q == 0 {
        return (p, q);
    }
    let g = gcd(p.unsigned_abs(), q);
    if g == 0 {
        (p, q)
    } else {
        (p / g as i128, q / g)
    }
}

fn gcd(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

fn is_defined_via_tag(s: &str) -> bool {
    matches!(s, "statistical" | "external")
}

fn is_combinator(s: &str) -> bool {
    matches!(
        s,
        "every"
            | "some"
            | "count"
            | "map"
            | "filter"
            | "fold"
            | "sort_by"
            | "group_by"
            | "sort"
            | "len"
            | "head"
            | "tail"
            | "nth"
            | "range"
            | "dedupe"
            | "concat"
            | "reverse"
            | "zip"
            | "unique?"
            | "subseteq?"
            | "valid_email?"
            | "well_formed_json?"
            | "not"
    ) || s.ends_with('?')
}

fn combinator_arity(s: &str) -> usize {
    match s {
        "every" | "some" | "count" | "map" | "filter" | "group_by" | "sort_by" | "subseteq?"
        | "nth" | "range" | "concat" | "zip" => 2,
        "fold" => 3,
        _ => 1,
    }
}

fn can_start_atom(t: &TokKind) -> bool {
    matches!(
        t,
        TokKind::Ident(_) | TokKind::TypeName(_) | TokKind::IntLit(_) | TokKind::LParen
    )
}

pub fn parse_tokens(toks: Vec<Tok>) -> Result<Program, ParseError> {
    let mut p = Parser::new(toks);
    p.parse_program()
}
