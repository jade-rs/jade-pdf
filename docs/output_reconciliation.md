# 公開済みDOCXと要求の照合

この版の[キー付き受付](durable_requests.md)は、要求キーと保存したfingerprintをDOCXのZIPコメントへ含めます。親CLIが私有候補へ書き込んで同期した後、最終pathへrenameします。公開直後に親がSIGKILLされ、終了記録が欠けても、現在読める成果物との対応を調べられます。キューのdispatchも同じ受付を使います。受付・CLI・終了フックは同じ版を配布してください。

```sh
sudo python3 scripts/reconcile_jade_pdf2docx_output.py /var/lib/jade-pdf2docx/jobs \
  0123456789abcdef0123456789abcdef /data/output.docx
```

対象は一つのschema 2要求と、運用側が指定した絶対出力pathです。schema 2の履歴には出力path自体を保存していないため、このコマンドはpathと要求引数の対応を検証しません。キューrecordや呼出元の要求記録から正しいpathを指定してください。履歴と出力を読み、元の履歴JSONへ`publication`と`inspected_output`を加えて返します。正常な観測は終了コード0です。

| publication | 観測結果 |
| --- | --- |
| `matched` | 読み取ったZIPコメントが要求キー・fingerprintと一致 |
| `unmatched` | 通常ZIPとして読める末尾に一致する照合情報がない |
| `absent` | 指定pathまたは親directoryが存在しない |

終了結果が欠けた場合の`state`/`output`は`unknown`のままです。失敗記録があれば`failed`を保持します。`matched`でも成功記録へ書き換えず、変換を起動・再送せず、要求キーを解放しません。managerへ問い合わせないoffline照会なので、生存確認は別途[履歴status](systemd_job_history.md)を使います。

照合情報は`JADE-PDF2DOCX-PUBLICATION-1\nTOKEN:REQUEST_SHA256`です。通常のZIPコメントを置換し、DOCX本文entryは変更しません。キーなし受付・旧版の成果物には自動付与されません。CLIへの直接指定は`--publication-receipt TOKEN:REQUEST_SHA256`で、`--input-sha256`も必要です。キー付き受付は自分でこのflagを付け、呼出元の指定は拒否します。追加flagはfingerprintの算出に含めず、元の要求identityを保持します。

readerは全path階層のsymlinkと特殊fileを拒否し、最大65,557 byteの末尾だけを読みます。読取後に絶対pathを全階層で開き直し、元のinode・長さ・変更時刻と比較します。読取中の書換や親directory/出力fileの置換を検出した場合、壊れた末尾、未対応のZIP64/分割ZIP、schema 1要求、不正pathは終了コード2です。ZIP全体を展開せず、CRCや本文の完全性は検証しません。最後の照合後に起きるpathの変更は検出できません。

照合情報は認証・署名ではありません。コメントを保持した複製や改変も一致し得ます。`matched`は内容の正しさや現在も公開済みであること、実電源断後の耐久性を保証しません。運用側で元要求、manager、入力、DOCX本文を確認し、未確定な要求を別キーで機械的に再送しないでください。出力公開と終了記録を一つにするtransactionは残課題です。

検証は実PDFのDOCX変換、本文entryの保持、実公開関数のrename後SIGKILL、既存出力の保持、最大長コメント、失敗/unknown履歴の保持を含みます。Linuxの実systemd受入にもCLIとPython readerの照合を含めます。実ホスト再起動/停電は未検証です。
