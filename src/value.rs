use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use crate::ast::{Args, Expr};

#[derive(Clone)]
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
    Path(String),
    List(Vec<Thunk>),
    AttrSet(HashMap<String, Thunk>),
    Closure { args: Args, body: Expr, env: Env },
    NativeClosure(Rc<dyn Fn(Value) -> Result<Value, String>>),
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Int(i) => write!(f, "{}", i),
            Value::Float(fl) => write!(f, "{}", fl),
            Value::Bool(b) => write!(f, "{}", b),
            Value::String(s) => write!(f, "\"{}\"", s),
            Value::Path(p) => write!(f, "p'{:?}'", p),
            Value::List(l) => {
                write!(f, "[ ")?;
                for t in l {
                    write!(f, "{:?} ", t)?;
                }
                write!(f, "]")
            }
            Value::AttrSet(attrs) => {
                write!(f, "{{ ")?;
                for (k, v) in attrs {
                    write!(f, "{} = {:?}; ", k, v)?;
                }
                write!(f, "}}")
            }
            Value::Closure { .. } => write!(f, "<closure>"),
            Value::NativeClosure(_) => write!(f, "<builtin>"),
        }
    }
}

pub enum ThunkState {
    Unevaluated { expr: Expr, env: Env },
    Evaluating,
    Evaluated(Value),
}

#[derive(Clone)]
pub struct Thunk(pub Rc<RefCell<ThunkState>>);

impl fmt::Debug for Thunk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &*self.0.borrow() {
            ThunkState::Evaluated(val) => write!(f, "{:?}", val),
            ThunkState::Evaluating => write!(f, "<evaluating>"),
            ThunkState::Unevaluated { .. } => write!(f, "<thunk>"),
        }
    }
}

impl Thunk {
    pub fn new(expr: Expr, env: Env) -> Self {
        Thunk(Rc::new(RefCell::new(ThunkState::Unevaluated { expr, env })))
    }

    pub fn evaluated(val: Value) -> Self {
        Thunk(Rc::new(RefCell::new(ThunkState::Evaluated(val))))
    }
}

#[derive(Clone, Default)]
pub struct Env {
    pub bindings: Rc<RefCell<HashMap<String, Thunk>>>,
    pub with_context: Option<Thunk>,
    pub parent: Option<Rc<Env>>,
}

impl Env {
    pub fn new() -> Self {
        Env {
            bindings: Rc::new(RefCell::new(HashMap::new())),
            with_context: None,
            parent: None,
        }
    }

    pub fn extend(&self) -> Self {
        Env {
            bindings: Rc::new(RefCell::new(HashMap::new())),
            with_context: None,
            parent: Some(Rc::new(self.clone())),
        }
    }

    pub fn with_context(&self, thunk: Thunk) -> Self {
        Env {
            bindings: Rc::new(RefCell::new(HashMap::new())),
            with_context: Some(thunk),
            parent: Some(Rc::new(self.clone())),
        }
    }

    pub fn define(&self, name: String, thunk: Thunk) {
        self.bindings.borrow_mut().insert(name, thunk);
    }

    pub fn get(&self, name: &str) -> Option<Thunk> {
        if let Some(thunk) = self.bindings.borrow().get(name) {
            return Some(thunk.clone());
        }
        if let Some(parent) = &self.parent {
            return parent.get(name);
        }
        None
    }
}
