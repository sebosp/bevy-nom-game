//! The syntax tree of the player's parser: a small subset of Rust, enough for the functions of a
//! nom parser.

/// A whole script: the parser functions, the last one is where the parsing starts.
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub fns: Vec<FnDef>,
    /// The paths of the `use` lines, `["nom", "number", "complete", "u8"]`.
    pub uses: Vec<Vec<String>>,
}

impl Program {
    pub fn get(&self, name: &str) -> Option<&FnDef> {
        self.fns.iter().find(|f| f.name == name)
    }
}

/// `fn name(input: &str) -> IResult<&str, T> { ... }`.
#[derive(Debug, Clone, PartialEq)]
pub struct FnDef {
    pub name: String,
    /// The name of the input argument, `input` most of the time.
    pub param: String,
    /// The type of the input argument as written, `&str` or `&[u8]`.
    pub param_type: String,
    pub body: Vec<Stmt>,
    /// The expression returned at the end, without a semicolon.
    pub tail: Option<Expr>,
    pub line: usize,
}

impl FnDef {
    /// `&str` parses text, `&[u8]` bytes.
    pub fn is_text(&self) -> bool {
        self.param_type.contains("str")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Let { pat: Pat, expr: Expr, line: usize },
    Expr { expr: Expr, line: usize },
}

impl Stmt {
    pub fn line(&self) -> usize {
        match self {
            Stmt::Let { line, .. } | Stmt::Expr { line, .. } => *line,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Pat {
    Ident(String),
    Wild,
    Tuple(Vec<Pat>),
}

impl Pat {
    /// The names bound, in order.
    pub fn names(&self) -> Vec<String> {
        match self {
            Pat::Ident(name) => vec![name.clone()],
            Pat::Wild => vec![],
            Pat::Tuple(pats) => pats.iter().flat_map(Pat::names).collect(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
    Deref,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// `alpha1`, `nom::character::complete::u32`, `n`, `str::parse::<u32>`.
    Path {
        segments: Vec<String>,
        generic: Option<String>,
    },
    Call {
        func: Box<Expr>,
        args: Vec<Expr>,
    },
    Method {
        recv: Box<Expr>,
        name: String,
        generic: Option<String>,
        args: Vec<Expr>,
    },
    Field {
        recv: Box<Expr>,
        name: String,
    },
    Try(Box<Expr>),
    Tuple(Vec<Expr>),
    Array(Vec<Expr>),
    /// `[0u8; 4]`.
    Repeat {
        value: Box<Expr>,
        count: Box<Expr>,
    },
    Str(String),
    ByteStr(Vec<u8>),
    Char(char),
    Byte(u8),
    Int(i128),
    Float(f64),
    Bool(bool),
    Ref(Box<Expr>),
    Unary {
        op: UnOp,
        expr: Box<Expr>,
    },
    Binary {
        op: BinOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Cast {
        expr: Box<Expr>,
        ty: String,
    },
    Closure {
        params: Vec<Pat>,
        body: Box<Expr>,
    },
    Struct {
        name: String,
        fields: Vec<(String, Expr)>,
    },
    /// `vec![...]`, `dbg_bytes_and_tail!(x)`.
    Macro {
        name: String,
        args: Vec<Expr>,
    },
    If {
        cond: Box<Expr>,
        then: Box<Expr>,
        otherwise: Option<Box<Expr>>,
    },
    Block {
        stmts: Vec<Stmt>,
        tail: Option<Box<Expr>>,
    },
    Range {
        start: Box<Expr>,
        end: Box<Expr>,
        inclusive: bool,
    },
    /// `v[i]`, `v[a..b]`; `v[..]` is `v`.
    Index {
        recv: Box<Expr>,
        index: Box<Expr>,
    },
}

impl Expr {
    pub fn path(name: &str) -> Expr {
        Expr::Path { segments: vec![name.to_string()], generic: None }
    }

    /// The last segment of a path, `u32` for `nom::character::complete::u32`.
    pub fn path_name(&self) -> Option<&str> {
        match self {
            Expr::Path { segments, .. } => segments.last().map(String::as_str),
            _ => None,
        }
    }
}
