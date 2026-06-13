# Eris Language Interpreter

Eris（エリス）は、純粋に機能的で遅延評価（Lazy Evaluation）を特徴とする、動的型付けのドメイン特化型プログラミング言語です。このプロジェクトは Rust で実装された Eris 言語のツリーウォーク型インタプリタ（Tree-walk Interpreter）です。

## 特徴 (Features)

- **純粋関数型 & 遅延評価**:
  変数の評価は実際に値が必要になるまで遅延（Thunk 化）されます。これにより、定義の順序に依存しない柔軟な変数宣言が可能です。
- **ケバブケースの変数名・属性名**:
  変数名や属性名にハイフン（ケバブケース）を使用することができます（例: `let write-file = 42; in write-file`）。
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
- **組み込み連携 & 標準ライブラリ (Preloaded Modules)**:
  エントリーポイントで `{ builtin } -> ...` のように外部機能を注入し、事前ロードされた豊富な標準ライブラリ（後述）を `builtin.import` でインポートできます。
- **高度なエラーレポートと Typo 検出 (Did you mean?)**:
  `ariadne` クレートによる美しいエラー表示に加え、変数名・オブジェクトのフィールド名・標準ライブラリのモジュール名で存在しないものが指定された際、**レーベンシュタイン距離を用いた自動 Typo 検出と修正候補の提案**を行います。また、未定義の `builtins` が呼び出された場合は自動で引数受け取りを促す親切なノートを表示します。

---

## 標準ライブラリ (Standard Library Modules)

Eris では、事前ロードされた以下の標準モジュールを `builtin.import` 経由で利用可能です。

| モジュール名 | 主な提供関数と機能 |
| :--- | :--- |
| **`fmt`** | 標準・エラー出力 (`println`, `eprintln`, `print`, `eprint`, `printf`, `eprintf`) |
| **`list`** | リスト操作と高階関数 (`map`, `filter`, `foldl`, `head`, `tail`) |
| **`string`** | 文字列操作とユーティリティ (`concat`, `split`, `trim`, `interpolate`) |
| **`attr`** | 属性セット操作 (`names`, `values`, `has`, `get`, `merge`) |
| **`fs`** | ファイルシステムの操作 (`read_file`, `read_dir`, `write_file`) |
| **`json`** | JSONデータのパースとシリアライズ (`from`, `to`) |
| **`path`** | パス文字列の操作と合成 (`join`, `dirname`, `basename`, `extname`) |
| **`child_process`** | 外部コマンドの実行と結果取得 (`exec`) |
| **`hash`** | 暗号学的ハッシュ関数の適用 (`sha256`, `sha512`, `blake3`, `content`) |
| **`build`** | Nix 風の derivation ビルド (`derivation`, `store_dir`)。入力のハッシュに基づくキャッシュ付きビルド |

---

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

### 標準モジュール `list` と関数適用の組み合わせ
リストの要素を2倍にする高階関数の使用例：

```text
{ builtin } ->
  let
    fmt = builtin.import 'fmt';
    list = builtin.import 'list';
    doubled = list.map (|x| -> x * 2) [ 1 2 3 ];
  in
    fmt.println "doubled: ${doubled}"
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

### `build` モジュールによる derivation ビルド
Nix の derivation に相当するキャッシュ付きビルドを実行できます。derivation の属性セット全体がハッシュ化され、ストアディレクトリ（デフォルト: `./eris-store`、環境変数 `ERIS_STORE` で変更可能）に `<hash>-<name>` として出力されます。同じ入力なら 2 回目以降はビルドをスキップしてキャッシュを返します。

ビルダーには `out`（出力先パス）と、`args` を除く derivation の全属性が環境変数として渡されます。ビルダーは `$out`（Windows では `%out%`）にファイルまたはディレクトリを作成する必要があります。

```text
{ builtin } ->
  let
    fmt = builtin.import "fmt";
    build = builtin.import "build";

    drv = build.derivation {
      name = "hello-txt";
      version = "1.0.0";
      builder = "cmd";
      args = [ "/C" "echo hello from eris build> %out%" ];
    };

    out = drv.out;
    cached = if drv.cached then "yes" else "no";
  in
    [
      (fmt.println "out: ${out}")
      (fmt.println "cached: ${cached}")
    ]
```

戻り値は `{ out; name; hash; cached; }` の属性セットです。ビルドが失敗（終了コード非 0、または `$out` 未作成）した場合はストアに何も残さずエラーになります。

---

## インストールと実行 (Usage)

本プロジェクトは Rust (`cargo`) で構築されています。

### スクリプトの実行
```bash
cargo run -- <file.eris>
```

### 単体テストの実行
言語機能（再帰バインド、比較演算子、リスト処理、ケバブケースなど）を網羅した評価テストが含まれています。
```bash
cargo test
```

---

## 使用技術と依存関係 (Dependencies)

- [Rust](https://www.rust-lang.org/) (2024 Edition)
- [chumsky](https://github.com/zesterer/chumsky) (0.12.0) - 高度なパーサコンビネータ
- [ariadne](https://github.com/zesterer/ariadne) (0.6.0) - エラー表示ユーティリティ
- [serde_json](https://github.com/serde-rs/json) (1.0) - JSON形式とのシリアライズ・デシリアライズ
- [sha2](https://github.com/RustCrypto/hashes) (0.10.9) - SHA-256 / SHA-512 暗号ハッシュ
- [blake3](https://github.com/BLAKE3-team/BLAKE3) (1.8.3) - 高速ハッシュ
- [base64](https://github.com/marshallpierce/rust-base64) (0.22.1) - Base64 エンコーダ
