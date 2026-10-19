# 中断した履歴保存の復旧

`recover_jade_pdf2docx_history.py`は、[ジョブ履歴](systemd_job_history.md)の書込中にプロセスが停止し、完全なJSONが一時名に残った場合に、fileの公開を完了します。対象は一つの保存先の一つのtokenです。要求schema 1/2と[永続キュー](durable_queue.md)に対応し、受付・終了フックと同じ版のscriptを配置してください。

```sh
sudo python3 scripts/recover_jade_pdf2docx_history.py /var/lib/jade-pdf2docx/jobs \
  0123456789abcdef0123456789abcdef
sudo python3 scripts/jade_pdf2docx_job_history.py status /var/lib/jade-pdf2docx/jobs \
  0123456789abcdef0123456789abcdef
```

操作前にstoreをバックアップし、同じ保存先の受付とjobを停止してから実行することを推奨します。移動・複製したstoreを本番の再送先へ切り替えないでください。復旧コマンドは受付・終了フックと共通のdirectory排他を取り、書込中なら最長1秒で失敗します。運用条件・所有者/権限・local filesystemの条件はジョブ履歴と同じです。

復旧できる状態は次の二つです。

| 保存状態 | 復旧動作 |
| --- | --- |
| 完全な`.pending-TOKEN.queue.json`、`.pending-TOKEN.request.json`または`.pending-TOKEN.result.json`だけがある | fileを同期して公開名へhardlinkし、公開を同期した後に一時名を外す |
| 一時名と公開名が同じinodeの二つのhardlinkとして残る | 両名のinodeとlink数を検証し、同期してから一時名を外す |

キュー・要求・終了結果をすべて検証してから変更します。queueとrequestが併存する場合は同じschema 2 identityを要求します。終了結果は要求のidentity・schema・型・終了状態と一致する必要があります。破損・途中JSON・別inodeの競合・余分なhardlink・symlink・所有者/権限不正・要求欠損は終了コード2で拒否し、記録を変更しません。破損fileを削除したり空の記録に置換したりする機能はありません。

正常終了は0で、元の記録と`repaired`配列をJSONで返します。配列には公開を完了した`queue`/`request`/`result`が入ります。完了済みの記録へ再実行しても上書きせず、配列は空です。I/O障害や再度のSIGKILLでは、一方の記録だけ復旧済みになることがあります。原因を解消して同じコマンドを再実行してください。復旧自身のhardlink後の停止も再実行できます。

queueのみでrequestがなければ`queued`を返します。キューの`status`で照会し、明示dispatchで起動できます。requestの一時記録も存在する場合は復旧後も再起動しません。

requestがあり終了結果がなければ、復旧結果の`state`と`output`は`unknown`です。このコマンドはmanagerへ問い合わせないため、現在の生存状態は続けて`status`で確認します。要求の公開を完了しても、変換を起動・再送せず、要求キーを解放しません。[同じ要求キーの再送](durable_requests.md)も元tokenを照会し、未確定なら75を返します。

終了結果が残っている場合も、保存されていた終了情報だけを復元します。現在のDOCXから成功を推測せず、出力を読み書きしません。`reported_published`は元CLIのexit 0の記録です。出力と終了記録のtransaction、実ホスト再起動/停電、ストレージ故障からの復旧は保証しません。

公開済み要求だけが残った「起動前の停止」と「起動したが結果を失った状態」は、この操作でも区別できません。永続キューはrequest同期前だけを再開可能にします。同期後の公開済みDOCXは[要求との照合](output_reconciliation.md)で調べられますが、実行や成功を確定するtransactionは残課題です。`unknown`を削除して同じキーで再起動したり、別キーで機械的に再送したりせず、元manager・出力・履歴を運用側で照合してください。

`test_history_recovery.py`は実SIGKILL後の同一キー再送、復旧の再中断、共有排他、競合/破損拒否、元の出力保持をmacOSとLinuxで検証します。実systemd・実ホスト再起動/停電はこのテストの対象ではありません。
