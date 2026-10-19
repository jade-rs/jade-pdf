# token別のジョブ履歴

Linuxの[pool受付](systemd_memory.md)へ`--history-dir`を指定すると、起動前の要求とsystemdの終了結果をtoken別に保存します。Python 3.11以降の標準libraryを使用します。既定では保存しません。

同じ要求を重複起動せず再送する場合は、[永続要求キー](durable_requests.md)を併用します。キー付き要求はschema 2にidentityのSHA-256を含め、キーなし要求は従来のschema 1を使います。両方を同じ保存先で保持・照会できます。

system managerでは、管理者が永続disk上にroot所有・0700のdirectoryを事前作成します。保存先は絶対pathとし、各階層のsymlinkと`..`を拒否します。fileの同期・hardlinkによる排他的公開・directoryの同期に対応したLinux filesystemが必要です。運用中のdirectory移動・置換・権限変更は避けてください。`/run`や`/tmp`は再起動後の保存先には使いません。

```sh
sudo install -d -m 700 /var/lib/jade-pdf2docx/jobs
sudo --preserve-env=PDFIUM_LIB_PATH python3 scripts/run_jade_pdf2docx_pool.py --system \
  --config /etc/jade-pdf2docx/pool.json --history-dir /var/lib/jade-pdf2docx/jobs \
  /opt/jade-pdf2docx/bin/jade-pdf2docx input.pdf output.docx --strict
```

受付は要求recordの同期後、service名と32桁のtokenをstderrへ表示してから一度だけ起動します。保存が失敗すると終了コード2で拒否し、変換を起動しません。同時実行枠または履歴の満杯、受付lockの待機期限超過は75です。拒否時は新tokenを発行せず、起動や自動再送をしません。要求recordは実行開始の証明ではありません。同期後・manager登録前に受付が死亡した場合も要求だけが残ります。

`ExecStopPost`がmanagerの`SERVICE_RESULT`・`EXIT_CODE`・`EXIT_STATUS`を記録します。受付と`systemd-run`待機側を同時SIGKILLしても、このフックはmanagerから起動します。system managerではフックだけをrootで実行し、通常CLIは従来どおりsudo呼出元のuid/gidで動かします。配置するPython/shell scriptとその親directoryは管理者が信頼するものを使用し、job終了まで同じpathで利用可能にしてください。フックの引用・環境変数展開無効化は[systemdのExec command仕様](https://github.com/systemd/systemd/blob/v255/man/systemd.service.xml)を使い、transient propertyの引数は[systemd-runのproperty解析](https://github.com/systemd/systemd/blob/v255/src/shared/bus-unit-util.c)に従います。

別プロセスから次のように照会します。末尾は発行されたtokenへ置き換えてください。

```sh
sudo python3 scripts/jade_pdf2docx_job_history.py status /var/lib/jade-pdf2docx/jobs \
  0123456789abcdef0123456789abcdef
```

JSONの`state`と`output`は次の意味です。

| state | 観測内容 | output |
| --- | --- | --- |
| `succeeded` | 保存した終了結果が`success / exited / 0` | `reported_published` |
| `failed` | 保存した終了結果が非ゼロ終了・signal・OOMなど | `unknown` |
| `running` | 終了記録はなく、同じtokenのserviceが現在active | `unknown` |
| `unknown` | 要求だけ残り、元serviceの実行状態を確定できない。または終了情報が不足 | `unknown` |

`reported_published`は通常CLIのexit 0を観測した記録です。現在のDOCXの存在・内容・後続jobによる置換や、電源断後の出力directoryの永続化を保証しません。最終rename後にCLIが停止した場合、`failed`でもDOCXが公開済みの場合があります。`unknown`も成功・未実行・安全な再試行を意味しません。既存出力を確認してから再試行してください。

要求・終了fileは所有者専用の通常JSONとし、サイズ・schema・identity・重複key・linkを検査します。tokenの要求と終了結果はそれぞれ一度だけ公開し、既存記録を上書きしません。確定した終了記録の照会ではmanagerを呼ばないため、serviceのGCや同じ枠の再利用後も旧jobを照会できます。保存先欠損・権限不正・破損など、信頼できる要求を読めない場合は終了コード2で診断します。正常な照会は`failed`や`unknown`でも0を返すため、呼出元はJSONを判定してください。

保存先一つにつき保持枠は固定4,096件です。完了・実行中・未確定・破損のtokenを同じ一件として数え、同じtokenのキュー/要求/終了/一時fileを集約します。旧版の中断fileや無関係なentryも各一枠を使います。内容を読めないrecordも空きへ戻さず、満杯になると既存記録を保持して新規受付を拒否します。上限を下回る既存storeはそのまま利用できます。既に上限以上のstoreでは新規受付だけを拒否し、既存jobの終了記録と照会は継続できます。

各recordの書込は4 KiB以下に制限し、一時名をtokenとrecord種別ごとに固定します。SIGKILLで一時fileが残った場合、再試行はそれを置換せず失敗し、追加の一時fileを増やしません。この場合も状態は生存観測か`unknown`です。専用の空storeからこの版だけで作成した管理対象データは最大6 file名/token・最大96 MiBのfile内容に収まります。通常完了時は2 file名/token、キュー付きでは3 file名/tokenです。既存fileの内容、他の書込元、filesystemのmetadata/block使用量はこの値に含まれません。物理diskの空きや終了recordの書込成功を予約するdisk quotaではないため、運用側でもdisk容量を監視してください。

完全な一時JSON、または一時名と公開名が同じinodeの二つのhardlinkとして残った場合は、[中断した履歴保存の復旧](history_recovery.md)で公開を完了できます。通常readerは複数hardlinkを拒否するため、公開直後に停止した記録は復旧まで照会エラーになることがあります。復旧は内容とlink関係を検証し、要求キーや出力を変更せず、変換を再起動しません。

同じ保存先を使う全poolの受付と終了書込は、directory inodeへの`flock`を共有します。lock待機は1秒で打ち切り、受付は75、終了フックは2で診断します。受付のscanは排他取得後にdirectory FDを巻き戻して最新の公開名を読み、entry数を制限し、終了フックは満杯チェックを通さず既存tokenの結果を保存します。lock FDは子へ継承せず、保持者の死亡でも解放します。Linuxのdirectory `flock`に対応したローカルfilesystemを使用し、異なる版のscriptを同じstoreで同時運用しないでください。filesystem自体のI/O時間を強制制限するものではありません。

終了フックはserviceのメモリ/時間制限を受けます。OOM・disk不足・lock競合・manager停止・強制再起動などで終了記録が残らない場合があります。その場合は生存serviceの観測か`unknown`を返し、成功へ補完しません。fileとdirectoryをfsyncしますが、ストレージ故障や要求/出力/終了結果をまたぐ一つのtransactionは保証しません。

保持期限・自動削除は提供しません。満杯時は同じstoreを使う全呼出元の受付を止め、全jobの終了とフック完了を確認してください。未確定jobはmanagerと出力を確認してから扱います。旧storeを削除せず保管し、新しい絶対pathへ所有者専用の空storeを作成して、全呼出元の`--history-dir`を切り替えて受付を再開します。旧tokenの照会には旧storeを指定します。実行中のstore移動、fileの間引き、未完了jobの記録削除は避けてください。全storeを合わせた容量と保管期限は管理者の運用が必要です。

user managerでは`--system`を省略し、自分が所有する0700の永続directoryを使います。実委譲環境でのuser managerは未検証です。受付と起動を分離する[永続キュー](durable_queue.md)も同じstoreを利用します。queueのみのtokenはキューのstatusで照会してください。自動巡回・自動再送・認証付き遠隔API・workspace回収は提供しません。

生成入力のPython harnessは `tests/test_job_history.py` と `tests/test_job_history_capacity.py` で検査する。実ホスト再起動・停電、実OCR推論中の履歴保存、storage faultからの復旧は未検証。
