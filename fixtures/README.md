# 公開PDFのE2E fixture

[py-pdf/sample-files](https://github.com/py-pdf/sample-files) のcontributorsが公開する、ダミーテキストのPDFを3件収録する。取得日: 2026-10-19。すべての出典URLはコミット `02b8203bf416db3dbdb03438bc8278dfcb0555f3` に固定している。

| ケース | 生成元 | ページ数 |
| --- | --- | --- |
| pdflatex-text | pdfLaTeX | 1 |
| libreoffice-text | LibreOffice Writer | 1 |
| pdflatex-four-pages | pdfLaTeX | 4 |

PDFの本文・メタデータ・全6ページの画像を確認し、個人情報・機密情報に該当する内容は見つからなかった。保存済み入力はmanifestに登録した公開PDFに限定する。

## 出典・ライセンス

PDFはupstreamから変更せず取得した。著作者表示は **py-pdf/sample-files contributors**。upstreamの[README](https://github.com/py-pdf/sample-files/tree/02b8203bf416db3dbdb03438bc8278dfcb0555f3)はすべてのPDFを **CC BY-SA 4.0** と明示し、投稿を自作資料に限定している。[ライセンス全文](LICENSE-CC-BY-SA-4.0.txt)を同梱する。PDFとその本文を抽出したexpectedテキストの再配布・改変には、出典・著作者・ライセンスの表示、変更の明示、継承条件が適用される。プログラム本体のライセンスとは区別する。

各PDFの固定URL、SHA-256、改変なしの表示、およびexpectedのSHA-256は [manifest.json](manifest.json) に記録した。

## 検証・更新

```sh
cargo test -p jade-pdf2docx --test public_pdf_e2e --locked
python3 -m unittest discover -s tests -p test_fixture_hygiene.py
```

E2Eは実CLIで一時DOCXを生成し、ZIP内の必須部品、ページ境界、全ページの本文と入力PDFの不変性を検証する。expectedは変換器から取得せず、独立したPoppler `pdftotext 26.10.0 -layout` で抽出した。比較時は改行・空白と期待本文の行末折返しハイフンを除去し、本文の文字・句読点・順序を照合する。単一ページのpdfLaTeX文書にある `taki-` / `mata` の折返しは、公開元LaTeXでも単語 `takimata` であることを確認した。DOCXを保存fixtureとして追加しない。テスト実行時のネットワークアクセスやPopplerは不要。

更新時は出典・ライセンス・本文・メタデータ・全ページの画像を確認し、manifestの固定コミットとハッシュを更新する。expectedの再抽出例:

```sh
pdftotext -layout fixtures/input/pdflatex-text.pdf fixtures/expected/pdflatex-text.txt
```

この3件はテキスト文書の変換確認用。OCR・画像・表・視覚的なレイアウトの精度評価は対象にしない。
