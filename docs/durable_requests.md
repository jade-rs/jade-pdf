# 永続要求キーによる再送制御

Linuxの[pool受付](systemd_memory.md)へ`--history-dir`と`--request-key`を渡すと、同じ保存先で同じ要求を二度起動しません。要求キーは呼出元が一度発行して保存する32桁の小文字hexです。このキーをjob tokenとしても使います。保存先とscriptの運用条件は[ジョブ履歴](systemd_job_history.md)に従います。既存のキーなし受付も利用できます。

```sh
# 要求キーと入力SHAは呼出元で保存し、再送にも同じ値を使う。
sudo --preserve-env=PDFIUM_LIB_PATH python3 scripts/run_jade_pdf2docx_pool.py --system \
  --config /etc/jade-pdf2docx/pool.json --history-dir /var/lib/jade-pdf2docx/jobs \
  --request-key 0123456789abcdef0123456789abcdef \
  /opt/jade-pdf2docx/bin/jade-pdf2docx /data/input.pdf /data/output.docx --strict \
  --input-sha256 "$INPUT_SHA256"
```

キー付き受付では実行ファイル・入力・出力を絶対pathにし、`--input-sha256`を一度だけ指定します。[workerのsnapshot照合](verified_input.md)が入力byteを検証します。要求のidentityにはmanager種別、pool設定全体、入力SHAを含む全CLI引数、作業directoryを含め、そのcanonical JSONのSHA-256を保存します。引数の順序や作業directoryを変えた再送も衝突として扱います。引数JSONは64 KiB以下です。binary・model・charset・runtimeの内容や環境変数のdigestは含めないため、それらは管理者が固定して配布してください。

新規要求は保存先の排他下で容量を確認し、schema 2の要求recordを同期してから一度だけ起動します。同時に別プロセスへ送っても、一件だけがキーを確保します。追加indexは作らず、既存の保持上限4,096件を共有します。既存要求の再送は満杯やservice枠の空き状況にかかわらず照会できます。同じキーで異なるidentity、旧schema 1のtoken、破損record、起動前の中断fileを見つけた場合は終了コード2で拒否し、新serviceを起動しません。

新規受付はstdoutへ`token`・`manager`・`service`・`replayed:false`のJSONを表示し、元の変換終了コードを返します。再送は元tokenの履歴JSONへ`replayed:true`を加えて表示します。完了済み要求はmanagerへ問い合わせずに返せます。

| 再送のstate | 終了コード | 動作 |
| --- | --- | --- |
| `succeeded` | 0 | 元の成功記録を返す |
| `failed` | 1 | 元の失敗記録を返す |
| `running` | 75 | 同じserviceの現在の生存を返す |
| `unknown` | 75 | 実行状態を確定できないことを返す |

失敗の再送は元CLIの詳細な終了コードを再現せず、終了履歴に残る`exit_code`と`exit_status`を返します。履歴照会専用コマンドの正常終了コード0とは異なります。いずれの再送も変換を起動せず、現在のDOCXを読み直したり書き直したりしません。成功の`reported_published`は元CLIのexit 0の記録であり、現在の出力の存在や内容の保証ではありません。

要求同期後・manager登録前に受付が死亡すると、実行されていなくても`unknown`が残ります。終了フックの失敗やホスト再起動で結果が欠損した場合も同様です。この方式は重複起動を防ぎますが、実行を必ず完了する保証や自動復旧は提供しません。`unknown`や失敗を別キーで機械的に再送せず、元のmanager・出力・履歴を運用側で照合してください。記録削除による再起動は提供しません。

履歴fileの書込途中で停止して完全な一時記録が残った場合は、[履歴保存の復旧](history_recovery.md)で公開を完了できます。復旧後も同じキーは元tokenを返し、要求だけの状態は`unknown`のまま再起動しません。

キーの範囲は一つの保存先です。保存先を切り替えると旧キーの確保情報は新しい保存先へ引き継がれません。切替前に全呼出元とjobを止め、旧storeを保管し、旧要求の再送先は旧storeに固定してください。旧キーを新storeへ送ると新規要求となります。別の出力を使っても同じキーのidentityは変更できません。

この版の受付と終了フックを同時に配布してください。旧版のフックはschema 2を読めません。実ホスト再起動・停電、共有network filesystem、実OCR推論中の再送は未検証です。自動巡回する待ち行列、出力公開と終了記録を一つにするtransaction、認証付きAPI、storeをまたぐ要求キー管理は今後の課題です。

Python harnessは独立8受付の同時送信、一時記録中断、起動失敗、同期失敗、要求確保後のSIGKILL、衝突と再送を検証します。

受付と起動を分離し、起動要求の同期前の停止から再開するには[永続キュー](durable_queue.md)を使います。同じ要求キーと保持上限を共有し、同期受付はqueued要求の起動を迂回できません。

この版のキー付き受付は、公開前のDOCXへ要求キーとfingerprintを自動付与します。終了結果欠損時は[公開済みDOCXとの照合](output_reconciliation.md)を使えます。元の終了状態は保持し、自動再送しません。
