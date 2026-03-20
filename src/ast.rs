#[derive(Debug, Clone)]
pub enum Expr {
    Literal(Value),
    Symbol(String),
    Binary(Box<Expr>, Op, Box<Expr>),
    Lambda(Vec<String>, Box<Expr>), // { x; y } -> ...
    Let(Vec<(String, Expr)>, Box<Expr>),
    AttrSet(Vec<(String, Expr)>, bool), // (fields, is_recursive)
    FieldAccess(Box<Expr>, String),
    App(Box<Expr>, Box<Expr>), // func arg
}

#[derive(Debug, Clone)]
pub enum Op { Add, Sub, Mul, Div, Concat, Merge } // ^ や \\
