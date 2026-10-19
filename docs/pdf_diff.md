# PDF差分

[READMEへ戻る](../README.md)

`jade-pdf-diff` はページとテキストブロックを対応付けて比較するライブラリです。抽出と描画にはPdfium、対応付けには [jade-core](https://github.com/jade-rs/jade-core) の `jade-diff-align` / `jade-diff-support` を使用します。Rustからは `pdf_diff` として利用できます。

## CLI

リポジトリのルートで実行します。

```sh
# 比較結果をJSONで出力
cargo run -p jade-pdf-diff -- old.pdf new.pdf

# ページとブロックの中間表現を出力
cargo run -p jade-pdf-diff -- --ir input.pdf

# 1ページ目を幅800pxのPNGで保存
cargo run -p jade-pdf-diff -- --render input.pdf 1 800 page.png

# 1ページ目のテキストレイヤを出力
cargo run -p jade-pdf-diff -- --text-layer input.pdf 1
```

比較時の終了コードは、差分なしが `0`、差分ありが `1` です。引数が不足している場合は使い方を表示して `2` を返します。

## 検証

```sh
cargo clippy -p jade-pdf-diff --all-targets --locked -- -D warnings
cargo test -p jade-pdf-diff --locked
```

Pdfiumを使用するテストは、CIではmacOSで実行します。その他の検証手順は [開発ガイド](development.md) を参照してください。
