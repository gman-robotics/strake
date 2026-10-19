//! Strake-A concrete syntax: parse, format, and module-level rejections.

#![allow(clippy::only_used_in_recursion)]

use super::diag::{Code, Diag, Kind, Span};

// --- Public CST ----------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct Module {
    pub span: Span,
    pub version: u32,
    pub items: Vec<Item>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    TypeDef(TypeDef),
    Import(ImportDecl),
    Use(UseDecl),
    Fuel(i128, Span),
    Pages(i128, Span),
    Fn(FnItem),
    Test(TestItem),
    Property(PropertyItem),
}

#[derive(Clone, Debug, PartialEq)]
pub struct TypeDef {
    pub span: Span,
    pub name: String,
    pub kind: TypeDefKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TypeDefKind {
    Record(Vec<FieldDecl>),
    Sum(Vec<SumVariant>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct FieldDecl {
    pub span: Span,
    pub name: String,
    pub ty: Type,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SumVariant {
    pub span: Span,
    pub tag: String,
    pub payload: Option<Type>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ImportDecl {
    pub span: Span,
    pub path: String,
    pub sig: ImportSig,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ImportSig {
    Region,
    Fn { params: Vec<Type>, ret: Type },
}

#[derive(Clone, Debug, PartialEq)]
pub struct UseDecl {
    pub span: Span,
    pub name: String,
    pub hash: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FnItem {
    pub span: Span,
    pub name: String,
    pub params: Vec<Param>,
    pub ret: Type,
    pub body: Vec<Stmt>,
    pub private: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TestItem {
    pub span: Span,
    pub name: String,
    pub body: Vec<Stmt>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PropertyItem {
    pub span: Span,
    pub name: String,
    pub body: Vec<Stmt>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    pub span: Span,
    pub name: String,
    pub ty: Type,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Type {
    Named(String, Span),
    List(Box<Type>, Span),
    Result(Box<Type>, Box<Type>, Span),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Stmt {
    Assign {
        span: Span,
        name: String,
        rhs: Rhs,
    },
    Ret {
        span: Span,
        value: Operand,
    },
    If {
        span: Span,
        cond: Operand,
        then_body: Vec<Stmt>,
        else_body: Vec<Stmt>,
    },
    Loop {
        span: Span,
        params: Vec<LoopParam>,
        body: Vec<Stmt>,
    },
    Break {
        span: Span,
    },
    Continue {
        span: Span,
        edges: Vec<ContinueEdge>,
    },
    For {
        span: Span,
        binder: String,
        list: String,
        body: Vec<Stmt>,
    },
    Match {
        span: Span,
        scrutinee: String,
        arms: Vec<MatchArm>,
    },
    Forall {
        span: Span,
        binder: String,
        bound: ForallBound,
        body: Vec<Stmt>,
    },
    Trap {
        span: Span,
    },
    AssertEq {
        span: Span,
        got: Operand,
        want: Operand,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct LoopParam {
    pub span: Span,
    pub name: String,
    pub init: Operand,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ContinueEdge {
    pub span: Span,
    pub name: String,
    pub value: Operand,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MatchArm {
    pub span: Span,
    pub tag: String,
    pub binder: Option<String>,
    pub body: Vec<Stmt>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ForallBound {
    Range {
        span: Span,
        ty: Type,
        lo: i128,
        hi: i128,
    },
    List {
        span: Span,
        ty: Type,
        name: String,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Rhs {
    BinOp {
        span: Span,
        op: BinOp,
        ty: Type,
        lhs: Operand,
        rhs: Operand,
    },
    Not {
        span: Span,
        operand: Operand,
    },
    Call {
        span: Span,
        callee: String,
        args: Vec<Operand>,
    },
    Ok {
        span: Span,
        ty: Type,
        value: Operand,
    },
    Err {
        span: Span,
        ty: Type,
        value: Operand,
    },
    ListEmpty {
        span: Span,
        elem: Type,
    },
    ListCons {
        span: Span,
        head: Operand,
        tail: Operand,
    },
    ListLen {
        span: Span,
        list: Operand,
    },
    ListGet {
        span: Span,
        list: Operand,
        index: Operand,
    },
    BytesNew {
        span: Span,
        hex: Vec<u8>,
    },
    BytesLen {
        span: Span,
        bytes: Operand,
    },
    BytesGet {
        span: Span,
        bytes: Operand,
        index: Operand,
    },
    Load {
        span: Span,
        ty: Type,
        addr: Operand,
    },
    Store {
        span: Span,
        ty: Type,
        addr: Operand,
        value: Operand,
    },
    Hole {
        span: Span,
        ty: Type,
    },
    Record {
        span: Span,
        fields: Vec<(String, Operand)>,
    },
    Project {
        span: Span,
        base: String,
        field: String,
    },
    Operand(Operand),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Operand {
    Name(String, Span),
    Int(i128, Span),
    Float(f64, Span),
    Bytes(Vec<u8>, Span),
    Unit(Span),
    Bool(bool, Span),
}

// --- Public API ----------------------------------------------------------------

pub fn format(module: &Module) -> String {
    Fmt::new().format_module(module)
}

pub fn codeless_rejection(module: &Module) -> Option<&'static str> {
    let mut fuel = 0u32;
    let mut pages = 0u32;
    for item in &module.items {
        match item {
            Item::Fuel(_, _) => {
                fuel += 1;
                if fuel > 1 {
                    return Some("module may contain at most one fuel directive");
                }
            }
            Item::Pages(_, _) => {
                pages += 1;
                if pages > 1 {
                    return Some("module may contain at most one pages directive");
                }
            }
            _ => {}
        }
    }
    None
}

// --- Parse diagnostics ---------------------------------------------------------

fn e_parse(file: &str, start: u32, end: u32) -> Diag {
    Diag {
        code: Code::EParse,
        kind: Kind::Check,
        span: Span {
            file: file.to_string(),
            start,
            end,
        },
        want: None,
        got: None,
        patch: None,
    }
}

// --- Lexer ---------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Ident(String),
    Int(i128),
    Float(f64),
    Bytes(Vec<u8>),
    LParen,
    RParen,
    LBrace,
    RBrace,
    Comma,
    Colon,
    Dot,
    Eq,
    Arrow,
    Pipe,
    At,
    DotDot,
    Hash,
}

struct Lexer<'a> {
    src: &'a str,
    file: &'a str,
    i: usize,
}

impl<'a> Lexer<'a> {
    fn new(src: &'a str, file: &'a str) -> Self {
        Self { src, file, i: 0 }
    }

    fn byte_pos(&self) -> u32 {
        self.i as u32
    }

    fn set_byte_pos(&mut self, pos: u32) {
        self.i = pos as usize;
    }

    fn peek_byte(&self) -> Option<u8> {
        self.src[self.i..].bytes().next()
    }

    fn bump(&mut self) -> Option<u8> {
        let b = self.peek_byte()?;
        self.i += 1;
        Some(b)
    }

    fn skip_ws(&mut self) {
        while let Some(b) = self.peek_byte() {
            if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' {
                self.bump();
            } else if b == b'#' {
                while let Some(c) = self.bump() {
                    if c == b'\n' {
                        break;
                    }
                }
            } else {
                break;
            }
        }
    }

    fn err(&self, start: u32) -> Diag {
        e_parse(self.file, start, self.byte_pos())
    }

    fn next(&mut self) -> Result<Option<(Tok, u32, u32)>, Diag> {
        self.skip_ws();
        let start = self.byte_pos();
        let b = self.peek_byte();
        if b.is_none() {
            return Ok(None);
        }
        let tok = match b.unwrap() {
            b'(' => {
                self.bump();
                Tok::LParen
            }
            b')' => {
                self.bump();
                Tok::RParen
            }
            b'{' => {
                self.bump();
                Tok::LBrace
            }
            b'}' => {
                self.bump();
                Tok::RBrace
            }
            b',' => {
                self.bump();
                Tok::Comma
            }
            b':' => {
                self.bump();
                Tok::Colon
            }
            b'=' => {
                self.bump();
                Tok::Eq
            }
            b'|' => {
                self.bump();
                Tok::Pipe
            }
            b'@' => {
                self.bump();
                Tok::At
            }
            b'#' => {
                self.bump();
                Tok::Hash
            }
            b'.' => {
                self.bump();
                if self.peek_byte() == Some(b'.') {
                    self.bump();
                    Tok::DotDot
                } else {
                    Tok::Dot
                }
            }
            b'-' => {
                if self.lookahead_number_after_sign() {
                    self.lex_number(true)?
                } else if self.src[self.i + 1..].bytes().next() == Some(b'>') {
                    self.bump();
                    self.bump();
                    Tok::Arrow
                } else {
                    return Err(self.err(start));
                }
            }
            b'0'..=b'9' => self.lex_number(false)?,
            b'x' => {
                if self.src[self.i + 1..].starts_with('"') {
                    self.lex_bytes_literal()?
                } else {
                    self.lex_ident_or_kw()?
                }
            }
            b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'%' => self.lex_ident_or_kw()?,
            _ => return Err(self.err(start)),
        };
        let end = self.byte_pos();
        Ok(Some((tok, start, end)))
    }

    fn lookahead_number_after_sign(&mut self) -> bool {
        let rest = &self.src[self.i + 1..];
        rest.starts_with(|c: char| c.is_ascii_digit())
    }

    fn lex_number(&mut self, neg: bool) -> Result<Tok, Diag> {
        let start = self.byte_pos();
        if neg {
            self.bump();
        }
        let num_start = self.i;
        while matches!(self.peek_byte(), Some(b'0'..=b'9')) {
            self.bump();
        }
        if matches!(self.peek_byte(), Some(b'e' | b'E')) {
            return Err(e_parse(self.file, start, self.byte_pos()));
        }
        let slice = &self.src[num_start..self.i];
        if self.peek_byte() == Some(b'.') {
            let dot = self.i;
            self.bump();
            if matches!(self.peek_byte(), Some(b'0'..=b'9')) {
                while matches!(self.peek_byte(), Some(b'0'..=b'9')) {
                    self.bump();
                }
                if matches!(self.peek_byte(), Some(b'e' | b'E')) {
                    return Err(e_parse(self.file, start, self.byte_pos()));
                }
                let fslice = &self.src[num_start..self.i];
                let mut v = fslice
                    .parse::<f64>()
                    .map_err(|_| e_parse(self.file, start, self.byte_pos()))?;
                if neg {
                    v = -v;
                }
                return Ok(Tok::Float(v));
            }
            self.i = dot;
        }
        let mut text = slice.to_string();
        if neg {
            text.insert(0, '-');
        }
        let v: i128 = text
            .parse()
            .map_err(|_| e_parse(self.file, start, self.byte_pos()))?;
        Ok(Tok::Int(v))
    }

    fn lex_bytes_literal(&mut self) -> Result<Tok, Diag> {
        let start = self.byte_pos();
        self.bump(); // x
        if self.bump() != Some(b'"') {
            return Err(self.err(start));
        }
        let hex_start = self.i;
        while let Some(b) = self.peek_byte() {
            if b == b'"' {
                break;
            }
            if !(b.is_ascii_hexdigit()) {
                return Err(self.err(start));
            }
            self.bump();
        }
        if self.bump() != Some(b'"') {
            return Err(self.err(start));
        }
        let hex_str = &self.src[hex_start..self.i - 1];
        if hex_str.len() % 2 != 0 {
            return Err(self.err(start));
        }
        let mut out = Vec::new();
        let mut i = 0;
        while i < hex_str.len() {
            let pair = &hex_str[i..i + 2];
            let byte = u8::from_str_radix(pair, 16).map_err(|_| self.err(start))?;
            out.push(byte);
            i += 2;
        }
        Ok(Tok::Bytes(out))
    }

    fn lex_ident_or_kw(&mut self) -> Result<Tok, Diag> {
        let start = self.byte_pos();
        let mut s = String::new();
        while let Some(b) = self.peek_byte() {
            if b.is_ascii_alphanumeric() || b == b'_' || b == b'.' || b == b'%' {
                s.push(b as char);
                self.bump();
            } else {
                break;
            }
        }
        if s.is_empty() {
            return Err(self.err(start));
        }
        Ok(Tok::Ident(s))
    }
}

// --- Parser --------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum BodyCtx {
    Fn,
    Test,
    Property,
}

struct Parser<'a> {
    lex: Lexer<'a>,
    file: &'a str,
    cur: Option<(Tok, u32, u32)>,
}

impl<'a> Parser<'a> {
    fn init(src: &'a str, file: &'a str) -> Result<Self, Diag> {
        let mut lex = Lexer::new(src, file);
        let cur = lex.next()?;
        Ok(Self { lex, file, cur })
    }

    fn bump(&mut self) -> Result<(), Diag> {
        self.cur = self.lex.next()?;
        Ok(())
    }

    fn span_of(&self) -> (u32, u32) {
        match &self.cur {
            Some((_, s, e)) => (*s, *e),
            None => (self.lex.byte_pos(), self.lex.byte_pos()),
        }
    }

    fn err_here(&self) -> Diag {
        let (s, e) = self.span_of();
        e_parse(self.file, s, e)
    }

    fn err_range(&self, start: u32, end: u32) -> Diag {
        e_parse(self.file, start, end)
    }

    fn expect_ident(&mut self) -> Result<(String, u32, u32), Diag> {
        match self.cur.take() {
            Some((Tok::Ident(s), st, en)) => {
                self.bump()?;
                Ok((s, st, en))
            }
            _ => Err(self.err_here()),
        }
    }

    fn expect_blake3_hash(&mut self) -> Result<(String, u32, u32), Diag> {
        let st = match &self.cur {
            Some((_, s, _)) => *s,
            None => self.lex.byte_pos(),
        };
        self.lex.set_byte_pos(st);
        self.cur = None;
        let mut hex = String::new();
        while let Some(b) = self.lex.peek_byte() {
            if b.is_ascii_hexdigit() {
                hex.push(b as char);
                self.lex.bump();
            } else {
                break;
            }
        }
        if hex.is_empty() {
            return Err(self.err_here());
        }
        let en = self.lex.byte_pos();
        self.cur = self.lex.next()?;
        Ok((hex, st, en))
    }

    fn peek_token_at(&self, byte_off: u32) -> Result<Option<(Tok, u32, u32)>, Diag> {
        let mut lx = Lexer {
            src: self.lex.src,
            file: self.file,
            i: byte_off as usize,
        };
        lx.next()
    }

    fn looks_like_match_arm_start(&self) -> bool {
        let Some((Tok::Ident(tag), _, en)) = self.cur.clone() else {
            return false;
        };
        if tag == "end" {
            return true;
        }
        match self.peek_token_at(en) {
            Ok(Some((Tok::Arrow, _, _))) => true,
            Ok(Some((Tok::Ident(_binder), _, en2))) => {
                matches!(self.peek_token_at(en2), Ok(Some((Tok::Arrow, _, _))))
            }
            _ => false,
        }
    }

    fn check_match_arm_tag(tag: &str, span: (u32, u32), file: &str) -> Result<(), Diag> {
        if tag == "Ok" || tag == "Err" {
            return Err(e_parse(file, span.0, span.1));
        }
        Ok(())
    }

    fn parse_match_arm_body(&mut self, ctx: BodyCtx) -> Result<Vec<Stmt>, Diag> {
        let mut stmts = Vec::new();
        while self.cur.is_some() {
            if self.cur_ident() == Some("end") {
                break;
            }
            if self.looks_like_match_arm_start() {
                break;
            }
            stmts.push(self.parse_stmt(ctx)?);
        }
        Ok(stmts)
    }

    fn expect_int(&mut self) -> Result<(i128, u32, u32), Diag> {
        match self.cur.take() {
            Some((Tok::Int(v), st, en)) => {
                self.bump()?;
                Ok((v, st, en))
            }
            _ => Err(self.err_here()),
        }
    }

    fn eat_ident(&mut self, want: &str) -> Result<bool, Diag> {
        if let Some((Tok::Ident(s), _, _)) = &self.cur {
            if s == want {
                self.bump()?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn cur_ident(&self) -> Option<&str> {
        match &self.cur {
            Some((Tok::Ident(s), _, _)) => Some(s.as_str()),
            _ => None,
        }
    }

    fn banned_op(name: &str) -> bool {
        matches!(
            name,
            "and" | "or" | "rem" | "mod" | "xor" | "shl" | "shr" | "as"
        )
    }

    fn check_user_name(name: &str, span: (u32, u32), file: &str) -> Result<(), Diag> {
        if name.starts_with('%') {
            return Err(e_parse(file, span.0, span.1));
        }
        Ok(())
    }

    /// After `name =`, distinguish `continue i = 0` / `loop i = 0` from `n = add i64 ...`.
    fn looks_like_stmt_rhs_after_eq(&self) -> bool {
        if matches!(&self.cur, Some((Tok::LBrace, _, _))) {
            return true;
        }
        let Some((Tok::Ident(head), _, en)) = self.cur.clone() else {
            return false;
        };
        if split_dotted_project(&head).is_some() {
            return true;
        }
        if matches!(self.peek_token_at(en), Ok(Some((Tok::Dot, _, _)))) {
            return true;
        }
        if binop_from_ident(&head).is_some() {
            return matches!(
                self.peek_token_at(en),
                Ok(Some((Tok::Ident(ty), _, _))) if is_type_name(&ty)
            );
        }
        if matches!(head.as_str(), "ok" | "err" | "hole") {
            return matches!(
                self.peek_token_at(en),
                Ok(Some((Tok::Ident(ty), _, _))) if ident_starts_type(&ty)
            );
        }
        matches!(
            head.as_str(),
            "call"
                | "list.empty"
                | "list.cons"
                | "list.len"
                | "list.get"
                | "bytes.new"
                | "bytes.len"
                | "bytes.get"
                | "load"
                | "store"
                | "not"
        )
    }

    fn parse_module(&mut self) -> Result<Module, Diag> {
        let start = self.span_of().0;
        if !self.eat_ident("strake")? {
            return Err(self.err_here());
        }
        let (ver, _, _) = self.expect_int()?;
        if ver < 0 {
            return Err(self.err_here());
        }
        let version = ver as u32;
        let mut items = Vec::new();
        while self.cur.is_some() {
            items.push(self.parse_item()?);
        }
        let end = self.lex.byte_pos();
        Ok(Module {
            span: Span {
                file: self.file.to_string(),
                start,
                end,
            },
            version,
            items,
        })
    }

    fn parse_item(&mut self) -> Result<Item, Diag> {
        let (s, _) = self.span_of();
        if self.eat_ident("type")? {
            return Ok(Item::TypeDef(self.parse_type_def(s)?));
        }
        if self.eat_ident("import")? {
            return Ok(Item::Import(self.parse_import(s)?));
        }
        if self.eat_ident("use")? {
            return Ok(Item::Use(self.parse_use(s)?));
        }
        if self.eat_ident("fuel")? {
            let (n, st, en) = self.expect_int()?;
            return Ok(Item::Fuel(
                n,
                Span {
                    file: self.file.to_string(),
                    start: st,
                    end: en,
                },
            ));
        }
        if self.eat_ident("pages")? {
            let (n, st, en) = self.expect_int()?;
            return Ok(Item::Pages(
                n,
                Span {
                    file: self.file.to_string(),
                    start: st,
                    end: en,
                },
            ));
        }
        let private = self.eat_ident("priv")?;
        if self.eat_ident("fn")? {
            return Ok(Item::Fn(self.parse_fn(s, private)?));
        }
        if self.eat_ident("test")? {
            return Ok(Item::Test(self.parse_test(s)?));
        }
        if self.eat_ident("property")? {
            return Ok(Item::Property(self.parse_property(s)?));
        }
        Err(self.err_here())
    }

    fn parse_type_def(&mut self, start: u32) -> Result<TypeDef, Diag> {
        let (name, nst, nen) = self.expect_ident()?;
        Self::check_user_name(&name, (nst, nen), self.file)?;
        if !self.eat_tok(Tok::LBrace)? {
            return Err(self.err_here());
        }
        let (first, fst, fen) = self.expect_ident()?;
        Self::check_user_name(&first, (fst, fen), self.file)?;
        let kind = if self.eat_tok(Tok::Colon)? {
            let ty = self.parse_type()?;
            let mut fields = vec![FieldDecl {
                span: span(self.file, fst, fen),
                name: first,
                ty,
            }];
            while self.eat_tok(Tok::Comma)? {
                let (fname, f0, f1) = self.expect_ident()?;
                Self::check_user_name(&fname, (f0, f1), self.file)?;
                if !self.eat_tok(Tok::Colon)? {
                    return Err(self.err_here());
                }
                fields.push(FieldDecl {
                    span: span(self.file, f0, f1),
                    name: fname,
                    ty: self.parse_type()?,
                });
            }
            if !self.eat_tok(Tok::RBrace)? {
                return Err(self.err_here());
            }
            TypeDefKind::Record(fields)
        } else {
            let mut sum = vec![self.finish_sum_variant(first, fst, fen)?];
            while self.eat_tok(Tok::Pipe)? {
                let (tag, t0, t1) = self.expect_ident()?;
                sum.push(self.finish_sum_variant(tag, t0, t1)?);
            }
            if !self.eat_tok(Tok::RBrace)? {
                return Err(self.err_here());
            }
            TypeDefKind::Sum(sum)
        };
        let end = self.lex.byte_pos();
        Ok(TypeDef {
            span: span(self.file, start, end),
            name,
            kind,
        })
    }

    fn finish_sum_variant(&mut self, tag: String, st: u32, en: u32) -> Result<SumVariant, Diag> {
        Self::check_user_name(&tag, (st, en), self.file)?;
        let payload = match &self.cur {
            Some((Tok::Ident(s), _, _)) if s != "}" && s != "|" => Some(self.parse_type()?),
            _ => None,
        };
        Ok(SumVariant {
            span: span(self.file, st, en),
            tag,
            payload,
        })
    }

    fn eat_tok(&mut self, want: Tok) -> Result<bool, Diag> {
        if self.cur.as_ref().map(|(t, _, _)| t) == Some(&want) {
            self.bump()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn parse_import(&mut self, start: u32) -> Result<ImportDecl, Diag> {
        let (path, _, _) = self.expect_ident()?;
        let sig = if path == "region" {
            let (mem, _, _) = self.expect_ident()?;
            if mem != "mem" {
                return Err(self.err_here());
            }
            if !self.eat_tok(Tok::Colon)? {
                return Err(self.err_here());
            }
            let ty = self.parse_type()?;
            if !type_is_region(&ty) {
                return Err(self.err_here());
            }
            ImportSig::Region
        } else {
            if !self.eat_tok(Tok::Colon)? {
                return Err(self.err_here());
            }
            if !self.eat_tok(Tok::LParen)? {
                return Err(self.err_here());
            }
            let mut params = Vec::new();
            if !matches!(&self.cur, Some((Tok::RParen, _, _))) {
                loop {
                    params.push(self.parse_type()?);
                    if self.eat_tok(Tok::Comma)? {
                        continue;
                    }
                    break;
                }
            }
            if !self.eat_tok(Tok::RParen)? {
                return Err(self.err_here());
            }
            if !self.eat_tok(Tok::Arrow)? {
                return Err(self.err_here());
            }
            let ret = self.parse_type()?;
            ImportSig::Fn { params, ret }
        };
        let end = self.lex.byte_pos();
        Ok(ImportDecl {
            span: span(self.file, start, end),
            path,
            sig,
        })
    }

    fn parse_use(&mut self, start: u32) -> Result<UseDecl, Diag> {
        let (name, _, _) = self.expect_ident()?;
        if !self.eat_tok(Tok::At)? {
            return Err(self.err_here());
        }
        let (scheme, _, _) = self.expect_ident()?;
        if scheme != "blake3" {
            return Err(self.err_here());
        }
        match self.cur.take() {
            Some((Tok::Colon, _, en)) => {
                self.lex.set_byte_pos(en);
                self.cur = None;
            }
            _ => return Err(self.err_here()),
        }
        let (hash, _, hen) = self.expect_blake3_hash()?;
        Ok(UseDecl {
            span: span(self.file, start, hen),
            name,
            hash,
        })
    }

    fn parse_fn(&mut self, start: u32, private: bool) -> Result<FnItem, Diag> {
        let (name, nst, nen) = self.expect_ident()?;
        Self::check_user_name(&name, (nst, nen), self.file)?;
        if !self.eat_tok(Tok::LParen)? {
            return Err(self.err_here());
        }
        let mut params = Vec::new();
        if !matches!(&self.cur, Some((Tok::RParen, _, _))) {
            loop {
                let (pname, pst, pen) = self.expect_ident()?;
                Self::check_user_name(&pname, (pst, pen), self.file)?;
                if !self.eat_tok(Tok::Colon)? {
                    return Err(self.err_here());
                }
                let ty = self.parse_type()?;
                params.push(Param {
                    span: span(self.file, pst, pen),
                    name: pname,
                    ty,
                });
                if self.eat_tok(Tok::Comma)? {
                    continue;
                }
                break;
            }
        }
        if !self.eat_tok(Tok::RParen)? {
            return Err(self.err_here());
        }
        if !self.eat_tok(Tok::Arrow)? {
            return Err(self.err_here());
        }
        let ret = self.parse_type()?;
        let body = self.parse_block(BodyCtx::Fn)?;
        let end = self.lex.byte_pos();
        Ok(FnItem {
            span: span(self.file, start, end),
            name,
            params,
            ret,
            body,
            private,
        })
    }

    fn parse_test(&mut self, start: u32) -> Result<TestItem, Diag> {
        let (name, nst, nen) = self.expect_ident()?;
        Self::check_user_name(&name, (nst, nen), self.file)?;
        let body = self.parse_block(BodyCtx::Test)?;
        let end = self.lex.byte_pos();
        Ok(TestItem {
            span: span(self.file, start, end),
            name,
            body,
        })
    }

    fn parse_property(&mut self, start: u32) -> Result<PropertyItem, Diag> {
        let (name, nst, nen) = self.expect_ident()?;
        Self::check_user_name(&name, (nst, nen), self.file)?;
        let body = self.parse_block(BodyCtx::Property)?;
        let end = self.lex.byte_pos();
        Ok(PropertyItem {
            span: span(self.file, start, end),
            name,
            body,
        })
    }

    fn parse_block(&mut self, ctx: BodyCtx) -> Result<Vec<Stmt>, Diag> {
        let stmts = self.parse_stmts_until(ctx, &["end"])?;
        if !self.eat_ident("end")? {
            return Err(self.err_here());
        }
        Ok(stmts)
    }

    fn parse_stmts_until(&mut self, ctx: BodyCtx, stop: &[&str]) -> Result<Vec<Stmt>, Diag> {
        let mut stmts = Vec::new();
        while self.cur.is_some() {
            if let Some(id) = self.cur_ident() {
                if stop.contains(&id) {
                    break;
                }
            }
            stmts.push(self.parse_stmt(ctx)?);
        }
        Ok(stmts)
    }

    fn parse_stmt(&mut self, ctx: BodyCtx) -> Result<Stmt, Diag> {
        let (st, _) = self.span_of();
        if self.eat_ident("if")? {
            return self.parse_if(st, ctx);
        }
        if self.eat_ident("loop")? {
            return self.parse_loop(st, ctx);
        }
        if self.eat_ident("break")? {
            let (s, e) = (st, self.span_of().1);
            return Ok(Stmt::Break {
                span: span(self.file, s, e),
            });
        }
        if self.eat_ident("continue")? {
            return self.parse_continue(st);
        }
        if self.eat_ident("for")? {
            return self.parse_for(st, ctx);
        }
        if self.eat_ident("match")? {
            return self.parse_match(st, ctx);
        }
        if self.eat_ident("forall")? {
            if ctx != BodyCtx::Property {
                return Err(self.err_range(st, self.span_of().1));
            }
            return self.parse_forall(st, ctx);
        }
        if self.eat_ident("trap")? {
            let (s, e) = (st, self.span_of().1);
            return Ok(Stmt::Trap {
                span: span(self.file, s, e),
            });
        }
        if self.cur_ident() == Some("assert.eq") {
            self.bump()?;
            if ctx != BodyCtx::Test && ctx != BodyCtx::Property {
                return Err(self.err_range(st, self.span_of().1));
            }
            let got = self.parse_operand()?;
            let want = self.parse_operand()?;
            let end = self.lex.byte_pos();
            return Ok(Stmt::AssertEq {
                span: span(self.file, st, end),
                got,
                want,
            });
        }
        if self.eat_ident("ret")? {
            let value = self.parse_operand()?;
            let end = self.lex.byte_pos();
            return Ok(Stmt::Ret {
                span: span(self.file, st, end),
                value,
            });
        }
        if self.cur_ident() == Some("call") {
            self.bump()?;
            let _rhs = self.parse_call_rhs(st)?;
            return Err(self.err_here()); // bare call stmt not allowed — must be assign
        }
        let (name, nst, nen) = self.expect_ident()?;
        Self::check_user_name(&name, (nst, nen), self.file)?;
        if !self.eat_tok(Tok::Eq)? {
            return Err(self.err_here());
        }
        let rhs = self.parse_rhs(st)?;
        Ok(Stmt::Assign {
            span: span(self.file, st, self.lex.byte_pos()),
            name,
            rhs,
        })
    }

    fn parse_if(&mut self, start: u32, ctx: BodyCtx) -> Result<Stmt, Diag> {
        let cond = self.parse_operand()?;
        let then_body = self.parse_stmts_until(ctx, &["else", "end"])?;
        let else_body = if self.eat_ident("else")? {
            self.parse_stmts_until(ctx, &["end"])?
        } else {
            Vec::new()
        };
        if !self.eat_ident("end")? {
            return Err(self.err_here());
        }
        let end = self.lex.byte_pos();
        Ok(Stmt::If {
            span: span(self.file, start, end),
            cond,
            then_body,
            else_body,
        })
    }

    fn parse_loop(&mut self, start: u32, ctx: BodyCtx) -> Result<Stmt, Diag> {
        let mut params = Vec::new();
        while self.cur_ident().is_some() && self.cur_ident() != Some("end") {
            let Some((Tok::Ident(name), pst, pen)) = self.cur.clone() else {
                break;
            };
            if matches!(
                name.as_str(),
                "end" | "break" | "continue" | "if" | "for" | "match" | "ret" | "trap" | "forall"
            ) {
                break;
            }
            self.bump()?;
            if !self.eat_tok(Tok::Eq)? {
                self.lex.set_byte_pos(pst);
                self.cur = None;
                self.bump()?;
                break;
            }
            if self.looks_like_stmt_rhs_after_eq() {
                self.lex.set_byte_pos(pst);
                self.cur = None;
                self.bump()?;
                break;
            }
            Self::check_user_name(&name, (pst, pen), self.file)?;
            let init = self.parse_operand()?;
            params.push(LoopParam {
                span: span(self.file, pst, pen),
                name,
                init,
            });
        }
        let body = self.parse_block(ctx)?;
        let end = self.lex.byte_pos();
        Ok(Stmt::Loop {
            span: span(self.file, start, end),
            params,
            body,
        })
    }

    fn parse_continue(&mut self, start: u32) -> Result<Stmt, Diag> {
        let mut edges = Vec::new();
        while self.cur_ident().is_some()
            && self.cur_ident() != Some("end")
            && self.cur_ident() != Some("else")
        {
            if matches!(
                self.cur_ident(),
                Some("break" | "continue" | "if" | "ret" | "for" | "match" | "trap" | "forall")
            ) {
                break;
            }
            let (name, pst, pen) = self.expect_ident()?;
            Self::check_user_name(&name, (pst, pen), self.file)?;
            if !self.eat_tok(Tok::Eq)? {
                self.lex.set_byte_pos(pst);
                self.cur = None;
                self.bump()?;
                break;
            }
            if self.looks_like_stmt_rhs_after_eq() {
                self.lex.set_byte_pos(pst);
                self.cur = None;
                self.bump()?;
                break;
            }
            let value = self.parse_operand()?;
            edges.push(ContinueEdge {
                span: span(self.file, pst, pen),
                name,
                value,
            });
        }
        let end = self.lex.byte_pos();
        Ok(Stmt::Continue {
            span: span(self.file, start, end),
            edges,
        })
    }

    fn parse_for(&mut self, start: u32, ctx: BodyCtx) -> Result<Stmt, Diag> {
        let (binder, bst, ben) = self.expect_ident()?;
        Self::check_user_name(&binder, (bst, ben), self.file)?;
        if !self.eat_ident("in")? {
            return Err(self.err_here());
        }
        let (list, lst, len) = self.expect_ident()?;
        Self::check_user_name(&list, (lst, len), self.file)?;
        let body = self.parse_block(ctx)?;
        let end = self.lex.byte_pos();
        Ok(Stmt::For {
            span: span(self.file, start, end),
            binder,
            list,
            body,
        })
    }

    fn parse_match(&mut self, start: u32, ctx: BodyCtx) -> Result<Stmt, Diag> {
        let (scrutinee, sst, sen) = self.expect_ident()?;
        Self::check_user_name(&scrutinee, (sst, sen), self.file)?;
        let mut arms = Vec::new();
        while self.cur.is_some() && self.cur_ident() != Some("end") {
            let (tag, tst, ten) = self.expect_ident()?;
            Self::check_user_name(&tag, (tst, ten), self.file)?;
            Self::check_match_arm_tag(&tag, (tst, ten), self.file)?;
            let binder = if matches!(&self.cur, Some((Tok::Ident(_), _, _))) {
                let (b, bst, ben) = self.expect_ident()?;
                if b != "_" {
                    Self::check_user_name(&b, (bst, ben), self.file)?;
                }
                Some(b)
            } else {
                None
            };
            if !self.eat_tok(Tok::Arrow)? {
                return Err(self.err_here());
            }
            let body = self.parse_match_arm_body(ctx)?;
            arms.push(MatchArm {
                span: span(self.file, tst, ten),
                tag,
                binder,
                body,
            });
        }
        if !self.eat_ident("end")? {
            return Err(self.err_here());
        }
        let end = self.lex.byte_pos();
        Ok(Stmt::Match {
            span: span(self.file, start, end),
            scrutinee,
            arms,
        })
    }

    fn parse_forall(&mut self, start: u32, ctx: BodyCtx) -> Result<Stmt, Diag> {
        let (binder, bst, ben) = self.expect_ident()?;
        Self::check_user_name(&binder, (bst, ben), self.file)?;
        if !self.eat_tok(Tok::Colon)? {
            return Err(self.err_here());
        }
        let ty = self.parse_type()?;
        if !self.eat_ident("in")? {
            return Err(self.err_here());
        }
        let bound = if matches!(&self.cur, Some((Tok::Int(_), _, _))) {
            let (lo, lst, _len) = self.expect_int()?;
            if !self.eat_tok(Tok::DotDot)? {
                return Err(self.err_here());
            }
            let (hi, _hst, hen) = self.expect_int()?;
            ForallBound::Range {
                span: span(self.file, lst, hen),
                ty,
                lo,
                hi,
            }
        } else {
            let (name, nst, nen) = self.expect_ident()?;
            Self::check_user_name(&name, (nst, nen), self.file)?;
            ForallBound::List {
                span: span(self.file, nst, nen),
                ty,
                name,
            }
        };
        let body = self.parse_block(ctx)?;
        let end = self.lex.byte_pos();
        Ok(Stmt::Forall {
            span: span(self.file, start, end),
            binder,
            bound,
            body,
        })
    }

    fn parse_rhs(&mut self, assign_start: u32) -> Result<Rhs, Diag> {
        let (st, _) = self.span_of();
        if matches!(
            &self.cur,
            Some((Tok::Int(_), _, _)) | Some((Tok::Float(_), _, _)) | Some((Tok::Bytes(_), _, _))
        ) || self.cur_ident() == Some("unit")
            || self.cur_ident() == Some("true")
            || self.cur_ident() == Some("false")
        {
            let op = self.parse_operand()?;
            return Ok(Rhs::Operand(op));
        }
        if self.eat_ident("not")? {
            let operand = self.parse_operand()?;
            return Ok(Rhs::Not {
                span: span(self.file, st, self.lex.byte_pos()),
                operand,
            });
        }
        if self.eat_ident("call")? {
            return self.parse_call_rhs(assign_start);
        }
        if self.eat_ident("ok")? {
            let ty = self.parse_type()?;
            let value = self.parse_operand()?;
            return Ok(Rhs::Ok {
                span: span(self.file, st, self.lex.byte_pos()),
                ty,
                value,
            });
        }
        if self.eat_ident("err")? {
            let ty = self.parse_type()?;
            let value = self.parse_operand()?;
            return Ok(Rhs::Err {
                span: span(self.file, st, self.lex.byte_pos()),
                ty,
                value,
            });
        }
        if self.eat_ident("hole")? {
            let ty = self.parse_type()?;
            return Ok(Rhs::Hole {
                span: span(self.file, st, self.lex.byte_pos()),
                ty,
            });
        }
        if self.cur_ident() == Some("list.empty") {
            self.bump()?;
            let elem = self.parse_type()?;
            return Ok(Rhs::ListEmpty {
                span: span(self.file, st, self.lex.byte_pos()),
                elem,
            });
        }
        if self.cur_ident() == Some("list.cons") {
            self.bump()?;
            let head = self.parse_operand()?;
            let tail = self.parse_operand()?;
            return Ok(Rhs::ListCons {
                span: span(self.file, st, self.lex.byte_pos()),
                head,
                tail,
            });
        }
        if self.cur_ident() == Some("list.len") {
            self.bump()?;
            let list = self.parse_operand()?;
            return Ok(Rhs::ListLen {
                span: span(self.file, st, self.lex.byte_pos()),
                list,
            });
        }
        if self.cur_ident() == Some("list.get") {
            self.bump()?;
            let list = self.parse_operand()?;
            let index = self.parse_operand()?;
            return Ok(Rhs::ListGet {
                span: span(self.file, st, self.lex.byte_pos()),
                list,
                index,
            });
        }
        if self.cur_ident() == Some("bytes.new") {
            self.bump()?;
            let hex = self.parse_bytes_operand()?;
            return Ok(Rhs::BytesNew {
                span: span(self.file, st, self.lex.byte_pos()),
                hex,
            });
        }
        if self.cur_ident() == Some("bytes.len") {
            self.bump()?;
            let bytes = self.parse_operand()?;
            return Ok(Rhs::BytesLen {
                span: span(self.file, st, self.lex.byte_pos()),
                bytes,
            });
        }
        if self.cur_ident() == Some("bytes.get") {
            self.bump()?;
            let bytes = self.parse_operand()?;
            let index = self.parse_operand()?;
            return Ok(Rhs::BytesGet {
                span: span(self.file, st, self.lex.byte_pos()),
                bytes,
                index,
            });
        }
        if self.eat_ident("load")? {
            let ty = self.parse_type()?;
            let addr = self.parse_operand()?;
            return Ok(Rhs::Load {
                span: span(self.file, st, self.lex.byte_pos()),
                ty,
                addr,
            });
        }
        if self.eat_ident("store")? {
            let ty = self.parse_type()?;
            let addr = self.parse_operand()?;
            let value = self.parse_operand()?;
            return Ok(Rhs::Store {
                span: span(self.file, st, self.lex.byte_pos()),
                ty,
                addr,
                value,
            });
        }
        if self.eat_tok(Tok::LBrace)? {
            let mut fields = Vec::new();
            loop {
                let (fname, fst, fen) = self.expect_ident()?;
                Self::check_user_name(&fname, (fst, fen), self.file)?;
                if !self.eat_tok(Tok::Colon)? {
                    return Err(self.err_here());
                }
                let val = self.parse_operand()?;
                fields.push((fname, val));
                if self.eat_tok(Tok::Comma)? {
                    continue;
                }
                break;
            }
            if !self.eat_tok(Tok::RBrace)? {
                return Err(self.err_here());
            }
            return Ok(Rhs::Record {
                span: span(self.file, st, self.lex.byte_pos()),
                fields,
            });
        }
        let (head, hst, hen) = self.expect_ident()?;
        if Self::banned_op(&head) {
            return Err(self.err_range(hst, hen));
        }
        if let Some((base, field)) = split_dotted_project(&head) {
            let fstart = hen - field.len() as u32;
            let bstart = hst;
            let bend = fstart;
            Self::check_user_name(&base, (bstart, bend), self.file)?;
            Self::check_user_name(&field, (fstart, hen), self.file)?;
            return Ok(Rhs::Project {
                span: span(self.file, hst, hen),
                base,
                field,
            });
        }
        if self.eat_tok(Tok::Dot)? {
            Self::check_user_name(&head, (hst, hen), self.file)?;
            let (field, fst, fen) = self.expect_ident()?;
            Self::check_user_name(&field, (fst, fen), self.file)?;
            return Ok(Rhs::Project {
                span: span(self.file, hst, fen),
                base: head,
                field,
            });
        }
        if is_type_name(&head) || self.cur_ident().is_some() {
            if let Some(op) = binop_from_ident(&head) {
                let ty = self.parse_type()?;
                let lhs = self.parse_operand()?;
                let rhs = self.parse_operand()?;
                self.ensure_anf_operand(&lhs)?;
                self.ensure_anf_operand(&rhs)?;
                if self.cur_ident() == Some("as") {
                    let (s, e) = self.span_of();
                    return Err(e_parse(self.file, s, e));
                }
                return Ok(Rhs::BinOp {
                    span: span(self.file, hst, self.lex.byte_pos()),
                    op,
                    ty,
                    lhs,
                    rhs,
                });
            }
        }
        // bare name
        Ok(Rhs::Operand(Operand::Name(head, span(self.file, hst, hen))))
    }

    fn parse_call_rhs(&mut self, start: u32) -> Result<Rhs, Diag> {
        let (callee, cst, cen) = self.expect_ident()?;
        Self::check_user_name(&callee, (cst, cen), self.file)?;
        if !self.eat_tok(Tok::LParen)? {
            return Err(self.err_here());
        }
        let mut args = Vec::new();
        if !matches!(&self.cur, Some((Tok::RParen, _, _))) {
            loop {
                let arg = self.parse_operand()?;
                self.ensure_anf_operand(&arg)?;
                if matches!(&arg, Operand::Name(_, _)) || matches!(&arg, Operand::Int(_, _)) {
                    // ok
                }
                args.push(arg);
                if self.eat_tok(Tok::Comma)? {
                    continue;
                }
                break;
            }
        }
        if !self.eat_tok(Tok::RParen)? {
            return Err(self.err_here());
        }
        Ok(Rhs::Call {
            span: span(self.file, start, self.lex.byte_pos()),
            callee,
            args,
        })
    }

    fn parse_operand(&mut self) -> Result<Operand, Diag> {
        let (st, en) = self.span_of();
        match self.cur.take() {
            Some((Tok::Int(v), s, e)) => {
                self.bump()?;
                Ok(Operand::Int(v, span(self.file, s, e)))
            }
            Some((Tok::Float(v), s, e)) => {
                self.bump()?;
                Ok(Operand::Float(v, span(self.file, s, e)))
            }
            Some((Tok::Bytes(b), s, e)) => {
                self.bump()?;
                Ok(Operand::Bytes(b, span(self.file, s, e)))
            }
            Some((Tok::Ident(s), ist, ien)) => {
                self.bump()?;
                if s == "unit" {
                    return Ok(Operand::Unit(span(self.file, ist, ien)));
                }
                if s == "true" {
                    return Ok(Operand::Bool(true, span(self.file, ist, ien)));
                }
                if s == "false" {
                    return Ok(Operand::Bool(false, span(self.file, ist, ien)));
                }
                if s == "call" {
                    return Err(e_parse(self.file, ist, ien));
                }
                if Self::banned_op(&s) {
                    return Err(e_parse(self.file, ist, ien));
                }
                if s.starts_with('%') {
                    return Err(e_parse(self.file, ist, ien));
                }
                if self.eat_tok(Tok::LParen)? {
                    return Err(e_parse(self.file, ist, self.lex.byte_pos()));
                }
                Ok(Operand::Name(s, span(self.file, ist, ien)))
            }
            _ => Err(e_parse(self.file, st, en)),
        }
    }

    fn parse_bytes_operand(&mut self) -> Result<Vec<u8>, Diag> {
        match self.cur.take() {
            Some((Tok::Bytes(b), _, _)) => {
                self.bump()?;
                Ok(b)
            }
            _ => Err(self.err_here()),
        }
    }

    fn ensure_anf_operand(&self, op: &Operand) -> Result<(), Diag> {
        if let Operand::Name(n, sp) = op {
            if n == "call" {
                return Err(e_parse(self.file, sp.start, sp.end));
            }
        }
        Ok(())
    }

    fn parse_type(&mut self) -> Result<Type, Diag> {
        let (name, st, en) = self.expect_ident()?;
        Self::check_user_name(&name, (st, en), self.file)?;
        if name == "list" {
            // list T
            let inner = self.parse_type()?;
            return Ok(Type::List(Box::new(inner), span(self.file, st, en)));
        }
        if name == "result" {
            let ok = self.parse_type()?;
            let err = self.parse_type()?;
            return Ok(Type::Result(
                Box::new(ok),
                Box::new(err),
                span(self.file, st, en),
            ));
        }
        Ok(Type::Named(name, span(self.file, st, en)))
    }
}

fn span(file: &str, start: u32, end: u32) -> Span {
    Span {
        file: file.to_string(),
        start,
        end,
    }
}

fn type_is_region(ty: &Type) -> bool {
    matches!(ty, Type::Named(n, _) if n == "region")
}

fn is_dotted_builtin_ident(name: &str) -> bool {
    matches!(
        name,
        "list.empty"
            | "list.cons"
            | "list.len"
            | "list.get"
            | "bytes.new"
            | "bytes.len"
            | "bytes.get"
    )
}

fn split_dotted_project(ident: &str) -> Option<(String, String)> {
    if !ident.contains('.') || is_dotted_builtin_ident(ident) {
        return None;
    }
    let (base, field) = ident.rsplit_once('.')?;
    if base.is_empty() || field.is_empty() {
        return None;
    }
    Some((base.to_string(), field.to_string()))
}

fn is_type_name(name: &str) -> bool {
    matches!(
        name,
        "i32" | "i64" | "f32" | "f64" | "bool" | "unit" | "bytes" | "region"
    )
}

fn ident_starts_type(name: &str) -> bool {
    if is_type_name(name) || name == "list" || name == "result" {
        return true;
    }
    !matches!(
        name,
        "break"
            | "continue"
            | "end"
            | "if"
            | "for"
            | "match"
            | "ret"
            | "trap"
            | "forall"
            | "loop"
            | "else"
            | "strake"
            | "type"
            | "import"
            | "use"
            | "fuel"
            | "pages"
            | "test"
            | "property"
            | "fn"
            | "call"
            | "not"
            | "ok"
            | "err"
            | "hole"
            | "true"
            | "false"
            | "unit"
            | "in"
    )
}

fn binop_from_ident(name: &str) -> Option<BinOp> {
    match name {
        "add" => Some(BinOp::Add),
        "sub" => Some(BinOp::Sub),
        "mul" => Some(BinOp::Mul),
        "div" => Some(BinOp::Div),
        "eq" => Some(BinOp::Eq),
        "ne" => Some(BinOp::Ne),
        "lt" => Some(BinOp::Lt),
        "le" => Some(BinOp::Le),
        "gt" => Some(BinOp::Gt),
        "ge" => Some(BinOp::Ge),
        _ => None,
    }
}

// --- Formatter -----------------------------------------------------------------

fn format_float_literal(v: f64) -> String {
    if !v.is_finite() {
        return format!("{}", v.to_bits());
    }
    let printed = format!("{}", v);
    if printed.contains('.') || printed.contains('e') || printed.contains('E') {
        printed
    } else {
        format!("{}.0", printed)
    }
}

fn blank_line_between_items(prev: &Item, cur: &Item) -> bool {
    !matches!(
        (prev, cur),
        (Item::Fuel(_, _), Item::Fuel(_, _))
            | (Item::Fuel(_, _), Item::Pages(_, _))
            | (Item::Pages(_, _), Item::Fuel(_, _))
            | (Item::Pages(_, _), Item::Pages(_, _))
    )
}

struct Fmt {
    out: String,
    indent: usize,
}

impl Fmt {
    fn new() -> Self {
        Self {
            out: String::new(),
            indent: 0,
        }
    }

    fn format_module(&mut self, module: &Module) -> String {
        self.writeln(&format!("strake {}", module.version));
        let header_blank = !matches!(
            module.items.first(),
            Some(Item::Fuel(_, _) | Item::Pages(_, _))
        );
        if header_blank && !module.items.is_empty() {
            self.blank();
        }
        let mut prev: Option<&Item> = None;
        for item in &module.items {
            if let Some(p) = prev {
                if blank_line_between_items(p, item) {
                    self.blank();
                }
            }
            self.format_item(item);
            prev = Some(item);
        }
        if !self.out.ends_with('\n') {
            self.out.push('\n');
        }
        self.out.clone()
    }

    fn blank(&mut self) {
        self.out.push('\n');
    }

    fn writeln(&mut self, line: &str) {
        if line.is_empty() {
            self.out.push('\n');
        } else {
            self.out.push_str(&"  ".repeat(self.indent));
            self.out.push_str(line);
            self.out.push('\n');
        }
    }

    fn format_item(&mut self, item: &Item) {
        match item {
            Item::TypeDef(td) => self.format_type_def(td),
            Item::Import(im) => self.format_import(im),
            Item::Use(u) => self.format_use(u),
            Item::Fuel(n, _) => self.writeln(&format!("fuel {}", n)),
            Item::Pages(n, _) => self.writeln(&format!("pages {}", n)),
            Item::Fn(f) => self.format_fn(f),
            Item::Test(t) => self.format_test(t),
            Item::Property(p) => self.format_property(p),
        }
    }

    fn format_type_def(&mut self, td: &TypeDef) {
        match &td.kind {
            TypeDefKind::Record(fields) => {
                self.write("type ");
                self.out.push_str(&td.name);
                self.out.push_str(" { ");
                for (i, f) in fields.iter().enumerate() {
                    if i > 0 {
                        self.out.push_str(", ");
                    }
                    self.out.push_str(&f.name);
                    self.out.push_str(": ");
                    self.write_type(&f.ty);
                }
                self.out.push_str(" }");
                self.out.push('\n');
            }
            TypeDefKind::Sum(variants) => {
                self.write("type ");
                self.out.push_str(&td.name);
                self.out.push_str(" { ");
                for (i, v) in variants.iter().enumerate() {
                    if i > 0 {
                        self.out.push_str(" | ");
                    }
                    self.out.push_str(&v.tag);
                    if let Some(ty) = &v.payload {
                        self.out.push(' ');
                        self.write_type(ty);
                    }
                }
                self.out.push_str(" }");
                self.out.push('\n');
            }
        }
    }

    fn write(&mut self, s: &str) {
        self.out.push_str(&"  ".repeat(self.indent));
        self.out.push_str(s);
    }

    fn format_import(&mut self, im: &ImportDecl) {
        match &im.sig {
            ImportSig::Region => {
                self.writeln("import region mem : region");
            }
            ImportSig::Fn { params, ret } => {
                let mut line = format!("import {} : (", im.path);
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        line.push_str(", ");
                    }
                    line.push_str(&self.type_string(p));
                }
                line.push_str(") -> ");
                line.push_str(&self.type_string(ret));
                self.writeln(&line);
            }
        }
    }

    fn format_use(&mut self, u: &UseDecl) {
        self.writeln(&format!("use {}@blake3:{}", u.name, u.hash));
    }

    fn format_fn(&mut self, f: &FnItem) {
        let mut head = String::new();
        if f.private {
            head.push_str("priv ");
        }
        head.push_str("fn ");
        head.push_str(&f.name);
        head.push('(');
        for (i, p) in f.params.iter().enumerate() {
            if i > 0 {
                head.push_str(", ");
            }
            head.push_str(&p.name);
            head.push_str(": ");
            head.push_str(&self.type_string(&p.ty));
        }
        head.push_str(") -> ");
        head.push_str(&self.type_string(&f.ret));
        self.writeln(&head);
        self.indent += 1;
        for stmt in &f.body {
            self.format_stmt(stmt);
        }
        self.indent -= 1;
        self.writeln("end");
    }

    fn format_test(&mut self, t: &TestItem) {
        self.writeln(&format!("test {}", t.name));
        self.indent += 1;
        for stmt in &t.body {
            self.format_stmt(stmt);
        }
        self.indent -= 1;
        self.writeln("end");
    }

    fn format_property(&mut self, p: &PropertyItem) {
        self.writeln(&format!("property {}", p.name));
        self.indent += 1;
        for stmt in &p.body {
            self.format_stmt(stmt);
        }
        self.indent -= 1;
        self.writeln("end");
    }

    fn format_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Assign { name, rhs, .. } => {
                let rhs_s = self.format_rhs(rhs);
                self.writeln(&format!("{} = {}", name, rhs_s));
            }
            Stmt::Ret { value, .. } => {
                self.writeln(&format!("ret {}", self.format_operand(value)));
            }
            Stmt::If {
                cond,
                then_body,
                else_body,
                ..
            } => {
                self.writeln(&format!("if {}", self.format_operand(cond)));
                self.indent += 1;
                for s in then_body {
                    self.format_stmt(s);
                }
                self.indent -= 1;
                if !else_body.is_empty() {
                    self.writeln("else");
                    self.indent += 1;
                    for s in else_body {
                        self.format_stmt(s);
                    }
                    self.indent -= 1;
                }
                self.writeln("end");
            }
            Stmt::Loop { params, body, .. } => {
                if params.is_empty() {
                    self.writeln("loop");
                } else {
                    let p = params
                        .iter()
                        .map(|lp| format!("{} = {}", lp.name, self.format_operand(&lp.init)))
                        .collect::<Vec<_>>()
                        .join(" ");
                    self.writeln(&format!("loop {}", p));
                }
                self.indent += 1;
                for s in body {
                    self.format_stmt(s);
                }
                self.indent -= 1;
                self.writeln("end");
            }
            Stmt::Break { .. } => self.writeln("break"),
            Stmt::Continue { edges, .. } => {
                if edges.is_empty() {
                    self.writeln("continue");
                } else {
                    let e = edges
                        .iter()
                        .map(|edge| format!("{} = {}", edge.name, self.format_operand(&edge.value)))
                        .collect::<Vec<_>>()
                        .join(" ");
                    self.writeln(&format!("continue {}", e));
                }
            }
            Stmt::For {
                binder, list, body, ..
            } => {
                self.writeln(&format!("for {} in {}", binder, list));
                self.indent += 1;
                for s in body {
                    self.format_stmt(s);
                }
                self.indent -= 1;
                self.writeln("end");
            }
            Stmt::Match {
                scrutinee, arms, ..
            } => {
                self.writeln(&format!("match {}", scrutinee));
                self.indent += 1;
                for arm in arms {
                    let mut line = arm.tag.clone();
                    if let Some(b) = &arm.binder {
                        line.push(' ');
                        line.push_str(b);
                    }
                    line.push_str(" ->");
                    self.writeln(&line);
                    self.indent += 1;
                    for s in &arm.body {
                        self.format_stmt(s);
                    }
                    self.indent -= 1;
                }
                self.indent -= 1;
                self.writeln("end");
            }
            Stmt::Forall {
                binder,
                bound,
                body,
                ..
            } => {
                let header = match bound {
                    ForallBound::Range { ty, lo, hi, .. } => {
                        format!(
                            "forall {}: {} in {}..{}",
                            binder,
                            self.type_string(ty),
                            lo,
                            hi
                        )
                    }
                    ForallBound::List { ty, name, .. } => {
                        format!("forall {}: {} in {}", binder, self.type_string(ty), name)
                    }
                };
                self.writeln(&header);
                self.indent += 1;
                for s in body {
                    self.format_stmt(s);
                }
                self.indent -= 1;
                self.writeln("end");
            }
            Stmt::Trap { .. } => self.writeln("trap"),
            Stmt::AssertEq { got, want, .. } => {
                self.writeln(&format!(
                    "assert.eq {} {}",
                    self.format_operand(got),
                    self.format_operand(want)
                ));
            }
        }
    }

    fn format_rhs(&self, rhs: &Rhs) -> String {
        match rhs {
            Rhs::BinOp {
                op, ty, lhs, rhs, ..
            } => {
                format!(
                    "{} {} {} {}",
                    binop_name(op),
                    self.type_string(ty),
                    self.format_operand(lhs),
                    self.format_operand(rhs)
                )
            }
            Rhs::Not { operand, .. } => format!("not {}", self.format_operand(operand)),
            Rhs::Call { callee, args, .. } => {
                let a = args
                    .iter()
                    .map(|o| self.format_operand(o))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("call {}({})", callee, a)
            }
            Rhs::Ok { ty, value, .. } => {
                format!("ok {} {}", self.type_string(ty), self.format_operand(value))
            }
            Rhs::Err { ty, value, .. } => format!(
                "err {} {}",
                self.type_string(ty),
                self.format_operand(value)
            ),
            Rhs::ListEmpty { elem, .. } => format!("list.empty {}", self.type_string(elem)),
            Rhs::ListCons { head, tail, .. } => format!(
                "list.cons {} {}",
                self.format_operand(head),
                self.format_operand(tail)
            ),
            Rhs::ListLen { list, .. } => format!("list.len {}", self.format_operand(list)),
            Rhs::ListGet { list, index, .. } => format!(
                "list.get {} {}",
                self.format_operand(list),
                self.format_operand(index)
            ),
            Rhs::BytesNew { hex, .. } => format!("bytes.new {}", self.format_bytes_lit(hex)),
            Rhs::BytesLen { bytes, .. } => format!("bytes.len {}", self.format_operand(bytes)),
            Rhs::BytesGet { bytes, index, .. } => format!(
                "bytes.get {} {}",
                self.format_operand(bytes),
                self.format_operand(index)
            ),
            Rhs::Load { ty, addr, .. } => format!(
                "load {} {}",
                self.type_string(ty),
                self.format_operand(addr)
            ),
            Rhs::Store {
                ty, addr, value, ..
            } => format!(
                "store {} {} {}",
                self.type_string(ty),
                self.format_operand(addr),
                self.format_operand(value)
            ),
            Rhs::Hole { ty, .. } => format!("hole {}", self.type_string(ty)),
            Rhs::Record { fields, .. } => {
                let inner = fields
                    .iter()
                    .map(|(n, v)| format!("{}: {}", n, self.format_operand(v)))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{{ {} }}", inner)
            }
            Rhs::Project { base, field, .. } => format!("{}.{}", base, field),
            Rhs::Operand(op) => self.format_operand(op),
        }
    }

    fn format_operand(&self, op: &Operand) -> String {
        match op {
            Operand::Name(n, _) => n.clone(),
            Operand::Int(v, _) => v.to_string(),
            Operand::Float(v, _) => format_float_literal(*v),
            Operand::Bytes(b, _) => self.format_bytes_lit(b),
            Operand::Unit(_) => "unit".into(),
            Operand::Bool(true, _) => "true".into(),
            Operand::Bool(false, _) => "false".into(),
        }
    }

    fn format_bytes_lit(&self, bytes: &[u8]) -> String {
        use std::fmt::Write as _;
        let mut hex = String::new();
        for b in bytes {
            write!(hex, "{:02x}", b).expect("hex digit");
        }
        format!("x\"{}\"", hex)
    }

    fn type_string(&self, ty: &Type) -> String {
        match ty {
            Type::Named(n, _) => n.clone(),
            Type::List(inner, _) => format!("list {}", self.type_string(inner)),
            Type::Result(ok, err, _) => {
                format!("result {} {}", self.type_string(ok), self.type_string(err))
            }
        }
    }

    fn write_type(&mut self, ty: &Type) {
        self.out.push_str(&self.type_string(ty));
    }
}

fn binop_name(op: &BinOp) -> &'static str {
    match op {
        BinOp::Add => "add",
        BinOp::Sub => "sub",
        BinOp::Mul => "mul",
        BinOp::Div => "div",
        BinOp::Eq => "eq",
        BinOp::Ne => "ne",
        BinOp::Lt => "lt",
        BinOp::Le => "le",
        BinOp::Gt => "gt",
        BinOp::Ge => "ge",
    }
}

pub fn parse(source: &str, file: &str) -> Result<Module, Diag> {
    let mut p = Parser::init(source, file)?;
    p.parse_module()
}

// --- Tests ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const ADD2: &str = r#"strake 1

type pair { a: i64, b: i64 }

fn add2(a: i64, b: i64) -> i64
  x = add i64 a b
  y = mul i64 x 2
  ret y
end

test add2.basic
  r = call add2(3, 4)
  assert.eq r 14
end

property add2.zero
  forall n: i64 in -10..10
    r = call add2(n, 0)
    w = mul i64 n 2
    assert.eq r w
  end
end
"#;

    #[test]
    fn add2_parse_and_fmt_idempotent() {
        let m = parse(ADD2, "add2.strake").expect("parse add2");
        let f1 = format(&m);
        let m2 = parse(&f1, "add2.strake").expect("re-parse");
        let f2 = format(&m2);
        assert_eq!(f1, f2);
    }

    #[test]
    fn i64_min_literal() {
        let src = "strake 1\n\nfn f() -> i64\n  x = add i64 -9223372036854775808 0\n  ret x\nend\n";
        let m = parse(src, "t.strake").expect("min i64 literal");
        let stmt = &m.items[0];
        let Item::Fn(f) = stmt else { panic!("fn") };
        let Stmt::Assign { rhs, .. } = &f.body[0] else {
            panic!("assign")
        };
        let Rhs::BinOp { lhs, .. } = rhs else {
            panic!("binop")
        };
        let Operand::Int(v, _) = lhs else {
            panic!("int")
        };
        assert_eq!(*v, -9223372036854775808);
    }

    #[test]
    fn e_parse_nested_call_operand() {
        let src = "strake 1\n\nfn f() -> i64\n  x = add i64 (call g(1)) 2\n  ret x\nend\n";
        let err = parse(src, "t.strake").unwrap_err();
        assert_eq!(err.code, Code::EParse);
    }

    #[test]
    fn e_parse_assert_eq_in_fn() {
        let src = "strake 1\n\nfn f() -> unit\n  assert.eq 1 2\n  ret unit\nend\n";
        let err = parse(src, "t.strake").unwrap_err();
        assert_eq!(err.code, Code::EParse);
    }

    #[test]
    fn e_parse_percent_name() {
        let src = "strake 1\n\nfn f() -> i64\n  %x = add i64 1 2\n  ret %x\nend\n";
        let err = parse(src, "t.strake").unwrap_err();
        assert_eq!(err.code, Code::EParse);
    }

    #[test]
    fn e_parse_odd_hex() {
        let src = "strake 1\n\nfn f() -> unit\n  x = bytes.new x\"abc\"\n  ret unit\nend\n";
        let err = parse(src, "t.strake").unwrap_err();
        assert_eq!(err.code, Code::EParse);
    }

    #[test]
    fn e_parse_closed_absence_and() {
        let src = "strake 1\n\nfn f() -> bool\n  x = and bool a b\n  ret x\nend\n";
        let err = parse(src, "t.strake").unwrap_err();
        assert_eq!(err.code, Code::EParse);
    }

    #[test]
    fn e_parse_cast_as() {
        let src = "strake 1\n\nfn f() -> i64\n  x = add i64 1 2 as i32\n  ret x\nend\n";
        let err = parse(src, "t.strake").unwrap_err();
        assert_eq!(err.code, Code::EParse);
    }

    #[test]
    fn parse_control_and_not() {
        let src = r#"strake 1

fn ctrl() -> unit
  if flag
    x = not flag
  else
    y = add i64 1 2
  end
  loop i = 0
    break
  end
  for v in xs
    z = mul i64 v 2
  end
  match r
    ok val ->
      w = add i64 val 1
    err _ ->
      trap
  end
  ret unit
end

property p
  forall k: i64 in 0..1
    t = not ok
  end
end
"#;
        parse(src, "t.strake").expect("control constructs");
    }

    #[test]
    fn codeless_duplicate_fuel() {
        let src = "strake 1\nfuel 1\nfuel 2\nfn f() -> unit\n  ret unit\nend\n";
        let m = parse(src, "t.strake").unwrap();
        assert!(codeless_rejection(&m).is_some());
    }

    #[test]
    fn missing_header_e_parse() {
        let err = parse("fn f() -> unit\n  ret unit\nend\n", "t.strake").unwrap_err();
        assert_eq!(err.code, Code::EParse);
    }

    #[test]
    fn assert_eq_parses_got_then_want() {
        let src = "strake 1\n\ntest t\n  assert.eq r 14\nend\n";
        let m = parse(src, "t.strake").expect("parse");
        let Item::Test(t) = &m.items[0] else {
            panic!("test")
        };
        let Stmt::AssertEq { got, want, .. } = &t.body[0] else {
            panic!("assert")
        };
        let Operand::Name(g, _) = got else {
            panic!("got name")
        };
        let Operand::Int(w, _) = want else {
            panic!("want int")
        };
        assert_eq!(g, "r");
        assert_eq!(*w, 14);
    }

    #[test]
    fn literal_zero_assign_parses() {
        let src = "strake 1\n\nfn f() -> unit\n  skipped = 0\n  ret unit\nend\n";
        parse(src, "t.strake").expect("zero assign");
    }

    #[test]
    fn project_dotted_roundtrip() {
        let src =
            "strake 1\n\ntype pair { x: i64 }\n\nfn f(p: pair) -> i64\n  v = p.x\n  ret v\nend\n";
        let m = parse(src, "t.strake").expect("parse");
        let f1 = format(&m);
        assert!(
            f1.contains("v = p.x\n"),
            "fmt must preserve projection field: {f1}"
        );
        let m2 = parse(&f1, "t.strake").expect("reparse");
        assert_eq!(format(&m2), f1);
    }

    #[test]
    fn tab_indented_body_parses() {
        let src = "strake 1\n\nfn f() -> i64\n\tx = add i64 1 2\n\tret x\nend\n";
        parse(src, "t.strake").expect("tabs");
    }

    #[test]
    fn i128_literal_above_i64_max() {
        let src = "strake 1\n\nfn f() -> i64\n  x = add i64 9223372036854775808 0\n  ret x\nend\n";
        let m = parse(src, "t.strake").expect("big int");
        let Item::Fn(f) = &m.items[0] else {
            panic!("fn")
        };
        let Stmt::Assign { rhs, .. } = &f.body[0] else {
            panic!("assign")
        };
        let Rhs::BinOp { lhs, .. } = rhs else {
            panic!("binop")
        };
        let Operand::Int(v, _) = lhs else {
            panic!("int")
        };
        assert_eq!(*v, 9223372036854775808);
    }

    #[test]
    fn pages_directive_large_u64() {
        let src = "strake 1\npages 9223372036854775808\n\nfn f() -> unit\n  ret unit\nend\n";
        let m = parse(src, "t.strake").expect("pages");
        let Item::Pages(n, _) = &m.items[0] else {
            panic!("pages")
        };
        assert_eq!(*n, 9223372036854775808);
    }

    #[test]
    fn e_parse_percent_test_name() {
        let err = parse("strake 1\n\ntest %t\n  ret unit\nend\n", "t.strake").unwrap_err();
        assert_eq!(err.code, Code::EParse);
    }

    #[test]
    fn e_parse_percent_property_name() {
        let err = parse(
            "strake 1\n\nproperty %p\n  forall n: i64 in 0..0\n  end\nend\n",
            "t.strake",
        )
        .unwrap_err();
        assert_eq!(err.code, Code::EParse);
    }

    #[test]
    fn e_parse_percent_for_list() {
        let src = "strake 1\n\nfn f() -> unit\n  for v in %xs\n    trap\n  end\n  ret unit\nend\n";
        assert_eq!(parse(src, "t.strake").unwrap_err().code, Code::EParse);
    }

    #[test]
    fn e_parse_percent_match_scrutinee() {
        let src = "strake 1\n\nfn f() -> unit\n  match %r\n    ok _ ->\n      trap\n  end\n  ret unit\nend\n";
        assert_eq!(parse(src, "t.strake").unwrap_err().code, Code::EParse);
    }

    #[test]
    fn e_parse_percent_call_callee() {
        let src = "strake 1\n\ntest t\n  x = call %g(1)\nend\n";
        assert_eq!(parse(src, "t.strake").unwrap_err().code, Code::EParse);
    }

    #[test]
    fn e_parse_percent_forall_list() {
        let src = "strake 1\n\nproperty p\n  forall n: i64 in %xs\n    trap\n  end\nend\n";
        assert_eq!(parse(src, "t.strake").unwrap_err().code, Code::EParse);
    }

    #[test]
    fn e_parse_closed_absence_or() {
        let src = "strake 1\n\nfn f() -> bool\n  x = or bool a b\n  ret x\nend\n";
        assert_eq!(parse(src, "t.strake").unwrap_err().code, Code::EParse);
    }

    #[test]
    fn e_parse_closed_absence_rem() {
        let src = "strake 1\n\nfn f() -> i64\n  x = rem i64 a b\n  ret x\nend\n";
        assert_eq!(parse(src, "t.strake").unwrap_err().code, Code::EParse);
    }

    #[test]
    fn e_parse_closed_absence_xor() {
        let src = "strake 1\n\nfn f() -> i64\n  x = xor i64 a b\n  ret x\nend\n";
        assert_eq!(parse(src, "t.strake").unwrap_err().code, Code::EParse);
    }

    #[test]
    fn e_parse_closed_absence_shl() {
        let src = "strake 1\n\nfn f() -> i64\n  x = shl i64 a b\n  ret x\nend\n";
        assert_eq!(parse(src, "t.strake").unwrap_err().code, Code::EParse);
    }

    #[test]
    fn e_parse_closed_absence_shr() {
        let src = "strake 1\n\nfn f() -> i64\n  x = shr i64 a b\n  ret x\nend\n";
        assert_eq!(parse(src, "t.strake").unwrap_err().code, Code::EParse);
    }

    #[test]
    fn e_parse_keyword_nested_call_operand() {
        let src = "strake 1\n\nfn f() -> i64\n  x = add i64 call g(1) 2\n  ret x\nend\n";
        assert_eq!(parse(src, "t.strake").unwrap_err().code, Code::EParse);
    }

    #[test]
    fn forall_range_keeps_type_in_fmt() {
        let src = "strake 1\n\nproperty p\n  forall n: i32 in 0..1\n    trap\n  end\nend\n";
        let m = parse(src, "t.strake").expect("parse");
        let f = format(&m);
        assert!(f.contains("forall n: i32 in 0..1"));
    }

    #[test]
    fn match_arm_stops_on_custom_tag() {
        let src = r#"strake 1

type rgb { Red i64 | Blue i64 }

fn f(c: rgb) -> i64
  match c
    Red x ->
      x = add i64 x 1
    Blue y ->
      ret y
  end
  ret 0
end
"#;
        parse(src, "t.strake").expect("custom match tags");
    }

    #[test]
    fn e_parse_match_arm_tag_ok_capital() {
        let src = "strake 1\n\nfn f() -> unit\n  match r\n    Ok x ->\n      trap\n  end\n  ret unit\nend\n";
        assert_eq!(parse(src, "t.strake").unwrap_err().code, Code::EParse);
    }

    #[test]
    fn import_region_mem_roundtrip() {
        let src = "strake 1\n\nimport region mem : region\n\nfn f() -> unit\n  ret unit\nend\n";
        let m = parse(src, "t.strake").expect("region import");
        let f = format(&m);
        assert!(f.contains("import region mem : region\n"));
        parse(&f, "t.strake").expect("reparse region import");
    }

    #[test]
    fn use_blake3_hash_leading_digit() {
        let src = "strake 1\n\nuse pkg@blake3:0123abcd\n\nfn f() -> unit\n  ret unit\nend\n";
        parse(src, "t.strake").expect("leading-digit blake3 hash");
    }

    #[test]
    fn float_literal_keeps_decimal_in_fmt() {
        let src = "strake 1\n\ntest t\n  x = add f64 1.0 2.0\nend\n";
        let m = parse(src, "t.strake").expect("parse float");
        let f = format(&m);
        assert!(f.contains("1.0"), "fmt output: {f}");
    }

    #[test]
    fn e_parse_percent_record_field() {
        let src = "strake 1\n\ntype pair { %a: i64 }\n\nfn f() -> unit\n  ret unit\nend\n";
        assert_eq!(parse(src, "t.strake").unwrap_err().code, Code::EParse);
    }

    #[test]
    fn e_parse_percent_project_field() {
        let src =
            "strake 1\n\ntype pair { x: i64 }\n\nfn f(p: pair) -> i64\n  v = p.%x\n  ret v\nend\n";
        assert_eq!(parse(src, "t.strake").unwrap_err().code, Code::EParse);
    }

    #[test]
    fn continue_then_ret_parses() {
        let src = "strake 1\n\nfn f() -> i64\n  loop i = 0\n    continue\n    ret 1\n  end\n  ret 0\nend\n";
        parse(src, "t.strake").expect("continue then ret");
    }

    #[test]
    fn loop_without_params_then_ret_parses() {
        let src = "strake 1\n\nfn f() -> i64\n  loop\n    ret 1\n  end\n  ret 0\nend\n";
        parse(src, "t.strake").expect("loop then ret");
    }

    #[test]
    fn loop_not_stmt_assign_rhs_as_header_param() {
        let src = "strake 1\n\nfn f() -> i64\n  loop\n    n = add i64 1 2\n  end\n  ret 0\nend\n";
        parse(src, "t.strake").expect("loop body assign not header param");
    }

    #[test]
    fn loop_two_header_params() {
        let src = "strake 1\n\nfn f() -> i64\n  loop a = 0 b = 1\n    break\n  end\n  ret 0\nend\n";
        let m = parse(src, "t.strake").expect("loop two params");
        let Item::Fn(f) = &m.items[0] else {
            panic!("fn")
        };
        let Stmt::Loop { params, .. } = &f.body[0] else {
            panic!("loop")
        };
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].name, "a");
        assert_eq!(params[1].name, "b");
    }

    #[test]
    fn continue_then_assign_parses() {
        let src = "strake 1\n\nfn f() -> i64\n  loop i = 0\n    continue\n    n = add i64 1 2\n  end\n  ret 0\nend\n";
        parse(src, "t.strake").expect("continue then assign");
    }

    #[test]
    fn continue_assign_rewind_not_edge() {
        let src = "strake 1\n\nfn f() -> i64\n  loop i = 0\n    continue n = add i64 1 2\n    ret 0\n  end\n  ret 0\nend\n";
        let m = parse(src, "t.strake").expect("continue rewinds stmt assign");
        let Item::Fn(f) = &m.items[0] else {
            panic!("fn")
        };
        let Stmt::Loop { body, .. } = &f.body[0] else {
            panic!("loop")
        };
        let Stmt::Continue { edges, .. } = &body[0] else {
            panic!("continue")
        };
        assert!(edges.is_empty(), "n = add must not be a continue edge");
        let Stmt::Assign { name, .. } = &body[1] else {
            panic!("assign after continue")
        };
        assert_eq!(name, "n");
    }

    #[test]
    fn e_parse_percent_record_literal_field() {
        let src = "strake 1\n\nfn f() -> unit\n  r = { %a: 1 }\n  ret unit\nend\n";
        assert_eq!(parse(src, "t.strake").unwrap_err().code, Code::EParse);
    }

    #[test]
    fn e_parse_percent_project_base() {
        let src = "strake 1\n\nfn f() -> i64\n  v = %p.x\n  ret v\nend\n";
        assert_eq!(parse(src, "t.strake").unwrap_err().code, Code::EParse);
    }

    #[test]
    fn e_parse_percent_sum_tag() {
        let src = "strake 1\n\ntype t { a | %b i64 }\n\nfn f() -> unit\n  ret unit\nend\n";
        assert_eq!(parse(src, "t.strake").unwrap_err().code, Code::EParse);
    }

    #[test]
    fn use_blake3_hash_long_leading_digits() {
        let src = "strake 1\n\nuse pkg@blake3:9999999999999999999999999999999999999999abcd\n\nfn f() -> unit\n  ret unit\nend\n";
        parse(src, "t.strake").expect("long leading-digit blake3 hash");
    }

    #[test]
    fn signed_float_literal_fmt_roundtrip() {
        let src = "strake 1\n\ntest t\n  a = add f64 -1.5 1.0\n  b = add f64 -1.0 0.0\n  c = add f64 -10.25 -0.5\nend\n";
        let m = parse(src, "t.strake").expect("parse signed floats");
        let f = format(&m);
        assert!(f.contains("-1.5"), "fmt: {f}");
        assert!(f.contains("-1.0"), "fmt: {f}");
        assert!(f.contains("-10.25"), "fmt: {f}");
        assert!(f.contains("-0.5"), "fmt: {f}");
        let m2 = parse(&f, "t.strake").expect("reparse signed floats");
        let f2 = format(&m2);
        assert_eq!(f, f2);
    }

    #[test]
    fn format_non_finite_float_not_pseudo_decimal() {
        let inf = format_float_literal(f64::INFINITY);
        let ninf = format_float_literal(f64::NEG_INFINITY);
        let nan = format_float_literal(f64::NAN);
        assert!(!inf.to_ascii_lowercase().contains("inf"));
        assert!(!ninf.to_ascii_lowercase().contains("inf"));
        assert!(!nan.to_ascii_lowercase().contains("nan"));
        assert!(!inf.ends_with(".0"));
        assert!(!ninf.ends_with(".0"));
        assert!(!nan.ends_with(".0"));
    }

    #[test]
    fn e_parse_exponent_float_literal() {
        let src = "strake 1\n\ntest t\n  x = add f64 1e2 1.0\nend\n";
        assert_eq!(parse(src, "t.strake").unwrap_err().code, Code::EParse);
    }

    #[test]
    fn loop_body_project_not_header() {
        let src =
            "strake 1\n\nfn f() -> i64\n  loop\n    v = p.x\n    break\n  end\n  ret 0\nend\n";
        let m = parse(src, "t.strake").expect("parse");
        let f = format(&m);
        assert!(
            !f.contains("loop v = p.x"),
            "projection must stay in body: {f}"
        );
        assert!(f.contains("v = p.x"), "fmt: {f}");
    }

    #[test]
    fn continue_body_project_not_edge() {
        let src = "strake 1\n\nfn f() -> i64\n  loop i = 0\n    continue\n    v = p.x\n  end\n  ret 0\nend\n";
        let m = parse(src, "t.strake").expect("parse");
        let f = format(&m);
        assert!(
            !f.contains("continue v = p.x"),
            "projection must stay in body: {f}"
        );
        assert!(f.contains("v = p.x"), "fmt: {f}");
    }

    #[test]
    fn loop_body_ok_err_hole() {
        let src = "strake 1\n\nfn f() -> unit\n  loop\n    r = ok i64 1\n    s = err i64 2\n    t = hole i64\n  end\n  ret unit\nend\n";
        parse(src, "t.strake").expect("ok/err/hole in loop body");
    }

    #[test]
    fn loop_header_hole_then_break_fmt_roundtrip() {
        let src =
            "strake 1\n\nfn f() -> unit\n  loop x = hole\n    break\n  end\n  ret unit\nend\n";
        let m = parse(src, "t.strake").expect("loop header hole then break");
        let f = format(&m);
        assert!(
            !f.contains("hole break"),
            "break must not fuse into hole rhs: {f}"
        );
        assert!(f.contains("break"), "fmt: {f}");
        let m2 = parse(&f, "t.strake").expect("reparse");
        assert_eq!(format(&m2), f);
    }

    #[test]
    fn loop_header_ok_err_stmt_rhs() {
        let ok_src =
            "strake 1\n\nfn f() -> unit\n  loop x = ok i64 1\n    break\n  end\n  ret unit\nend\n";
        parse(ok_src, "t.strake").expect("loop header ok stmt rhs");
        let err_src =
            "strake 1\n\nfn f() -> unit\n  loop x = err i64 2\n    break\n  end\n  ret unit\nend\n";
        parse(err_src, "t.strake").expect("loop header err stmt rhs");
    }

    #[test]
    fn continue_header_ok_stmt_rhs() {
        let src = "strake 1\n\nfn f() -> unit\n  loop i = 0\n    continue i = ok i64 1\n    break\n  end\n  ret unit\nend\n";
        let m = parse(src, "t.strake").expect("continue header ok stmt rhs");
        let f = format(&m);
        let m2 = parse(&f, "t.strake").expect("reparse continue ok");
        assert_eq!(format(&m2), f);
    }

    #[test]
    fn loop_spaced_projection_after_paramless_loop() {
        let src =
            "strake 1\n\nfn f() -> i64\n  loop\n    v = p . x\n    break\n  end\n  ret 0\nend\n";
        let m = parse(src, "t.strake").expect("spaced projection after loop");
        let f = format(&m);
        assert!(f.contains("v = p.x") || f.contains("v = p . x"), "fmt: {f}");
        let m2 = parse(&f, "t.strake").expect("reparse spaced projection");
        assert_eq!(format(&m2), f);
    }

    #[test]
    fn continue_spaced_projection_after_bare_continue() {
        let src = "strake 1\n\nfn f() -> i64\n  loop i = 0\n    continue\n    v = p . x\n  end\n  ret 0\nend\n";
        let m = parse(src, "t.strake").expect("spaced projection after continue");
        let f = format(&m);
        assert!(
            !f.contains("continue v = p"),
            "projection must stay in body: {f}"
        );
        let m2 = parse(&f, "t.strake").expect("reparse");
        assert_eq!(format(&m2), f);
    }

    #[test]
    fn e_parse_percent_type_position() {
        let src = "strake 1\n\nfn f(a: %t) -> i64\n  ret a\nend\n";
        assert_eq!(parse(src, "t.strake").unwrap_err().code, Code::EParse);
    }

    #[test]
    fn e_parse_percent_spaced_projection_base() {
        let src = "strake 1\n\nfn f() -> i64\n  v = %p . x\n  ret v\nend\n";
        assert_eq!(parse(src, "t.strake").unwrap_err().code, Code::EParse);
    }

    #[test]
    fn e_parse_percent_match_arm_tag() {
        let src = "strake 1\n\nfn f() -> unit\n  match r\n    %b v ->\n      trap\n  end\n  ret unit\nend\n";
        assert_eq!(parse(src, "t.strake").unwrap_err().code, Code::EParse);
    }

    #[test]
    fn e_parse_percent_type_name_span() {
        let src = "strake 1\n\ntype %pair { a: i64 }\n\nfn f() -> unit\n  ret unit\nend\n";
        let err = parse(src, "t.strake").unwrap_err();
        assert_eq!(err.code, Code::EParse);
        assert_eq!(err.span.start, 15);
        assert_eq!(err.span.end, 20);
    }

    #[test]
    fn use_decl_span_ends_at_hash() {
        let src = "strake 1\n\nuse pkg@blake3:abcd\n\nfn f() -> unit\n  ret unit\nend\n";
        let m = parse(src, "t.strake").expect("parse use");
        let Item::Use(u) = &m.items[0] else {
            panic!("use item")
        };
        let slice = &src[u.span.start as usize..u.span.end as usize];
        assert_eq!(slice, "use pkg@blake3:abcd");
    }
}
