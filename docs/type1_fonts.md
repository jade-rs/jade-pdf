# Type1 font の対応範囲と検証

PDF の Type1 `/FontFile` に含まれる対応済みの静的描画プログラムを、pure Rust で OpenType/CFF に変換し、DOCX に埋め込みます。`parse_pdf_bytes` と bytes/WASM 変換で利用でき、native のファイル変換でも文字照合が成立するページでは選択した font を保持します。新しい CLI オプションや外部 font インストールは不要です。

元の PDF code、Encoding/Differences、ToUnicode、Widths を対応付けます。同じ source glyph に異なる幅が指定されている場合や、複数 Unicode 文字に対応する ligature では、必要な face を分けます。通常の `f` の連続を元の `ff` glyph に誤変換しないよう、ligature 用 face の適用範囲を元の描画 code に限定します。`Tc`、`Tw`、`Tz`、`TJ` の位置計算は元の code 幅に従います。Form 内の同名 resource は、ページ側の同名 font と区別します。

変換した family 名は `JadeT1-` で始まる SHA-256 由来の識別名です。元 program、幅、文字対応、属性、権利表示、実際の glyph 出力を識別に含め、元の予約 family 名を再利用しません。PDF の名前の部分文字列から bold/italic を推定せず、生成 SFNT の属性を DOCX run と埋め込み slot に反映します。

## 権利と制限

変換前に元 program の FSType と確認可能なライセンス表示を検査します。対応する OFL 1.1 表示の場合は元の著作権・notice と標準ライセンス本文を保持します。埋め込みが許されない場合や権利根拠を確認できない場合は、この変換経路では font を生成しません。no-subsetting 属性を保持し、元 program 内の全 glyph を出力へ残します。これは元の Type1 暗号文や hint 命令を同一 byte のまま保存することを意味しません。

対応は、検証済みの binary eexec、静的 CharStrings/Subrs、標準的な FontMatrix などに限定します。任意の PostScript、任意の FontMatrix、あらゆる複合文字や shaping を実装したわけではありません。元 glyph の Unicode が未確認なら、別の Unicode や PUA を推測して割り当てません。既存の `unverified_encoding` 診断と strict の拒否は維持します。

中間 font、文字属性、resource cache、埋め込み byte 数は既存の解析予算で制限します。未使用 resource の face は公開結果へ含めず、最終結果の埋め込み font 数上限は既存の32件です。native の文字数不一致や Form の抽出順不一致はページ単位で SDK 結果へ戻します。生成 cmap と異なる Unicode に置き換わる場合も、そのページへ元の生成 font を引き継ぎません。

## 検証と残っている問題

所有するメモリ内 PDF による Red/Green テストで、混在した単文字・複数文字 code、literal/hex/octal、`TJ` と文字間隔、同名 resource の異なる幅、Form の局所 scope、文書間 cache reset、未使用 font、native のページ単位 fallback を検証しています。font metadata の境界、style flags、ページ単位の不変条件には property test もあります。

公開資料の回帰検証には [GNU Hello manual](https://www.gnu.org/software/hello/manual/hello.pdf) を使用しました。原本 SHA-256 は `3c5212427682a49771d653a5f46ae2e69bdee33b4c0cf428d6ec672ab99d5e3b`、17ページです。原本の GFDL 1.3+ 表示と埋め込み Computer Modern font の OFL 表示を確認し、変換前に全ページを確認しています。原本と出力は Git に追加していません。この資料は改善に用いた既知の回帰資料であり、未見の holdout ではありません。

11個の元 font から15 face を生成し、独立した fontTools/HarfBuzz 検査で339個の確認済み source code を機能の有効・無効の両方で照合しました。678件の照合は font shaping の検査数であり、PDF 件数ではありません。純 Rust の実解析では861要素のうち859要素が生成 face を参照します。native の実解析では843要素のうち794要素が生成 face を参照し、一ページの文字数不一致は他ページの選択を失わせません。

実際の native CLI 出力には15 face が入り、同梱 LibreOffice で描画した PDF にも生成 family が現れました。しかし、17ページの原本が26ページに増え、索引の配置も崩れています。Unicode 未確認の `circlecopyrt`、native の一ページの文字数差、空白 glyph の代替、折り返しとページ送りは未解決です。Microsoft Word での描画は未検証です。font の描画・幅・Unicode の部品検査が通っても、文書全体の本番品質を保証しません。

SFNT 属性は [OpenType OS/2](https://learn.microsoft.com/en-us/typography/opentype/spec/os2) と [head](https://learn.microsoft.com/en-us/typography/opentype/spec/head)、DOCX の face slot は [Microsoft の font 実装注記](https://learn.microsoft.com/en-us/openspecs/office_standards/ms-oi29500/09b83e15-30de-481d-8ddf-777107d4122e) を参照しています。
