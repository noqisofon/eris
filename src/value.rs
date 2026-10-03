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
    Poison,
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
                // HashMap order changes from run to run; print in key order.
                let mut keys: Vec<&String> = attrs.keys().collect();
                keys.sort();
                for k in keys {
                    write!(f, "{} = {:?}; ", k, attrs[k])?;
                }
                write!(f, "}}")
            }
            Value::Closure { .. } => write!(f, "<closure>"),
            Value::NativeClosure(_) => write!(f, "<builtin>"),
            Value::Poison => write!(f, "<poison>"),
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

thread_local! {
    static DEBUG_DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl fmt::Debug for Thunk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Printing recurses over the value's shape; cut it off rather than
        // overflow the stack on pathologically deep data.
        let depth = DEBUG_DEPTH.with(|d| {
            d.set(d.get() + 1);
            d.get()
        });
        let result = if depth > crate::eval::max_depth() {
            write!(f, "...")
        } else {
            match &*self.0.borrow() {
                ThunkState::Evaluated(val) => write!(f, "{:?}", val),
                ThunkState::Evaluating => write!(f, "<evaluating>"),
                ThunkState::Unevaluated { .. } => write!(f, "<thunk>"),
            }
        };
        DEBUG_DEPTH.with(|d| d.set(d.get() - 1));
        result
    }
}

/// Moves the child thunks of an evaluated list/attrset out of `thunk` (leaving
/// it empty) so they can be released iteratively.
fn take_children(thunk: &Rc<RefCell<ThunkState>>, out: &mut Vec<Thunk>) {
    let Ok(mut state) = thunk.try_borrow_mut() else {
        return;
    };
    if let ThunkState::Evaluated(val) = &mut *state {
        match val {
            Value::List(items) => out.append(items),
            Value::AttrSet(map) => out.extend(map.drain().map(|(_, t)| t)),
            _ => {}
        }
    }
}

/// Dropping a deeply nested list/attrset would otherwise recurse once per level
/// (`Vec<Thunk>` -> `Thunk` -> `Vec<Thunk>` ...) and overflow the stack, so
/// the last owner of a thunk dismantles the structure with an explicit worklist.
impl Drop for Thunk {
    fn drop(&mut self) {
        if Rc::strong_count(&self.0) != 1 {
            return;
        }
        let mut pending = Vec::new();
        take_children(&self.0, &mut pending);
        while let Some(child) = pending.pop() {
            if Rc::strong_count(&child.0) == 1 {
                take_children(&child.0, &mut pending);
            }
            // `child` drops here with its children already detached, so its own
            // `Drop` finds nothing left to do.
        }
    }
}

thread_local! {
    /// When `Some`, every lazily created thunk is recorded here so that
    /// `eris check --level type` can force the ones the program never needed.
    static TRACKED_THUNKS: RefCell<Option<Vec<Thunk>>> = const { RefCell::new(None) };
}

/// Starts recording every thunk created from now on (see `TRACKED_THUNKS`).
pub fn start_tracking_thunks() {
    TRACKED_THUNKS.with(|t| *t.borrow_mut() = Some(Vec::new()));
}

/// The `index`-th recorded thunk, if there is one yet. Thunks created while
/// earlier ones are being forced are appended, so walking the indices until
/// this returns `None` visits everything.
pub fn tracked_thunk(index: usize) -> Option<Thunk> {
    TRACKED_THUNKS.with(|t| t.borrow().as_ref().and_then(|v| v.get(index).cloned()))
}

/// Stops recording and releases the recorded thunks.
pub fn stop_tracking_thunks() {
    let tracked = TRACKED_THUNKS.with(|t| t.borrow_mut().take());
    drop(tracked);
}

impl Thunk {
    pub fn new(expr: Expr, env: Env) -> Self {
        let thunk = Thunk(Rc::new(RefCell::new(ThunkState::Unevaluated { expr, env })));
        TRACKED_THUNKS.with(|t| {
            if let Some(tracked) = t.borrow_mut().as_mut() {
                tracked.push(thunk.clone());
            }
        });
        thunk
    }

    /// Whether this thunk has not been forced yet.
    pub fn is_unevaluated(&self) -> bool {
        matches!(&*self.0.borrow(), ThunkState::Unevaluated { .. })
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
    pub source: Rc<String>,
    pub filename: Rc<String>,
}

impl Env {
    pub fn new(source: Rc<String>, filename: Rc<String>) -> Self {
        Env {
            bindings: Rc::new(RefCell::new(HashMap::new())),
            with_context: None,
            parent: None,
            source,
            filename,
        }
    }

    pub fn extend(&self) -> Self {
        Env {
            bindings: Rc::new(RefCell::new(HashMap::new())),
            with_context: None,
            parent: Some(Rc::new(self.clone())),
            source: self.source.clone(),
            filename: self.filename.clone(),
        }
    }

    pub fn with_context(&self, thunk: Thunk) -> Self {
        Env {
            bindings: Rc::new(RefCell::new(HashMap::new())),
            with_context: Some(thunk),
            parent: Some(Rc::new(self.clone())),
            source: self.source.clone(),
            filename: self.filename.clone(),
        }
    }

    pub fn define(&self, name: String, thunk: Thunk) {
        self.bindings.borrow_mut().insert(name, thunk);
    }

    /// Every name visible from this scope, innermost first: its own bindings and
    /// those of all enclosing scopes. Shadowed names appear once. Meant for
    /// building "did you mean" suggestions, not for lookups.
    pub fn visible_names(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut scope: Option<&Env> = Some(self);
        while let Some(env) = scope {
            let mut here: Vec<String> = env.bindings.borrow().keys().cloned().collect();
            // Keep suggestions deterministic when two candidates are equally close.
            here.sort();
            for name in here {
                if seen.insert(name.clone()) {
                    names.push(name);
                }
            }
            scope = env.parent.as_deref();
        }
        names
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
