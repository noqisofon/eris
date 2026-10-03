//! A static check for variables that are not bound anywhere.
//!
//! It walks the AST with the same lexical scoping rules the evaluator uses, so
//! it can report `Variable 'x' not found` for code that never runs (a branch not
//! taken, a function nobody calls) without evaluating anything.

use crate::ast::*;
use ariadne::{Color, Label, Report, ReportKind, Source};
use std::collections::HashSet;

/// Names the host supplies to a script's outermost `{ ... } -> ...`.
const HOST_PROVIDED: &[&str] = &["builtin", "__native"];

pub struct ScopeError {
    pub span: Span,
    pub name: String,
    /// A close-by name that is in scope, for "did you mean".
    pub hint: Option<String>,
}

struct Checker {
    /// Innermost scope last.
    scopes: Vec<Vec<String>>,
    errors: Vec<ScopeError>,
}

impl Checker {
    fn is_bound(&self, name: &str) -> bool {
        self.scopes.iter().any(|s| s.iter().any(|n| n == name))
    }

    /// Names in scope, innermost first, each scope's names sorted and shadowed
    /// names listed once (same order as `Env::visible_names`).
    fn visible_names(&self) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut names = Vec::new();
        for scope in self.scopes.iter().rev() {
            let mut here = scope.clone();
            here.sort();
            for name in here {
                if seen.insert(name.clone()) {
                    names.push(name);
                }
            }
        }
        names
    }

    fn scoped<F: FnOnce(&mut Self)>(&mut self, names: Vec<String>, f: F) {
        self.scopes.push(names);
        f(self);
        self.scopes.pop();
    }

    fn walk(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Bool(_) | ExprKind::Path(_) => {}
            // `.name` reads an attribute of the `with` object; it is not a variable.
            ExprKind::ImplicitAccess(_) => {}
            ExprKind::String(parts) => {
                for part in parts {
                    if let StringPart::Interpolation(inner) = part {
                        self.walk(inner);
                    }
                }
            }
            ExprKind::List(items) => items.iter().for_each(|e| self.walk(e)),
            ExprKind::AttrSet { is_rec, attrs } => {
                if *is_rec {
                    // Every key is visible from every value.
                    let keys = attrs.iter().map(|(k, _)| k.clone()).collect();
                    self.scoped(keys, |c| attrs.iter().for_each(|(_, v)| c.walk(v)));
                } else {
                    // A plain attrset's values do not see their siblings.
                    attrs.iter().for_each(|(_, v)| self.walk(v));
                }
            }
            ExprKind::Ident(name) => {
                if !self.is_bound(name) {
                    let candidates = self.visible_names();
                    let hint = crate::eval::did_you_mean(name, candidates.iter()).cloned();
                    self.errors.push(ScopeError {
                        span: expr.span.clone(),
                        name: name.clone(),
                        hint,
                    });
                }
            }
            ExprKind::FieldAccess(lhs, _) => self.walk(lhs),
            ExprKind::Lambda(args, body) => {
                let names = match args {
                    Args::Positional(names) => names.clone(),
                    Args::Destructure { names, .. } => names.clone(),
                };
                self.scoped(names, |c| c.walk(body));
            }
            ExprKind::App(f, arg) => {
                self.walk(f);
                self.walk(arg);
            }
            ExprKind::LetIn(bindings, body) => {
                // `let` is recursive: every binding is visible from every binding.
                let names = bindings.iter().map(|(k, _)| k.clone()).collect();
                self.scoped(names, |c| {
                    bindings.iter().for_each(|(_, v)| c.walk(v));
                    c.walk(body);
                });
            }
            ExprKind::BinOp(lhs, _, rhs) => {
                self.walk(lhs);
                self.walk(rhs);
            }
            ExprKind::Neg(inner) | ExprKind::TypeAnnotation(inner, _) => self.walk(inner),
            ExprKind::With(obj, body) => {
                self.walk(obj);
                self.walk(body);
            }
            ExprKind::IfElse(cond, then_branch, else_branch) => {
                self.walk(cond);
                self.walk(then_branch);
                self.walk(else_branch);
            }
        }
    }
}

/// Finds every use of a variable that nothing binds.
///
/// If the script's outermost expression is itself a lambda, the host calls it
/// and binds only `builtin` / `__native` (whichever it names): its other
/// parameters, positional or destructured, stay undefined at run time, so a use
/// of one is reported. A lambda anywhere else is treated like any other, with
/// all of its parameters bound, so nothing that could work is rejected.
/// Modules pulled in with `import` are not followed.
pub fn check(root: &Expr) -> Vec<ScopeError> {
    let mut checker = Checker {
        scopes: Vec::new(),
        errors: Vec::new(),
    };
    match &root.kind {
        ExprKind::Lambda(args, body) => {
            let provided = match args {
                Args::Destructure { names, .. } => names
                    .iter()
                    .filter(|n| HOST_PROVIDED.contains(&n.as_str()))
                    .cloned()
                    .collect(),
                Args::Positional(_) => Vec::new(),
            };
            checker.scoped(provided, |c| c.walk(body));
        }
        _ => checker.walk(root),
    }
    let mut errors = checker.errors;
    errors.sort_by_key(|e| e.span.start);
    errors
}

/// Prints the errors as ariadne diagnostics against `source`.
pub fn report(filename: &str, source: &str, errors: &[ScopeError]) {
    for error in errors {
        let msg = format!("Variable '{}' not found", error.name);
        let mut builder = Report::build(
            ReportKind::Error,
            (filename.to_string(), error.span.clone()),
        )
        .with_message(&msg)
        .with_label(
            Label::new((filename.to_string(), error.span.clone()))
                .with_message(&msg)
                .with_color(Color::Red),
        );
        if let Some(hint) = &error.hint {
            builder = builder.with_label(
                Label::new((filename.to_string(), error.span.clone()))
                    .with_message(format!("did you mean '{}'?", hint))
                    .with_color(Color::Yellow),
            );
        }
        if error.name == "builtins" {
            builder = builder.with_note(
                "consider receiving 'builtin' as a function argument, e.g., `{ builtin } ->`",
            );
        }
        let _ = builder
            .finish()
            .eprint((filename.to_string(), Source::from(source)));
    }
}
