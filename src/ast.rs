#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Int(i64),
    Float(f64),
    Bool(bool),
    String(Vec<StringPart>),
    Path(String),
    List(Vec<Expr>),
    AttrSet {
        is_rec: bool,
        attrs: Vec<(String, Expr)>,
    },
    Ident(String),
    FieldAccess(Box<Expr>, Vec<String>),
    Lambda(Args, Box<Expr>),
    App(Box<Expr>, Box<Expr>),
    LetIn(Vec<(String, Expr)>, Box<Expr>),
    BinOp(Box<Expr>, Op, Box<Expr>),
    ImplicitAccess(Vec<String>),
    With(Box<Expr>, Box<Expr>),
    IfElse(Box<Expr>, Box<Expr>, Box<Expr>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum StringPart {
    Literal(String),
    Interpolation(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Args {
    Positional(Vec<String>),
    Destructure {
        names: Vec<String>,
        ignore_rest: bool,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Eq,
    Neq,
    Lt,
    Lte,
    Gt,
    Gte,
    And,
    Or,
}
