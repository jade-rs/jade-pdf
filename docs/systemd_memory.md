# 親CLIを含むLinuxメモリ制限

`scripts/run_jade_pdf2docx_systemd.sh`は、一つの変換をsystemdの一時serviceとして起動します。通常CLIの親・監督・worker・描画/OCR子孫を同じcgroupへ入れ、起動前から合計メモリを制限します。service外のsystemd managerと`systemd-run`待機プロセスはこの容量に含みません。

Linuxのcgroup v2、memory/swap controllerとsystemd 254以降が必要です。user managerへmemory controllerが委譲されている環境では、一般ユーザーで実行できます。

```sh
sh scripts/run_jade_pdf2docx_systemd.sh 2048 jade-pdf2docx-job-unique-id \
  /opt/jade-pdf2docx/bin/jade-pdf2docx input.pdf output.docx --strict
```

引数は正のMiB容量、変換ごとに一意の`jade-pdf2docx-`で始まるjob名、実行ファイルの絶対パス、通常CLIの引数です。job名は英数字とハイフンで128文字以内にしてください。入力・出力の相対パスは呼出元directoryを基準にし、空白や`$`も元の引数のまま渡します。`PDFIUM_LIB_PATH`を指定した場合はserviceへ明示的に渡します。OCRのmodel/charsetは通常CLIの絶対パス指定を推奨します。その他の呼出元環境変数はserviceへ引き継ぎません。

user managerを委譲しないサーバーでは、管理者がsystem managerから起動できます。`sudo`の元ユーザーのuid/gidを使うため、変換自体をrootにする必要はありません。

```sh
sudo --preserve-env=PDFIUM_LIB_PATH sh scripts/run_jade_pdf2docx_systemd.sh --system \
  2048 jade-pdf2docx-job-unique-id /opt/jade-pdf2docx/bin/jade-pdf2docx input.pdf output.docx --strict
```

serviceには`MemoryMax`、`MemorySwapMax=0`、`OOMPolicy=kill`、`KillMode=control-group`を設定します。`OOMPolicy=kill`はkernelの`memory.oom.group=1`も設定します。さらにservice内で`memory.max`が指定byte数と一致し、swap禁止・group OOMが有効なことをkernelから読み直してから、同じPIDをCLIへ置き換えます。設定が無効・未委譲ならCLIを起動せず失敗します。直接起動へのfallbackはありません。仕様は[systemdのOOMPolicy](https://github.com/systemd/systemd/blob/main/man/systemd.service.xml)と[systemd-run](https://github.com/systemd/systemd/blob/main/man/systemd-run.xml)を参照してください。

内側の`--max-memory-mib`と`--memory-cgroup-parent`は通常は指定しません。外側serviceの制限が親も含めて適用されます。内側で専用cgroupを作るための委譲はこのwrapperでは行いません。library/exampleの直接呼出には適用されません。

OOMではCLI自身も停止するため、通常CLIの`conversion memory limit exceeded`や終了コード1ではなく、`systemd-run`の非ゼロ終了と`oom-kill`診断を扱います。親CLIだけのSIGKILLでもmanagerが残りのserviceプロセスを停止し、空cgroupを回収します。停止の猶予は5秒です。CLI側のguardは同時SIGKILLでは動かないため、停止後の登録済みworkspaceは同じユーザーで`jade-pdf2docx --cleanup-workspace <絶対パス>`を実行します。登録前の未完成領域は管理者による回収が必要です。

自動回収と明示回収は、所有markerを残して作業fileを先に削除します。作業fileの削除に失敗した場合や、その途中で回収者がSIGKILLされた場合は、権限修復・全所有者の停止確認後に同じ明示回収を再実行できます。markerの削除から空directoryの削除までの間で停止すると、空の未登録領域が残る可能性があります。この場合もprefixだけで所有を推測せず、管理者が確認して回収してください。電源断時の永続性は未検証です。

公開前のOOM・停止では入力PDFと既存DOCXを保持します。最終rename後の強制停止では、新しいDOCXが残っていてもserviceは失敗になります。公開結果を確認してから再試行してください。呼出側の`systemd-run`待機プロセスだけをSIGKILLしてもserviceは継続します。明示停止は`systemctl --user stop jade-pdf2docx-job-unique-id.service`、system managerでは`sudo systemctl stop jade-pdf2docx-job-unique-id.service`です。

容量はこのjobの群にkernelがaccountするメモリで、RSS合計や瞬間的な絶対上限ではありません。複数jobの合算上限は以下の共有sliceで制限できます。同時実行数の受付制限は末尾のpool受付を使用してください。待ち行列は別途ジョブ管理側で扱います。

共有sliceによる複数jobの合計上限

管理者は[jade-pdf2docx.slice](../deploy/systemd/jade-pdf2docx.slice)をsystem managerの`/etc/systemd/system/jade-pdf2docx.slice`へ配置し、`sudo systemctl daemon-reload`を実行します。サンプルは合計4096 MiB・swap禁止です。user managerでは`~/.config/systemd/user/jade-pdf2docx.slice`へ配置し、`systemctl --user daemon-reload`を実行します。slice名は`jade-pdf2docx`で始まる英数字と`.slice`で128文字以内とし、ハイフンによる階層化は扱いません。

```sh
sudo --preserve-env=PDFIUM_LIB_PATH sh scripts/run_jade_pdf2docx_systemd.sh --system \
  --pool jade-pdf2docx.slice 4096 2048 jade-pdf2docx-job-unique-id \
  /opt/jade-pdf2docx/bin/jade-pdf2docx input.pdf output.docx --strict
```

`--pool <slice名> <合計MiB>`は個別MiBの前に指定します。`--system`を使う場合はその直後です。全jobに同じmanager・slice名・合計容量を指定します。user managerは同じユーザー内、system managerは複数ユーザーをまたいで同じsliceの合計を制限します。wrapperは共有容量を変更せず、jobのserviceをそのsliceの直下へ入れます。新しいjobが他のjobの予算を書き換えることはありません。

service内で、実際の直上cgroupの名前、`memory.max`の指定byte数、`memory.swap.max=0`、`memory.oom.group=0`を確認してからCLIを起動します。未定義sliceが自動作成されても上限未設定なら失敗し、容量不一致・swap有効・pool一括group OOMも`aggregate memory pool unavailable`として拒否します。管理者は実行中もこれらの設定を維持してください。起動時の照合であり、管理者による後続の設定変更を監視する機能はありません。

各serviceは従来どおり`memory.oom.group=1`で、OOM時の停止単位をjobにします。共有sliceの容量不足でも失敗するjobは一つとは限らず、選択や公平性を保証しません。メモリの回収とOOMの挙動は[kernelのcgroup v2仕様](https://docs.kernel.org/admin-guide/cgroup-v2.html#memory)に従います。poolに属さない変換、別managerのjob、systemd manager・待機プロセスは合計に含みません。待機プロセスだけが死亡しても、生存serviceはpoolの容量を使用し続けます。

複数の呼出元に共通の同時実行枠

`scripts/run_jade_pdf2docx_pool.py`はPython 3.11以降の標準libraryだけで動くLinux用受付です。上記の共有sliceを構成したうえで、管理者が[jade-pdf2docx-pool.json](../deploy/systemd/jade-pdf2docx-pool.json)を`/etc/jade-pdf2docx/pool.json`へ配置します。system manager用の設定はroot所有の通常fileとし、group/otherの書込権限を付けないでください。親directoryも管理者が保護します。サンプルは合計4096 MiB、jobあたり2048 MiB、同時2件です。sliceの実際の合計容量はJSONと一致させます。

```sh
sudo --preserve-env=PDFIUM_LIB_PATH python3 scripts/run_jade_pdf2docx_pool.py --system \
  --config /etc/jade-pdf2docx/pool.json \
  /opt/jade-pdf2docx/bin/jade-pdf2docx input.pdf output.docx --strict
```

user managerでは`--system`を省略し、そのユーザーまたはroot所有の設定を指定します。同時数は1〜64です。system managerは全呼出ユーザーに共通、user managerは同じuid内の枠です。全jobを同じmanager・sliceのこの受付経由で起動してください。直接CLIや`run_jade_pdf2docx_systemd.sh`で別名を使うjobは同時数に含みません。この受付は信頼した呼出元向けで、任意コマンドを起動できるsudo権限を制限するsecurity境界ではありません。

枠の実体は`jade-pdf2docx-jade-pdf2docx-slot-1.service`、`jade-pdf2docx-jade-pdf2docx-slot-2.service`という固定service名です。`slice=jade-pdf2docxProduction.slice`なら`jade-pdf2docx-jade-pdf2docxProduction-slot-1.service`になります。同じserviceの重複起動をsystemdが拒否するため、受付や`systemd-run`待機側の死亡で実行中jobの枠が解放されることはありません。managerがunitを回収し、`LoadState=not-found`になるまで使用中と数えます。停止途中・inactive・failedのunitを受付側から停止/削除して空けることはありません。各jobの引数・uid/gid・メモリ検証は既存wrapperを使い、終了コードもそのまま返します。変換失敗を自動再実行しません。

`LoadState=not-found`でも空cgroupだけ残る場合があるため、新しいjobを起動する前に、受付gate内で設定されたsliceの`ControlGroup`を照会します。設定済みsliceがまだ停止中なら先に起動して再照会します。残存slotがある場合は各階層と元群をnofollowのFDで固定し、`cgroup.events`が`populated 0`、managerが未追跡、同じdevice/inodeであることを確認して同期的に`rmdir`します。これにより古いFDが新jobのkernel群を指す再利用を防ぎます。生存メンバー・別群・権限不足・不正状態は、履歴/tokenのclaimやCLI起動より前に終了コード2で拒否します。生存群へのkillや子群の再帰削除は行いません。kernelの`EBUSY`/`ENOTEMPTY`は最大5秒のポーリング期限内で再確認し、manager照会は1回最大2秒、slice起動は最大5秒です。元群がすでに削除済みなら回収済みとして扱います。cgroup親の参照権限と、残存時の`rmdir`権限が必要です。Ubuntu noble/systemd 255のmemory controller委譲環境では、一般uidのuser managerでも実受入を確認しています。他のkernel/systemdや委譲構成は未検証です。

pool待機側が生存していれば、通常終了・変換失敗でも結果を返す前に元群を回収します。起動前に空slotと親FDを確認し、最初の受付gate内で今回のtokenと元群FDを固定します。変換終了後はgateを再取得し、取消と同じ元FD・manager状態・空状態・device/inodeの検査で回収します。元群が既に削除済みなら同名の新jobは照会も削除もしません。回収成功後は元の変換終了コードを返します。観測不足や回収失敗は2、gate期限超過は75で、起動済み変換は一度だけ待ち、自動再実行しません。待機側自体のSIGKILLやwrapper直接実行にはこの終了時回収は適用されず、残留群は次回受付時にも検査します。

全枠使用中はCLIを開始せず終了コード75と`pool busy`を返します。待ち行列や公平性、受付順序の保証はありません。呼出側でbackoffやキューを管理してください。起動時の枠選択とservice登録の観測、および終了後の元群回収を短いfile lockで直列化し、変換の終了待ち中はlockを保持しません。lock取得は5秒で打ち切り75を返します。登録の観測が5秒で終わらない場合や通信が失敗した場合は診断を出し、元の起動結果を待ちます。別名への自動再起動はしません。service名の排他は観測失敗時もmanagerが維持します。

受付は起動前にservice名と今回のtokenをstderrへ出します。呼出側の切断・Ctrl-C・SIGKILLが変換を取り消したことを意味するとは限りません。`sudo systemctl status jade-pdf2docx-jade-pdf2docx-slot-1.service`で状態を確認します。枠のservice名は再利用されるため、対象jobを強制停止するときは、記録したservice名とtokenを下記のcancel入口へ指定してください。名前だけの`systemctl stop`や手動でのtoken照合後の停止には、照合と停止の間に新jobへ枠が再利用される競合が残ります。

```sh
sudo python3 scripts/cancel_jade_pdf2docx_pool.py --system --config /etc/jade-pdf2docx/pool.json \
  jade-pdf2docx-jade-pdf2docx-slot-1.service 0123456789abcdef0123456789abcdef
```

最後の引数は、受付時に記録した32桁のtokenへ置き換えます。

cancelは受付と同じmanager・policy・gateを使用します。設定されたslot以外のservice、形式不正/古いtoken、登録済みjobと異なる群は停止前に終了コード2で拒否します。群のdirectoryを各階層nofollowで開き、tokenと群を再照合してから、その元FDから`cgroup.kill`へ一度だけ書き込みます。削除後に同じpathnameへ新jobが作られても元FDから新群を開き直しません。停止を名前指定の別プロセスへ委譲しないため、cancel呼出側が死んで遅れた名前指定stopが新jobへ到達する経路もありません。

これはSIGKILLによる強制停止です。[kernelのcgroup v2仕様](https://docs.kernel.org/admin-guide/cgroup-v2.html#core-interface-files)に従い、対象群と子孫群を停止します。元群の`cgroup.events`が`populated 0`になるか、元群が削除されることを5秒まで確認します。その後も受付gateと元FDを保持し、元kernel群の削除を確認してから0を返します。systemdがunitをGC済み（`LoadState=not-found`）なのに空群が残る場合だけ、固定した親FDから名前と元群のdevice/inodeを再照合して`rmdir`します。managerがまだ所有する群や、別の同名群は削除しません。kernelの`EBUSY`/`ENOTEMPTY`は5秒のポーリング期限内で再確認し、状態照会は1回最大2秒です。元群の削除判定にはFD相対の`cgroup.events`の消失を使い、kernfsのリンク数には依存しません。

停止・回収の失敗や期限切れは2で診断し、自動で停止を再送しません。gate取得の5秒timeoutは75です。0は変換成功・DOCX未公開・workspace回収完了を意味しません。削除処理を別プロセスへ残さず、回収完了または失敗後にgateを解放します。同じ受付を使うjobは回収中のslotを再利用できません。停止と群削除を確認後、残存する登録済みworkspaceを所有ユーザーの通常CLIで回収し、公開済みDOCXがある場合は結果を確認してから再試行します。

user managerでは`--system`を省略します。cgroup v2の`cgroup.kill`、そのjob群への書込権限、および空群回収時は親directoryでの`rmdir`権限が必要です。未対応kernelや委譲不足は2で失敗し、名前による停止へfallbackしません。実受入はLinux/system manager・service一般uidと、Ubuntu noble/systemd 255のmemory controller委譲済みuser managerで検証しています。信頼した呼出元が同じ受付を使う前提で、管理者による稼働中の群移動や直接wrapperを使う起動は対象外です。

system用gateは`/run/jade-pdf2docx-pool/<slice名>.lock`、user用は`/run/user/<uid>/jade-pdf2docx-pool/<slice名>.lock`に置きます。directoryとfileは所有者専用で、linkは拒否します。同じsliceに異なる同時数や容量を指定すると上限を破るため、policy全体のfingerprintを最初の起動前に固定します。同じ設定なら別のJSON pathからも利用できます。変更・破損は終了コード2で拒否し、再起動まで保持します。設定を変更する管理者は、**新規受付を止め、全slot serviceと全受付プロセスを停止した後にだけ**、対応するgate fileを削除し、JSONとslice設定を更新してください。実行中にgate fileを削除/置換すると排他が分離するため禁止します。

終了記録をserviceの回収後も保持する場合は、[token別のジョブ履歴](systemd_job_history.md)を有効にしてください。`ExecStopPost`が受付プロセスと独立して結果を保存します。記録がない場合は不明とし、名前の再利用や待機側の終了コードから成功・未実行を推定しません。
