use crate::ast::*;
use chumsky::prelude::*;

pub fn parser<'src>() -> impl Parser<'src, &'src str, Expr, extra::Err<Rich<'src, char>>> {
    let comment = text::inline_whitespace()
        .then(just('#'))
        .then(none_of('\n').repeated())
        .padded();

    recursive(|expr| {
        let ident = any().filter(|c: &char| c.is_alphabetic() || *c == '_')
            .then(any().filter(|c: &char| c.is_alphanumeric() || *c == '_' || *c == '-').repeated().collect::<String>())
            .map(|(first, rest)| format!("{}{}", first, rest))
            .filter(|s: &String| {
                !matches!(
                    s.as_str(),
                    "if" | "then" | "else" | "let" | "in" | "with" | "true" | "false" | "rec"
                )
            });

        let float = text::int(10)
            .then_ignore(just('.'))
            .then(text::int(10))
            .map(|(a, b): (&str, &str)| Expr::Float(format!("{}.{}", a, b).parse().unwrap()));

        let bool_val = choice((
            text::keyword("true").to(Expr::Bool(true)),
            text::keyword("false").to(Expr::Bool(false)),
        ));

        let int_val = text::int(10).map(|s: &str| Expr::Int(s.parse().unwrap()));

        let interp = just("${")
            .ignore_then(ident.clone().padded())
            .then_ignore(just("}"))
            .map(StringPart::Interpolation);

        let literal_chars = choice((
            none_of("$\"")
                .repeated()
                .at_least(1)
                .to_slice()
                .map(|s: &str| s.to_string()),
            just('$')
                .then_ignore(just('{').not())
                .map(|_| "$".to_string()),
        ))
        .map(StringPart::Literal);

        let string_part = interp.or(literal_chars);
        let string = just('"')
            .ignore_then(string_part.repeated().collect::<Vec<_>>())
            .then_ignore(just('"'))
            .map(Expr::String);

        let sq_string = just('\'')
            .ignore_then(
                none_of("'")
                    .repeated()
                    .to_slice()
                    .map(|s: &str| vec![StringPart::Literal(s.to_string())]),
            )
            .then_ignore(just('\''))
            .map(Expr::String);

        let path = just("p'")
            .ignore_then(
                none_of("'")
                    .repeated()
                    .to_slice()
                    .map(|s: &str| s.to_string()),
            )
            .then_ignore(just('\''))
            .map(Expr::Path);

        let atom = recursive(|atom| {
            let field_access = atom
                .clone()
                .then(
                    just('.')
                        .ignore_then(ident.clone())
                        .repeated()
                        .collect::<Vec<_>>(),
                )
                .map(|(lhs, fields)| {
                    if fields.is_empty() {
                        lhs
                    } else {
                        Expr::FieldAccess(Box::new(lhs), fields)
                    }
                });

            let list = field_access
                .clone()
                .padded()
                .repeated()
                .collect::<Vec<_>>()
                .delimited_by(just('[').padded(), just(']'))
                .map(Expr::List);

            let attr_binding = ident
                .clone()
                .padded()
                .then_ignore(just('=').padded())
                .then(expr.clone())
                .then_ignore(just(';').padded());

            let attr_set_body = attr_binding
                .repeated()
                .collect::<Vec<_>>()
                .delimited_by(just('{').padded(), just('}'));

            let attr_set =
                just("rec")
                    .padded()
                    .or_not()
                    .then(attr_set_body)
                    .map(|(rec_kw, attrs)| Expr::AttrSet {
                        is_rec: rec_kw.is_some(),
                        attrs,
                    });

            let let_binding = ident
                .clone()
                .padded()
                .then_ignore(just('=').padded())
                .then(expr.clone())
                .then_ignore(just(';').padded());

            let let_in = just("let")
                .padded()
                .ignore_then(let_binding.repeated().collect::<Vec<_>>())
                .then_ignore(just("in").padded())
                .then(expr.clone())
                .map(|(bindings, body)| Expr::LetIn(bindings, Box::new(body)));

            let with_expr = just("with")
                .padded()
                .ignore_then(expr.clone())
                .then(expr.clone())
                .map(|(obj, body)| Expr::With(Box::new(obj), Box::new(body)));

            let if_expr = just("if")
                .padded()
                .ignore_then(expr.clone())
                .then_ignore(just("then").padded())
                .then(expr.clone())
                .then_ignore(just("else").padded())
                .then(expr.clone())
                .map(|((cond, t), f)| Expr::IfElse(Box::new(cond), Box::new(t), Box::new(f)));

            choice((
                float, int_val, string, sq_string, path, bool_val, list, attr_set,
            ))
            .or(choice((
                let_in,
                with_expr,
                if_expr,
                ident.clone().map(Expr::Ident),
                expr.clone()
                    .delimited_by(just('(').padded(), just(')')),
            )))
        });

        let implicit_access = just('.')
            .ignore_then(ident.clone())
            .then(
                just('.')
                    .ignore_then(ident.clone())
                    .repeated()
                    .collect::<Vec<_>>(),
            )
            .map(|(first, mut rest)| {
                let mut path = vec![first];
                path.append(&mut rest);
                Expr::ImplicitAccess(path)
            });

        let field_access = atom
            .then(
                just('.')
                    .ignore_then(ident.clone())
                    .repeated()
                    .collect::<Vec<_>>(),
            )
            .map(|(lhs, fields)| {
                if fields.is_empty() {
                    lhs
                } else {
                    Expr::FieldAccess(Box::new(lhs), fields)
                }
            });

        let app_lhs = field_access.clone().or(implicit_access.clone());
        let app_arg = field_access.clone(); // Restrict arguments to prevent eating implicit accesses of the with body!

        let app = app_lhs
            .padded()
            .then(app_arg.padded().repeated().collect::<Vec<_>>())
            .map(|(lhs, args)| {
                args.into_iter()
                    .fold(lhs, |acc, arg| Expr::App(Box::new(acc), Box::new(arg)))
            });

        let lambda_pos = ident
            .clone()
            .padded()
            .separated_by(just(',').padded())
            .allow_trailing()
            .collect::<Vec<_>>()
            .delimited_by(just('|').padded(), just('|'))
            .map(Args::Positional);

        let destructure_args = just('{')
            .padded()
            .ignore_then(
                ident
                    .clone()
                    .padded()
                    .separated_by(just(';').padded())
                    .allow_trailing()
                    .collect::<Vec<_>>(),
            )
            .then_ignore(just(';').padded().or_not())
            .then(just("...").padded().or_not())
            .then_ignore(just('}'))
            .map(|(names, rest)| Args::Destructure {
                names,
                ignore_rest: rest.is_some(),
            });

        let lambda_args = lambda_pos.or(destructure_args);
        let lambda = lambda_args
            .then_ignore(just("->").padded())
            .then(expr.clone())
            .map(|(args, body)| Expr::Lambda(args, Box::new(body)));

        let op_mul_div = choice((just('*').to(Op::Mul), just('/').to(Op::Div)));

        let product = app
            .clone()
            .then(op_mul_div.padded().then(app).repeated().collect::<Vec<_>>())
            .map(|(lhs, rhs)| {
                rhs.into_iter().fold(lhs, |acc, (op, arg)| {
                    Expr::BinOp(Box::new(acc), op, Box::new(arg))
                })
            });

        let op_add_sub = choice((just('+').to(Op::Add), just('-').to(Op::Sub)));

        let sum = product
            .clone()
            .then(
                op_add_sub
                    .padded()
                    .then(product)
                    .repeated()
                    .collect::<Vec<_>>(),
            )
            .map(|(lhs, rhs)| {
                rhs.into_iter().fold(lhs, |acc, (op, arg)| {
                    Expr::BinOp(Box::new(acc), op, Box::new(arg))
                })
            });

        let comp_op = choice((
            just("==").to(Op::Eq),
            just("!=").to(Op::Neq),
            just("<=").to(Op::Lte),
            just(">=").to(Op::Gte),
            just('<').to(Op::Lt),
            just('>').to(Op::Gt),
        ));

        let comp = sum
            .clone()
            .then(
                comp_op
                    .padded()
                    .then(sum.clone())
                    .repeated()
                    .collect::<Vec<_>>(),
            )
            .map(|(lhs, rhs)| {
                rhs.into_iter().fold(lhs, |acc, (op, arg)| {
                    Expr::BinOp(Box::new(acc), op, Box::new(arg))
                })
            });

        let logical_and = comp
            .clone()
            .then(
                just("&&")
                    .to(Op::And)
                    .padded()
                    .then(comp.clone())
                    .repeated()
                    .collect::<Vec<_>>(),
            )
            .map(|(lhs, rhs)| {
                rhs.into_iter().fold(lhs, |acc, (op, arg)| {
                    Expr::BinOp(Box::new(acc), op, Box::new(arg))
                })
            });

        let logical_or = logical_and
            .clone()
            .then(
                just("||")
                    .to(Op::Or)
                    .padded()
                    .then(logical_and.clone())
                    .repeated()
                    .collect::<Vec<_>>(),
            )
            .map(|(lhs, rhs)| {
                rhs.into_iter().fold(lhs, |acc, (op, arg)| {
                    Expr::BinOp(Box::new(acc), op, Box::new(arg))
                })
            });

        lambda.or(logical_or).padded()
    })
    .padded_by(comment.repeated())
}
