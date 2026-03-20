# Eris Language Interpreter

Eris（エリス）は、純粋に機能的で遅延評価（Lazy Evaluation）を特徴とする、動的型付けのドメイン特化型プログラミング言語です。このプロジェクトは Rust で実装された Eris 言語のツリーウォーク型インタプリタ（Tree-walk Interpreter）です。

## 特徴 (Features)

- **純粋関数型 & 遅延評価**:
  変数の評価は実際に値が必要になるまで遅延（Thunk 化）されます。これにより、定義の順序に依存しない柔軟な変数宣言が可能です。
- **データ構造**:
  - **リスト (Lists)**: 空白区切りのリスト `[ 1 2 3 ]`
  - **属性セット (Attribute Sets)**: レコードやオブジェクトに相当するキー・バリューの構造 `{ a = 1; b = 2; }`
  - **再帰的属性セット (`rec`)**: 自身のスコープを参照可能な属性セット `rec { a = b; b = 42; }`
- **リテラルと展開**:
  - 単一引用符・二重引用符文字列: `"Hello"`, `'World'`
  - パスリテラル: `p'./example.txt'`
  - 文字列補間: `let x = 42; in "x is ${x}"`
- **関数 (Lambdas)**:
  - 位置引数: `| a, b | -> a + b`
  - デストラクチャリング (分配代入): `{ x; y; ... } -> x + y`
  - カリー化や部分適用が可能です。
- **制御構文**:
  - `let ... in ...`: ローカル変数のバインディング
  - `with obj expr`: オブジェクトの暗黙的なスコープ展開（`.` 始まりでフィールドにアクセス）
  - `if ... then ... else ...`: 条件分岐と遅延評価
- **組み込み関数連携 (Builtins)**:
  エントリーポイントで `{ builtin } -> ...` のように外部機能（副作用など）を注入して実行することが可能です。

## コードの例 (Examples)

### Hello, World!
組み込み（`builtin`）の `fmt` モジュールを呼び出して出力する例：

```text
{ builtin } ->
  let 
    fmt = builtin.import 'fmt';
  in
    fmt.println "Hello, World!"
```

### `with` ブロックと暗黙的アクセス
`with` を使用し、オブジェクトのプロパティを暗黙的に参照（`.` から開始）できます。

```text
{ builtin } ->
  with builtin.import 'fmt'
    .println "Hello, World!"
```

### 比較演算・パターン・条件分岐 (`if` 式)
```text
{ builtin } ->
  let
    fmt = builtin.import 'fmt';
    
    config = rec {
      threshold = 100;
      value = threshold * 2;
    };
    
    msg = if config.value > 150 && true then
      "Value is large: ${config.value}"
    else
      "Value is small";
  in
    fmt.println msg
```

## インストールと実行 (Usage)

本プロジェクトは Rust (`cargo`) で構築されています。

### スクリプトの実行
```bash
cargo run -- <file.eris>
```

### 単体テストの実行
言語機能（再帰バインド、比較演算子、リスト処理など）を網羅した評価テストが含まれています。
```bash
cargo test
```

## 使用技術 (Dependencies)

- [Rust](https://www.rust-lang.org/) (2021 Edition)
- [chumsky](https://github.com/zesterer/chumsky) (1.0.0-alpha) - パーサコンビネータ
