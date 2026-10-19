# Linuxの変換群メモリ制限

通常CLIでは、次の二つの引数を対で指定できます。

```sh
jade-pdf2docx input.pdf output.docx --max-memory-mib 2048 \
  --memory-cgroup-parent /sys/fs/cgroup/jade-pdf2docx-service
```

`--max-memory-mib`は正の整数で、1 MiBは1,048,576 bytesです。委譲先は絶対パスで指定し、サービス管理者がcgroup v2のmemory controllerを有効化して、実行ユーザーへ委譲しておく必要があります。CLIが共通祖先の`cgroup.procs`へ書き込めるよう、起動元も同じ委譲先内に配置してください。CLIはホスト全体のcontrollerや共通の制限値を変更しません。

変換ごとに専用cgroupを作り、`memory.max`、`memory.swap.max=0`、`memory.oom.group=1`を設定します。監督をexecする前に所属を変更するため、そのworker・Pdfium・描画・OCRの子孫にも同じ上限が適用されます。対象はカーネルがこの群にaccountするメモリです。RSSを足し合わせた値や、瞬間的な使用量の絶対上限とは異なり、kernelは一時的な超過を許す場合があります。[カーネルのcgroup v2仕様](https://www.kernel.org/doc/html/latest/admin-guide/cgroup-v2.html)を参照してください。

群自身のOOMを検出すると`conversion memory limit exceeded`で失敗し、公開前の既存DOCXと入力PDFを保持します。`cgroup.kill`でprocess groupを離れた子孫も停止し、自分の空cgroupだけを削除します。このため`cgroup.kill`を提供するLinux 5.14以降が必要です。通常directory、未委譲、controller不足、対応外OSは変換開始前に失敗します。制限を適用できない状態で変換を続けません。容量不足がOOMを起こさずallocationエラーになる場合は、その変換エラーが返ります。

親CLIがSIGKILLされると、監督は親pipeの切断を検出して変換を停止します。workspace guardの終了後、自分が所属する登録済みcgroupの`cgroup.kill`を使い、`setsid`でprocess groupを離れた子孫と監督自身を停止します。内部環境変数で受け取ったパスについても登録情報・所有uid・kernelの現在PID所属を検証し、同じユーザーの別cgroupは拒否します。停止権は元の`cgroup.kill` FDとして保持します。

この停止後に残った登録済みの空cgroupは、変換と同じユーザーで絶対パスを指定し、一件ずつ回収できます。子孫がworkspace leaseを保持していた場合は、全子孫の終了後に`--cleanup-workspace`でその登録済み領域も回収してください。

`--cleanup-workspace`は終了直後のlease解放を最大1秒待ちます。解放されなければ使用中として拒否し、成果物を保持します。待機後もdirectoryと所有markerのinodeを再確認し、差し替えられた領域は削除しません。

```sh
jade-pdf2docx --cleanup-memory-cgroup /sys/fs/cgroup/jade-pdf2docx-service/jade-pdf2docx-memory-v1-1234-5678-0
```

実際に残っている群の名前を指定してください。`jade-pdf2docx-memory-v1-<pid>-<timestamp>-<counter>`という名前・所有uid・有限の`memory.max`・swap禁止・group OOM・`cgroup.max.depth=0`を検査します。専用群の作成時は、directoryの排他生存leaseを取ってから最後にdepthを0へ設定して登録します。depth=0は子cgroupの作成も禁止します。親が生存中なら空の起動待ち群でも、所有者が死んでいても所属プロセスが残っていれば、終了コード1で拒否します。PIDの再利用では生存を判断しません。

明示回収はプロセスを停止せず、再帰削除も行いません。対象と親のFDを保持し、削除前にinodeを再検査します。通常directory・末尾symlink・旧版の名前・登録前/設定変更後の群は保持します。自動回収も同名の別inodeを削除しません。登録前の強制終了や、これらの検証を満たさない残骸はサービス管理者による委譲先のライフサイクル管理が必要です。同じuidの外部プロセスによる変更を排他的に防ぐものではありません。

親CLI自身と他のプロセスはこの専用群の対象外です。ライブラリやexampleにはこの引数の強制制限は適用されません。引数を省略した通常CLIもメモリ無制限のままです。親を含む全体の上限はLinux/systemdの[一時service用wrapper](systemd_memory.md)で適用できます。他OS、systemdを使わない親の制限、登録前の強制終了後の回収、OCRの性能・品質・配布はproduction判定の残課題です。

Linux CIでは`sh scripts/check_cgroup_memory.sh`で専用委譲先を用意し、元のrunnerユーザーへ戻って検証します。128 MiBに対して80 MiBの割当一つは成功し、二つの同時保持は合算OOMで失敗すること、離脱した子孫の停止、失敗したexec後の回収、実監督での既存出力保護、通常CLIのDOCX公開を検査します。回収では所有者SIGKILL前後・所属プロセスの保持・登録情報変更・同名inode置換を実カーネルで検証し、通常CLIの回収入口も実行します。通常CLIの親をFIFO入力待ちでSIGKILLする受入テストでは、別sessionにしたfixtureを同じcgroupへ所属させ、共有workspace leaseを保持させます。群の停止、一時領域と空群の明示回収、入力・既存出力・無関係な領域の保護、その後の実PDF再変換を一連で検証します。離脱fixtureはテスト側から所属させたもので、OCRが実際に生成する子孫ではありません。controllerや権限が足りなければCI自体を失敗させます。
