# 起動前の要求を保持する永続キュー

`jade_pdf2docx_durable_queue.py`は受付と起動を分離します。`enqueue`はコマンドとpool設定を永続化するだけで、変換を起動しません。保存後に受付が停止しても、別プロセスの明示的な`dispatch`で同じtokenを起動できます。Linux/Python 3.11以降のsystemd環境と、[ジョブ履歴](systemd_job_history.md)と同じ私有store・script配置を使用します。

```sh
sudo install -d -m 700 /var/lib/jade-pdf2docx/jobs
sudo python3 scripts/jade_pdf2docx_durable_queue.py enqueue --system \
  --config /etc/jade-pdf2docx/pool.json /var/lib/jade-pdf2docx/jobs \
  0123456789abcdef0123456789abcdef \
  /opt/jade-pdf2docx/bin/jade-pdf2docx /data/input.pdf /data/output.docx --strict \
  --input-sha256 "$INPUT_SHA256"
sudo python3 scripts/jade_pdf2docx_durable_queue.py status /var/lib/jade-pdf2docx/jobs \
  0123456789abcdef0123456789abcdef
sudo --preserve-env=PDFIUM_LIB_PATH python3 scripts/jade_pdf2docx_durable_queue.py dispatch \
  /var/lib/jade-pdf2docx/jobs 0123456789abcdef0123456789abcdef
```

tokenと入力SHAは呼出元が一度発行して保存します。実行ファイル・入力・出力は絶対pathです。`TOKEN.queue.json`へpool設定・manager・全引数・受付時の作業directoryとidentity digestを保存し、fileとdirectoryを同期します。JSON全体が4 KiBを超える要求は保存前に拒否します。同じtoken・同じ内容の再受付は元の状態を返し、変更は終了コード2で拒否します。既存の同期受付の64 KiB引数上限とは異なります。

| 永続状態 | status | dispatch |
| --- | --- | --- |
| queueのみ公開済み | `queued` | 空き枠があれば起動要求を同期して一度だけ起動 |
| queueとrequestが公開済み、結果なし | `running`または`unknown` | 元の履歴を返す。終了コード75、再起動しない |
| 終了結果あり | `succeeded`または`failed` | 元の履歴を返す。終了コード0または1、再起動しない |
| 公開途中・破損・identity不一致 | エラー | 終了コード2、起動しない |

`status`は正常なJSON照会ならstateによらず0です。`dispatch`は起動時にtokenを表示し、完了を待って元CLIの終了コードを返します。再送のJSONには`replayed:true`が付きます。pool満杯や排他待ち期限超過は75で、queueのみの要求は待機状態を保ちます。独立dispatcher間でも起動要求を共有排他下で一度だけ確保します。同期受付からqueued tokenの起動を迂回することはできません。

起動要求`TOKEN.request.json`の同期が再起動可否の境界です。同期後・manager登録前にdispatcherが死亡すると、未実行でも`unknown`になります。以後は推測で再起動せず、[永続要求キー](durable_requests.md)と同じ照合手順で扱います。queue公開中に完全な一時JSONだけが残った場合は、[履歴復旧](history_recovery.md)で`queued`へ戻せます。requestの一時記録も残った場合は復旧後も`unknown`となり、再起動しません。

dispatchは受付時の作業directoryで起動します。未起動要求のdirectoryは維持してください。完了済みの再送は元directoryが失われても履歴を返せます。binary・model・runtime・環境変数の内容は記録しないため、管理者が固定して配布し、必要な環境をdispatch時に設定します。CLIは一プロセス一要求で使います。内部APIはprocess全体のcwdを切り替えるため、同じプロセス内の並列呼出には対応しません。

queue・request・resultと各一時名は同じtokenの一保持枠を使い、store上限は4,096件です。満杯でも受付済みqueueのdispatchと終了記録は可能です。queueは完了後も保持し、削除・キー再利用は提供しません。全関連scriptを同じ版で配布してください。

自動巡回・FIFO順序・daemon・自動再送は提供しません。再起動後の明示dispatchはできますが、実ホスト再起動/停電・共有network filesystemは未検証です。出力公開と終了記録のtransaction、`unknown`の自動解決は残課題です。

`test_durable_queue.py`の13件は、実SIGKILL、公開途中の復旧、独立8 dispatcherの競合、容量上限、cwd消失、衝突・破損拒否を検証します。Linux default CIでは実systemdでdispatcherをSIGKILLし、running再送・元PDFの変換完了・DOCX本文・完了後の出力保持を検証します。
