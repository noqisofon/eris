use chumsky::prelude::*;

fn parser() -> impl Parser<char, Expr, Error = Simple<char>> {
    recursive(|expr| {
        let ident = text::ident().padded();

        // リテラル (数値、文字列、パス)
        let literal = todo!();

        // 属性集合 { x = 1; y = 2; }
        let attr_set = expr.clone()
            .separated_by(just(';'))
            .allow_trailing()
            .delimited_by(just('{'), just('}'))
            .map(|fields| Expr::AttrSet(fields, false));

        // ラムダ式 { x; y } -> expr
        let params = ident.clone()
            .separated_by(just(';'))
            .delimited_by(just('{'), just('}'));
        let lambda = params
            .then_ignore(just("->"))
            .then(expr.clone())
            .map(|(p, body)| Expr::Lambda(p, Box::new(body)));

        // 優先順位を考慮した演算子の組み立て...
        let term = literal.or(attr_set).or(ident.map(Expr::Symbol));
        
        // ... (ここから binary op をうにゃうにゃ繋げる)
        term
    })
}
