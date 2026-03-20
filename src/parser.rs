use crate::ast::*;
use chumsky::prelude::*;

pub fn parser<'src>() -> impl Parser<'src, &'src str, Expr, extra::Err<Rich<'src, char>>> {
    recursive(|expr| {
        let ident = text::ident().map(|s: &str| s.to_string()).padded();

        let float = text::int(10)
            .then_ignore(just('.'))
            .then(text::int(10))
            .map(|(a, b): (&str, &str)| Expr::Float(format!("{}.{}", a, b).parse().unwrap()))
            .padded();

        let int_val = text::int(10)
            .map(|s: &str| Expr::Int(s.parse().unwrap()))
            .padded();

        let interp = just("${")
            .ignore_then(ident.clone())
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
            .map(Expr::String)
            .padded();

        let path = just("p'")
            .ignore_then(
                none_of("'")
                    .repeated()
                    .to_slice()
                    .map(|s: &str| s.to_string()),
            )
            .then_ignore(just('\''))
            .map(Expr::Path)
            .padded();

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
                .repeated()
                .collect::<Vec<_>>()
                .delimited_by(just('[').padded(), just(']').padded())
                .map(Expr::List);

            let attr_binding = ident
                .clone()
                .then_ignore(just('=').padded())
                .then(expr.clone())
                .then_ignore(just(';').padded());

            let attr_set_body = attr_binding
                .repeated()
                .collect::<Vec<_>>()
                .delimited_by(just('{').padded(), just('}').padded());

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
                .then_ignore(just('=').padded())
                .then(expr.clone())
                .then_ignore(just(';').padded());

            let let_in = just("let")
                .padded()
                .ignore_then(let_binding.repeated().collect::<Vec<_>>())
                .then_ignore(just("in").padded())
                .then(expr.clone())
                .map(|(bindings, body)| Expr::LetIn(bindings, Box::new(body)));

            choice((float, int_val, string, path, list, attr_set)).or(choice((
                let_in,
                ident.clone().map(Expr::Ident),
                expr.clone()
                    .delimited_by(just('(').padded(), just(')').padded()),
            )))
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

        let app = field_access
            .clone()
            .then(field_access.repeated().collect::<Vec<_>>())
            .map(|(lhs, args)| {
                args.into_iter()
                    .fold(lhs, |acc, arg| Expr::App(Box::new(acc), Box::new(arg)))
            });

        let lambda_pos = ident
            .clone()
            .separated_by(just(',').padded())
            .allow_trailing()
            .collect::<Vec<_>>()
            .delimited_by(just('|').padded(), just('|').padded())
            .map(Args::Positional);

        let destructure_args = just('{')
            .padded()
            .ignore_then(
                ident
                    .clone()
                    .separated_by(just(';').padded())
                    .allow_trailing()
                    .collect::<Vec<_>>(),
            )
            .then_ignore(just(';').padded().or_not())
            .then(just("...").padded().or_not())
            .then_ignore(just('}').padded())
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

        lambda.or(sum).padded()
    })
}
