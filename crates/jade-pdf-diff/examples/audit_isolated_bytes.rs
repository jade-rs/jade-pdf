//! Differential audit on verified public bytes; never run this host-side oracle
//! on arbitrary/unpinned input. Application callers only need the isolated client.
use anyhow::{ensure, Context, Result};
use pdf_diff::isolation::{IsolatedPdfClient, IsolatedResponse};
use pdf_diff::{DiffOptions, Limits, RenderedPage};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

#[derive(Deserialize)]
struct Manifest {
    documents: Vec<Entry>,
    robustness: Vec<Entry>,
}
#[derive(Deserialize)]
struct Entry {
    id: String,
    sha256: String,
    bytes: usize,
    pages: Option<usize>,
    outcome: Option<String>,
    invalid_page: Option<usize>,
}

fn matched<T: Serialize>(
    name: &str,
    direct: Result<T>,
    remote: Result<IsolatedResponse<T>>,
) -> Result<serde_json::Value> {
    match (direct, remote) {
        (Ok(direct), Ok(remote)) => {
            ensure!(
                serde_json::to_value(direct)? == serde_json::to_value(remote.value)?,
                "{name}: native/isolated byte result differs"
            );
            Ok(
                serde_json::json!({"operation":name,"status":"matched","exit_code":remote.exit_code,"execution":remote.execution,"staging":remote.staging}),
            )
        }
        (Err(direct), Err(remote)) => Ok(
            serde_json::json!({"operation":name,"status":"both_rejected","direct_error":format!("{direct:#}").chars().take(500).collect::<String>(),"isolated_error":format!("{remote:#}").chars().take(500).collect::<String>()}),
        ),
        (Ok(_), Err(error)) => anyhow::bail!("{name}: isolated operation failed: {error:#}"),
        (Err(error), Ok(_)) => anyhow::bail!("{name}: only direct operation failed: {error:#}"),
    }
}

#[derive(Serialize)]
struct Image {
    width: u32,
    height: u32,
    viewport: pdf_diff::PageViewport,
    png_sha256: String,
}
fn image(page: RenderedPage) -> Image {
    Image {
        width: page.width,
        height: page.height,
        viewport: page.viewport,
        png_sha256: format!("{:x}", Sha256::digest(&page.png)),
    }
}
fn image_response(response: IsolatedResponse<RenderedPage>) -> IsolatedResponse<Image> {
    IsolatedResponse {
        value: image(response.value),
        exit_code: response.exit_code,
        stderr: response.stderr,
        execution: response.execution,
        staging: response.staging,
    }
}

fn main() -> Result<()> {
    let args = std::env::args_os()
        .skip(1)
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    ensure!(
        args.len() == 4,
        "usage: audit_isolated_bytes <pdf-diff> <manifest> <cache> <private-temp-root>"
    );
    let manifest: Manifest = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    let client = IsolatedPdfClient::new(&args[0]).with_temporary_directory(&args[3]);
    let count = manifest.documents.len();
    for (index, entry) in manifest
        .documents
        .into_iter()
        .chain(manifest.robustness)
        .enumerate()
    {
        let bytes = std::fs::read(args[2].join(format!("{}.pdf", entry.id)))?;
        ensure!(
            bytes.len() == entry.bytes && format!("{:x}", Sha256::digest(&bytes)) == entry.sha256,
            "Unverified public input {}",
            entry.id
        );
        let expected_rejection = matches!(
            entry.outcome.as_deref(),
            Some("geometry_rejected" | "password_required")
        );
        let direct = pdf_diff::load_document_from_bytes(&bytes, &Limits::default());
        ensure!(
            direct.is_err() == expected_rejection,
            "{}: unexpected IR outcome",
            entry.id
        );
        if let Ok(ir) = &direct {
            ensure!(
                Some(ir.pages.len()) == entry.pages,
                "Unexpected public page count"
            );
        }
        let pages = direct.as_ref().map(|ir| ir.pages.len()).unwrap_or(0);
        let uncertain_empty_layers = direct
            .as_ref()
            .ok()
            .map(|ir| {
                ir.pages
                    .iter()
                    .filter(|p| p.blocks.is_empty() && p.sdk_text_objects_without_characters > 0)
                    .map(|p| format!("text_layer_{}", p.index))
                    .collect::<std::collections::BTreeSet<_>>()
            })
            .unwrap_or_default();
        let mut operations = vec![matched(
            "inspect",
            direct,
            client.inspect_bytes(&bytes, &Limits::default()),
        )?];
        let selected = if pages > 0 {
            (1..=pages).collect::<Vec<_>>()
        } else {
            vec![entry
                .invalid_page
                .context("Rejected source has no declared invalid page")?]
        };
        for &page in &selected {
            let name = format!("text_layer_{page}");
            operations.push(matched(
                &name,
                pdf_diff::text_layer_from_bytes(&bytes, page),
                client.text_layer_bytes(&bytes, page),
            )?);
        }
        let page = entry.invalid_page.unwrap_or(1);
        operations.push(matched(
            "render_width",
            pdf_diff::render_page_from_bytes(&bytes, page, 400).map(image),
            client.render_bytes(&bytes, page, 400).map(image_response),
        )?);
        operations.push(matched(
            "render_dpi",
            pdf_diff::render_page_at_dpi_from_bytes(&bytes, page, 72).map(image),
            client
                .render_bytes_at_dpi(&bytes, page, 72)
                .map(image_response),
        )?);
        let mut rewritten = bytes.clone();
        rewritten.extend_from_slice(b"\n% isolated byte representation rewrite\n");
        let options = DiffOptions::default();
        operations.push(matched(
            "compare",
            pdf_diff::diff_bytes(
                "original",
                "rewrite",
                &bytes,
                &rewritten,
                &Limits::default(),
                &options,
            ),
            client.diff_bytes(
                "original",
                "rewrite",
                &bytes,
                &rewritten,
                &Limits::default(),
                &options,
            ),
        )?);
        ensure!(
            std::fs::read_dir(&args[3])?.count() == 0,
            "Private byte staging was retained"
        );
        if index < count {
            ensure!(
                operations
                    .iter()
                    .filter(|o| o["operation"] != "render_dpi"
                        && !uncertain_empty_layers.contains(o["operation"].as_str().unwrap_or("")))
                    .all(|o| o["status"] == "matched"),
                "Ordinary public operation was rejected"
            );
            for operation in &operations {
                if operation["status"] == "both_rejected" {
                    ensure!(
                        (operation["operation"] == "render_dpi"
                            && operation["direct_error"]
                                .as_str()
                                .unwrap_or("")
                                .contains("rendering limit"))
                            || (uncertain_empty_layers
                                .contains(operation["operation"].as_str().unwrap_or(""))
                                && operation["direct_error"]
                                    .as_str()
                                    .unwrap_or("")
                                    .contains("no character records")
                                && operation["isolated_error"]
                                    .as_str()
                                    .unwrap_or("")
                                    .contains("no character records")),
                        "Unexpected normal-source rejection"
                    );
                }
            }
        }
        println!(
            "{}",
            serde_json::json!({"id":entry.id,"passed":true,"normal_document":index<count,"pages":pages,"uncertain_empty_text_layers":uncertain_empty_layers,"operations":operations,"private_directory_empty":true})
        );
    }
    Ok(())
}
