//! CLIの親でCtrl+CとUnixのSIGTERM・SIGHUPを協調停止tokenへ伝える。
use jade_pdf2docx_docx::ParseCancellation;

/// 専用threadで停止要求だけを設定し、回収やファイル操作は監督側に任せる。
/// CLIが受付を所有し、nohupなどから継承したsignal無視設定も置き換える。
pub(crate) fn install(cancellation: &ParseCancellation) -> Result<(), ctrlc::Error> {
    let cancellation = cancellation.clone();
    ctrlc::set_handler(move || cancellation.cancel())
}
