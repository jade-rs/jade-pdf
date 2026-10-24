//! Record per-operation process supervision statistics on a pinned corpus.
use pdf_diff::isolation::{run_pdf_worker, ProcessLimits};
use std::ffi::OsString;
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args_os().skip(1);
    let binary = PathBuf::from(
        args.next()
            .ok_or_else(|| anyhow::anyhow!("usage: audit_isolation <pdf-diff> <PDF...>"))?,
    );
    for path in args.map(PathBuf::from) {
        let output = run_pdf_worker(
            &binary,
            &[OsString::from("--ir"), path.as_os_str().to_owned()],
            &ProcessLimits::default(),
        )?;
        let pages = if output.exit_code == 0 {
            Some(
                serde_json::from_slice::<serde_json::Value>(&output.stdout)?["pages"]
                    .as_array()
                    .ok_or_else(|| anyhow::anyhow!("Missing IR pages"))?
                    .len(),
            )
        } else {
            None
        };
        println!(
            "{}",
            serde_json::json!({"file":path,"exit_code":output.exit_code,"pages":pages,"stdout_bytes":output.stdout.len(),"stderr":String::from_utf8_lossy(&output.stderr).chars().take(500).collect::<String>(),"stats":output.stats})
        );
    }
    Ok(())
}
