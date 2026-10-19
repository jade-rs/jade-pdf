# Pdfiumの固定版・offline配布

native CLI/APIはPdfiumで文字を補完する。`scripts/pdfium_lock.json`は、
Cargo.lockの`pdfium-auto 0.3.1`と同じ`chromium/7961`（152.0.7961.0）を固定する。
Linux/macOSのx86_64・aarch64向けアーカイブについて、GitHub release assetのSHA-256と
実downloadの一致を確認し、ライブラリ、VERSION、LICENSE、`licenses/`の全ファイルの
サイズとSHA-256を記録している。

取得元は[bblanchon/pdfium-binariesのchromium/7961](https://github.com/bblanchon/pdfium-binaries/releases/tag/chromium/7961)。
上流の更新時は、公開digest、実アーカイブ、同梱notice、対応APIを再確認してlockを更新する。
依存版またはPdfium版の更新だけを行うと回帰テストが失敗する。

## ネットワークのある準備環境

以下はリポジトリrootで実行する。Python 3.11以上、curl、ビルド済みの`jade-pdf2docx`を使う。
ライブラリは実行時のhostに合わせ、musl/Windowsはこの配布対象に含めない。

```sh
pdfium_url=$(python3 -c 'import sys; sys.path.insert(0, "scripts"); import pdfium_bundle as p, json; print(json.loads(p.LOCK_PATH.read_text())["platforms"][p.host_platform()]["url"])')
curl --fail --location --retry 3 --max-time 120 "$pdfium_url" --output pdfium.tgz
python3 scripts/pdfium_bundle.py install --archive pdfium.tgz --destination ./pdfium-bundle
```

`install`はローカルのアーカイブだけを読み、downloadしない。別host向けには
`--platform linux-x86_64` / `linux-aarch64` / `macos-x86_64` / `macos-aarch64`を指定する。
既存の配布先は空directoryやsymlinkでも置換しない。新しいbasenameへ配置する。
アーカイブ全体、選択ファイル、全noticeを検査し、パストラバーサル、リンク、重複entryを拒否する。
header等の未選択ファイルは配置しない。

準備後は`pdfium-bundle/`全体を、`scripts/pdfium_bundle.py`、
`scripts/pdfium_lock.json`、対応host用の`jade-pdf2docx`と一緒に運ぶ。
アーカイブは実行環境に不要。license/noticeは削除しない。
CIのdefault feature jobはOSごとのPdfium bundleをartifactとして保存する。
これはPdfiumの配布物であり、アプリ本体のrelease packageは別途必要。

## offline環境での検査・実行

```sh
python3 scripts/pdfium_bundle.py verify --bundle ./pdfium-bundle
python3 scripts/pdfium_bundle.py run --bundle ./pdfium-bundle --binary ./jade-pdf2docx -- input.pdf output.docx
```

`verify`は実行host用の全固定ファイルを検査する。libraryだけでなくnoticeの欠落・改変や
markerの版/host不一致、不完全配置も拒否する。準備中に強制終了したdirectoryは
検査を通るまで使わず、内容を確認して新しいbasenameで作り直す。
スクリプトの検査/起動失敗は終了2。`run`は検査後に`PDFIUM_LIB_PATH`を設定して
`exec`するため、CLIの引数、終了コード、signal、supervisorのprocess境界を維持する。
両コマンドともdownloadしない。

CLI/APIへ`PDFIUM_LIB_PATH=/absolute/path/libpdfium.so`（macOSは`.dylib`）を直接渡すこともできる。
その場合のSHA-256検査は呼び出し元が行う。空指定・欠落・directory・不正binaryは
native抽出時に`ParseError::Pdfium`として返し、自動downloadへ切り替えない。
変数未指定時の従来の自動取得は残るため、offline運用には検査付きランチャーを使う。

配布先の親directory、script、lock、CLI、実行環境は信頼できるものを使い、
検査から実行中までbundleを書き換えない。配置後は実行ユーザーから書込できないよう管理する。
hash検査は署名や同一ユーザーによる同時置換への防御、native codeの資源制限を保証しない。

## 検証範囲と残課題

4アーカイブの整合性と配置を確認する。nativeのLinux/macOS feature matrixでは
固定libraryを使用し、検査付きランチャーの変換結果もDOCX packageとして検査する。
CPU architectureごとの実行可否は実際に実行したrunnerに限られ、cross配置だけで保証しない。

OCRの実モデル・checksum・配布、アプリ自身のlicense/依存監査、全fixtureの本文/visual品質、
総メモリ上限は別の未完了ゲート。Pdfiumの固定配布だけでproduction readyとは扱わない。
