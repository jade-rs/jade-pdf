# 入力内容の照合

```sh
input_digest=$(shasum -a 256 input.pdf | cut -d " " -f 1)
jade-pdf2docx input.pdf output.docx --input-sha256 "$input_digest"
```

指定値は呼出元が対象PDFから計算したSHA-256（64桁の小文字hex）です。上の例は手元の入力から計算します。大文字・桁数不正・重複指定・値なしはusage error（2）とし、変換を開始しません。通常CLI、systemd wrapper、pool受付のCLI引数として利用できます。

workerは原本を一度だけ開き、専用snapshotへ固定8 KiB bufferでコピーしながらSHA-256を計算します。parserの入力上限（通常CLIは128 MiB）を適用し、超過確認も上限+1 byteで止めます。FIFOはwriter待ちに入らず拒否します。通常ファイルへのsymlinkはリンク先を開いて照合します。snapshotの完成後に原本のpathnameを置換・削除・書換えしても、その原本を解析用に開き直しません。コピー中に原本が変わった場合も、コピーされたbyte全体が期待値と一致した場合だけ処理します。

不一致・上限超過・I/O障害は変換失敗（1）です。既存出力を公開前に置換せず、原本と同じ出力（別名・hardlink・symlinkを含む）もコピー前に拒否します。照合とコピーは監督対象のworkerで実行するので、既存のtimeout・メモリ上限・中断/回収経路が適用されます。scratchの物理disk容量は予約しません。OCRは照合済みsnapshotから既存OCR用snapshotを作るため、入力コピー二つ分の一時diskが必要です。通常終了・失敗時は回収し、workerの強制終了時は監督が登録済み領域を回収します。

このオプションは入力byteの照合です。jobの重複起動はまだ防ぎません。同じdigestを持つ呼出を何度行っても変換します。永続request keyと要求identityの照合、未確定な起動の扱い、永続キュー、出力公開とのtransactionは次の実装対象です。実行バイナリ・OCRモデル・文字セット・外部runtimeの内容も、この入力digestには含みません。SHA-256の計算元を含む呼出元と同じ実行ユーザーを信頼する運用が前提です。
