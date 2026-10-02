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
  - 二重引用符文字列: `"Hello"`。`\n` `\t` `\r` `\\` `\"` `\$` のエスケープが使えます。`\$` は、補間にならない `$` を書くためのものです（例: `"cost \$5"`、`"literal \${x}"`）。それ以外の `\` + 文字（`\q` など）は構文エラーになります。Windows のパスなどバックスラッシュを多く含むテキストは `"C:\\Users\\me"` と書くか、次の単一引用符文字列を使ってください。
  - 単一引用符文字列: `'World'`。エスケープも補間もない生の文字列です（`'C:\Users'`、`'a\nb'` は書いたとおりの文字になります）。
  - パスリテラル: `p'./example.txt'`
  - 文字列補間: `let x = 42; in "x is ${x}"`
- **算術と単項マイナス**:
  `+ - * /` と、単項マイナス `-x`、`-(a + b)`、`-f 2`（`-(f 2)` として読まれます）。単項マイナスは `*` `/` より強く、関数適用より弱く結合します。リストの要素や関数の引数に負の数を書くときは `[ 1 (-2) ]` のように括弧で囲んでください（`[ 1 -2 ]` や `f -2` は引き算として読まれます）。最小の整数 `-9223372036854775808` はリテラルとして書けません（`0 - 9223372036854775807 - 1` と書いてください）。
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
- **`::` によるランタイム型ヒント**:
  式の末尾に `expr :: type_name` を付けると、その式が評価された際に実際の値が指定した型と一致するかをチェックします（例: `(1 + 2) :: int`）。`int` / `float` / `bool` / `string` / `path` / `list` / `attrset` / `closure` とそのエイリアス（`str`, `bool`→`boolean`, `fn`→`function` など）に対応し、`eris check --level type` で実行前に一括検査できます。

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

**ハッシュに含まれるもの**: derivation の属性値に加えて、`builder` の実体（ファイルの中身）、属性に書かれた `path` 型の値（`args` の中も含む）の中身、ビルダーの既定環境（下記）がハッシュに入ります。`path` がファイルなら中身（と実行権限の有無）を、ディレクトリなら名前順に再帰したツリー全体を使います。シンボリックリンクは辿って実体の中身をハッシュします（`builder` が `/bin/sh` のようなリンクでも同じです）。リンク切れとリンクの循環はエラーです。そのため `args = [ p'./build.sh' ]` のスクリプトを書き換えると、ハッシュが変わって再ビルドされます。文字列は値そのものだけがハッシュ対象で、文字列が指すファイルの中身は見ません（中身を追跡したいファイルは `p'...'` で書いてください）。

**ビルダーの環境**: ビルダーは空の環境から始まり、評価を行ったシェルの環境変数（`HOME` など）は一切引き継ぎません。渡されるのは次のものだけです。

- 既定環境: Unix では `PATH=/usr/bin:/bin`、Windows では `SystemRoot` と `PATH`（`%SystemRoot%\System32;%SystemRoot%`）。同名の属性（例: `PATH = "..."`）があればそちらが優先されます。
- `args` を除く、derivation の宣言された全属性（環境変数名として使えない名前はエラー）
- `out`: 出力先の**絶対パス**。ビルダーは `$out`（Windows では `%out%`）にファイルまたはディレクトリを作成する必要があります。`out` という名前の属性はエラーです。

`builder` に `/` や `\` を含まない名前（`sh` や `cmd` など）を書いた場合は、ビルダーが見る `PATH` から探します。見つからなければエラーです。

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

戻り値は `{ out; name; hash; cached; }` の属性セットです。ハッシュの算出方法を変更したため、以前のバージョンで作った `eris-store` のキャッシュは再利用されません（再ビルドされるだけで、削除は不要です）。ビルドが失敗（終了コード非 0、または `$out` 未作成）した場合はストアに何も残さずエラーになります。

---

## インストールと実行 (Usage)

本プロジェクトは Rust (`cargo`) で構築されています。

### スクリプトの実行
`eris run <file>` サブコマンド、または省略形として直接ファイルパスを渡す形式のどちらでも実行できます。

```bash
cargo run -- run <file.eris>
# もしくは
cargo run -- <file.eris>
```

### 構文・型のチェック
`eris check` サブコマンドには 2 つのレベルがあります。

- `--level syntax`（デフォルト）: スクリプトを**実行せず**、構文エラーの有無だけを確認します。
- `--level type`: スクリプトを**実際に評価して**、式に付与された `:: type_name` の型ヒントがすべて満たされているかを検査します。

```bash
cargo run -- check <file.eris>
cargo run -- check --level type <file.eris>
```

`--level type` が検査する範囲は次のとおりです。

- 結果として評価される値に加えて、**使われなかった値も強制評価して**検査します（未使用の `let` 束縛、未使用の属性・リスト要素、未使用の関数引数など）。`run` は遅延評価なのでこれらを評価しませんが、`check` は評価して `::` を確かめます。
- 使われなかった値を評価しているときに出た**型注釈の不一致だけ**を失敗として数えます。そこで起きた `1 / 0` や未定義変数のような他のエラーは、型の検査の対象外なので無視されます（例: `let x = 1 / 0; in 0` は `check` を通ります）。結果を求める通常の評価で起きたエラーは、従来どおり失敗です。
- **評価の経路に乗らないコードは検査されません**。通らない `if` の分岐や、どこからも呼ばれない関数の本体は、`::` が書かれていても見逃します（`:: type` は「評価された値が実際にその型か」を確かめる実行時の表明であり、静的な型推論ではないためです）。
- 使われなかった値の強制評価は、最大 100 万個で打ち切ります（遅延的に作られる無限の構造対策）。打ち切ると `note:` を表示し、残りは検査しません。このとき成功の行は `OK (incomplete: stopped after 1000000 values): ...` となり、検査が不完全だったことが分かります（終了コードは 0 のままです）。

> **注意**: `--level type` はスクリプトを実行します。使われなかった束縛も強制評価するので、`run` では実行されない束縛の中の副作用（ファイル書き込み、`child_process`、`build.derivation` など）も実行されます。信頼できないスクリプトに対して使わないでください。ただし `abort` だけは例外で、使われなかった束縛の中の `abort` は、`check` の強制評価では実行されません（実際の評価の経路で呼ばれた `abort` は、`run` と同じく終了コードを返して終了します）。

### 単体テストの実行
言語機能（再帰バインド、比較演算子、リスト処理、ケバブケースなど）を網羅した評価テストが含まれています。
```bash
cargo test
```

---

## 制限事項 (Limitations)

- **再帰の深さ**: Eris には末尾呼び出し最適化がありません。評価のネストが 10,000 段（関数呼び出しにして約 3,300 回。式の形によって前後します）を超えると、`Recursion limit exceeded` を報告して Poison を返します（終了コード 1）。`ulimit -v` などでアドレス空間が制限されている環境では、この上限は自動的に下がります。
- **整数**: 整数は 64bit 符号付きです。範囲外のリテラルは構文エラー、`+ - * /` のオーバーフローは `Integer overflow` として報告されます。
- **ビルドの信頼境界**: `build.derivation` は環境を整えて再現性を高めますが、サンドボックスではありません。ビルダーは評価を行ったユーザーの権限でそのまま実行され、ファイルシステムやネットワークにもアクセスできます（`fs`・`child_process` モジュールも同様です）。信頼できないスクリプトは実行しないでください。
- **深いデータ構造**: リスト・属性セットのネストが再帰の上限（上記）を超えると、評価結果の展開（`deep_force`）や `hash` / `json.to` / `build.derivation` で `Recursion limit exceeded: value is nested too deeply` を報告します（終了コード 1）。値の解放は反復的に行うので、深いデータでもスタックは溢れません。

## 使用技術と依存関係 (Dependencies)

- [Rust](https://www.rust-lang.org/) (2024 Edition)
- [chumsky](https://github.com/zesterer/chumsky) (0.12.0) - 高度なパーサコンビネータ
- [ariadne](https://github.com/zesterer/ariadne) (0.6.0) - エラー表示ユーティリティ
- [clap](https://github.com/clap-rs/clap) (4) - `run` / `check` サブコマンドを備えた CLI 引数パーサ（derive マクロ使用）
- [serde_json](https://github.com/serde-rs/json) (1.0) - JSON形式とのシリアライズ・デシリアライズ
- [sha2](https://github.com/RustCrypto/hashes) (0.10.9) - SHA-256 / SHA-512 暗号ハッシュ
- [blake3](https://github.com/BLAKE3-team/BLAKE3) (1.8.3) - 高速ハッシュ
- [base64](https://github.com/marshallpierce/rust-base64) (0.22.1) - Base64 エンコーダ
