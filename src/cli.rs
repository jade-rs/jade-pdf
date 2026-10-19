//! OSのパス表現を保持するCLI引数の検証。
use std::{ffi::OsString, path::PathBuf, process::Command, time::Duration};

/// 明示的な実OCRのモデルとページ描画設定。
#[derive(Debug)]
pub(crate) struct OcrOptions {
    /// 公開PARSeq ONNXモデルのパス。
    pub(crate) model: PathBuf,
    /// モデルに対応する文字セットYAMLのパス。
    pub(crate) charset: PathBuf,
    /// PDFページの描画解像度。
    pub(crate) dpi: u32,
    /// 縦書き列を右から左へ読む明示指定。
    pub(crate) vertical: bool,
    /// 紙面の陰影補正を明示的に有効にする。
    pub(crate) paper_shading: bool,
}

/// 起動・変換・保存を親で監督する既定の待機期限。
pub(crate) const DEFAULT_TIMEOUT: Duration = Duration::from_millis(300_000);
/// 同じ実行ファイル内で再帰起動を避ける内部worker引数。
pub(crate) const WORKER_FLAG: &str = "--jade-pdf2docx-conversion-worker";
/// 親の生存leaseを持ち、一時領域と変換workerを所有する内部引数。
pub(crate) const SUPERVISOR_FLAG: &str = "--jade-pdf2docx-conversion-supervisor";

/// Linuxの変換群に適用するカーネルメモリ上限。
#[derive(Debug)]
pub(crate) struct MemoryOptions {
    /// カーネルへ設定するbyte数。
    pub(crate) bytes: u64,
    /// memory controllerを有効化済みの委譲先。
    pub(crate) parent: PathBuf,
}

/// 検証済みの変換要求。
#[derive(Debug)]
pub(crate) struct Request {
    /// 元のPDFパス。
    pub(crate) input: PathBuf,
    /// workerが固定したPDFへ照合する小文字hexのSHA-256。
    pub(crate) input_sha256: Option<String>,
    /// 親が公開前にDOCXへ含める要求キーとfingerprint。
    pub(crate) publication_receipt: Option<String>,
    /// DOCXの保存先。
    pub(crate) output: PathBuf,
    /// 品質損失をエラーにする指定。
    pub(crate) strict: bool,
    /// デバッグ情報を出力する指定。
    pub(crate) debug: bool,
    /// 起動・変換・保存を親で監督する待機期限。
    pub(crate) timeout: Duration,
    /// 指定時だけ実推論で画像ページを補う。
    pub(crate) ocr: Option<OcrOptions>,
    /// 親が一度だけ設定し、子孫へ継承させる上限。
    pub(crate) memory: Option<MemoryOptions>,
}

impl Request {
    /// 親・監督・workerの変換設定を同じ引数表現で引き継ぐ。
    pub(crate) fn append_conversion_args(&self, command: &mut Command) {
        if let Some(digest) = &self.input_sha256 {
            command.arg("--input-sha256").arg(digest);
        }
        if self.strict {
            command.arg("--strict");
        }
        if self.debug {
            command.arg("--debug-output");
        }
        if let Some(ocr) = &self.ocr {
            command
                .arg("--ocr")
                .arg("--ocr-model")
                .arg(&ocr.model)
                .arg("--ocr-charset")
                .arg(&ocr.charset)
                .arg("--ocr-dpi")
                .arg(ocr.dpi.to_string());
            if ocr.vertical {
                command.args(["--ocr-direction", "vertical"]);
            }
            if ocr.paper_shading {
                command.arg("--ocr-paper-shading");
            }
        }
    }
}

/// CLIが実行する処理。
#[derive(Debug)]
pub(crate) enum Action {
    /// 使用法の表示。
    Help,
    /// バージョンの表示。
    Version,
    /// 明示した一時領域の所有情報と生存leaseを検査して残骸を回収する。
    CleanupWorkspace(PathBuf),
    /// 所有者が終了した空の専用cgroupを、明示パスで回収する。
    CleanupMemoryGroup(PathBuf),
    /// 親プロセスで監督する変換。
    Convert(Request),
    /// 親の死亡時も変換workerを回収する監督プロセス。
    Supervisor(Request),
    /// 隔離された子プロセス内の変換。
    Worker(Request),
}

/// flagの重複と正の整数timeoutを検証し、パスのOS表現を維持する。
pub(crate) fn parse(args: &[OsString]) -> Result<Action, ()> {
    if args
        .first()
        .is_some_and(|arg| arg == "--cleanup-memory-cgroup")
    {
        return if args.len() == 2 && PathBuf::from(&args[1]).is_absolute() {
            Ok(Action::CleanupMemoryGroup(PathBuf::from(&args[1])))
        } else {
            Err(())
        };
    }
    if args.first().is_some_and(|arg| arg == "--cleanup-workspace") {
        return if args.len() == 2 && PathBuf::from(&args[1]).is_absolute() {
            Ok(Action::CleanupWorkspace(PathBuf::from(&args[1])))
        } else {
            Err(())
        };
    }
    if args.len() == 1 {
        if args[0] == "--help" || args[0] == "-h" {
            return Ok(Action::Help);
        }
        if args[0] == "--version" {
            return Ok(Action::Version);
        }
    }
    let worker = args.first().is_some_and(|arg| arg == WORKER_FLAG);
    let supervisor = args.first().is_some_and(|arg| arg == SUPERVISOR_FLAG);
    let args = if worker || supervisor {
        &args[1..]
    } else {
        args
    };
    if args.len() < 2
        || args[..2]
            .iter()
            .any(|arg| arg.to_string_lossy().starts_with("--"))
    {
        return Err(());
    }
    let mut request = Request {
        input: PathBuf::from(&args[0]),
        input_sha256: None,
        publication_receipt: None,
        output: PathBuf::from(&args[1]),
        strict: false,
        debug: false,
        timeout: DEFAULT_TIMEOUT,
        ocr: None,
        memory: None,
    };
    let mut memory_bytes = None;
    let mut memory_parent = None;
    let mut timeout_seen = false;
    let mut ocr_seen = false;
    let mut model = None;
    let mut charset = None;
    let mut dpi = None;
    let mut vertical = None;
    let mut paper_shading = false;
    let mut flags = args[2..].iter();
    while let Some(flag) = flags.next() {
        if flag == "--input-sha256" && request.input_sha256.is_none() {
            let digest = flags.next().and_then(|value| value.to_str()).ok_or(())?;
            if digest.len() != 64
                || !digest
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            {
                return Err(());
            }
            request.input_sha256 = Some(digest.to_string());
        } else if flag == "--publication-receipt"
            && !worker
            && !supervisor
            && request.publication_receipt.is_none()
        {
            let receipt = flags.next().and_then(|value| value.to_str()).ok_or(())?;
            if !crate::publication_receipt::valid(receipt) {
                return Err(());
            }
            request.publication_receipt = Some(receipt.to_string());
        } else if flag == "--max-memory-mib" && !worker && !supervisor && memory_bytes.is_none() {
            let value = flags.next().and_then(|value| value.to_str()).ok_or(())?;
            if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(());
            }
            let bytes = value
                .parse::<u64>()
                .map_err(|_| ())?
                .checked_mul(1024 * 1024)
                .ok_or(())?;
            if bytes == 0 || bytes > i64::MAX as u64 {
                return Err(());
            }
            memory_bytes = Some(bytes);
        } else if flag == "--memory-cgroup-parent"
            && !worker
            && !supervisor
            && memory_parent.is_none()
        {
            let path = PathBuf::from(flags.next().ok_or(())?);
            if !path.is_absolute() {
                return Err(());
            }
            memory_parent = Some(path);
        } else if flag == "--strict" && !request.strict {
            request.strict = true;
        } else if flag == "--debug-output" && !request.debug {
            request.debug = true;
        } else if flag == "--ocr" && !ocr_seen {
            ocr_seen = true;
        } else if flag == "--ocr-model" && model.is_none() {
            model = Some(ocr_path(flags.next())?);
        } else if flag == "--ocr-charset" && charset.is_none() {
            charset = Some(ocr_path(flags.next())?);
        } else if flag == "--ocr-direction" && vertical.is_none() {
            vertical = Some(match flags.next().and_then(|value| value.to_str()) {
                Some("horizontal") => false,
                Some("vertical") => true,
                _ => return Err(()),
            });
        } else if flag == "--ocr-paper-shading" && !paper_shading {
            paper_shading = true;
        } else if flag == "--ocr-dpi" && dpi.is_none() {
            let value = flags.next().and_then(|value| value.to_str()).ok_or(())?;
            if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(());
            }
            let value: u32 = value.parse().map_err(|_| ())?;
            if !(1..=600).contains(&value) {
                return Err(());
            }
            dpi = Some(value);
        } else if flag == "--timeout-ms" && !worker && !timeout_seen {
            let value = flags.next().and_then(|value| value.to_str()).ok_or(())?;
            if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(());
            }
            let millis: u64 = value.parse().map_err(|_| ())?;
            if millis == 0 {
                return Err(());
            }
            request.timeout = Duration::from_millis(millis);
            timeout_seen = true;
        } else {
            return Err(());
        }
    }
    if request.publication_receipt.is_some() && request.input_sha256.is_none() {
        return Err(());
    }
    request.memory = match (memory_bytes, memory_parent) {
        (Some(bytes), Some(parent)) => Some(MemoryOptions { bytes, parent }),
        (None, None) => None,
        _ => return Err(()),
    };
    if ocr_seen {
        request.ocr = Some(OcrOptions {
            model: model.ok_or(())?,
            charset: charset.ok_or(())?,
            dpi: dpi.unwrap_or(150),
            vertical: vertical.unwrap_or(false),
            paper_shading,
        });
    } else if model.is_some()
        || charset.is_some()
        || dpi.is_some()
        || vertical.is_some()
        || paper_shading
    {
        return Err(());
    }
    Ok(if worker {
        Action::Worker(request)
    } else if supervisor {
        Action::Supervisor(request)
    } else {
        Action::Convert(request)
    })
}

/// 空パスや次のflagをOCRファイル名として受け付けない。
fn ocr_path(value: Option<&OsString>) -> Result<PathBuf, ()> {
    let value = value.ok_or(())?;
    if value.is_empty() || value.to_string_lossy().starts_with("--") {
        return Err(());
    }
    Ok(PathBuf::from(value))
}
