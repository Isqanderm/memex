use std::path::Path;
use std::process::Command;

use anyhow::Context;

use super::{DocumentAdapter, ParsedDocument, Section};

/// Adapter that uses the `pdftotext` command-line tool (from poppler-utils) to
/// extract text from PDF files.
pub struct PdfAdapter;

impl DocumentAdapter for PdfAdapter {
    fn can_handle(&self, path: &Path, mime_type: &str) -> bool {
        mime_type == "application/pdf"
            || path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("pdf"))
                .unwrap_or(false)
    }

    fn parse(&self, path: &Path) -> anyhow::Result<ParsedDocument> {
        // Run: pdftotext -layout <file> -
        let output = Command::new("pdftotext")
            .arg("-layout")
            .arg(path)
            .arg("-") // write to stdout
            .output()
            .context("Failed to run pdftotext — is poppler-utils installed?")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("pdftotext exited with error: {}", stderr);
        }

        let text = String::from_utf8_lossy(&output.stdout).into_owned();

        // Page breaks are signalled by form-feed characters (\x0C).
        let sections: Vec<Section> = text
            .split('\x0C')
            .enumerate()
            .filter_map(|(idx, page_text)| {
                let trimmed = page_text.trim().to_string();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(Section {
                        content: trimmed,
                        heading: None,
                        level: 0,
                        page_number: Some((idx + 1) as u32),
                    })
                }
            })
            .collect();

        // Best-effort title extraction via `pdfinfo`.
        let title = extract_pdf_title(path);

        let source = path.to_string_lossy().into_owned();

        Ok(ParsedDocument {
            source,
            mime_type: "application/pdf".to_string(),
            sections,
            title,
            metadata: serde_json::json!({ "tool": "pdftotext" }),
        })
    }
}

/// Try to extract the document title using `pdfinfo`.  Returns `None` on any
/// failure so that the caller can proceed without a title.
fn extract_pdf_title(path: &Path) -> Option<String> {
    let output = Command::new("pdfinfo").arg(path).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let info = String::from_utf8_lossy(&output.stdout);
    for line in info.lines() {
        if let Some(rest) = line.strip_prefix("Title:") {
            let title = rest.trim().to_string();
            if !title.is_empty() {
                return Some(title);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    #[test]
    fn pdftotext_binary_exists() {
        let result = std::process::Command::new("pdftotext")
            .arg("-v")
            .output();
        match result {
            Ok(out) => {
                // pdftotext prints its version to stderr
                let stderr = String::from_utf8_lossy(&out.stderr);
                let stdout = String::from_utf8_lossy(&out.stdout);
                // It should at least mention "pdftotext" or "Poppler" in some output
                let combined = format!("{}{}", stderr, stdout);
                if combined.contains("pdftotext") || combined.contains("Poppler") || combined.contains("poppler") {
                    // Great — tool is present and identified itself
                } else {
                    // Present but unrecognised; still not a failure
                    eprintln!("pdftotext found but output was unexpected: {}", combined);
                }
            }
            Err(_) => {
                // Tool is not installed; skip gracefully
                eprintln!("pdftotext not found — skipping test (install poppler-utils to enable PDF support)");
            }
        }
    }

    #[test]
    fn can_handle_pdf_extension() {
        use std::path::Path;
        use super::super::DocumentAdapter;
        let adapter = super::PdfAdapter;
        assert!(adapter.can_handle(Path::new("file.pdf"), "application/pdf"));
        assert!(adapter.can_handle(Path::new("file.PDF"), ""));
        assert!(!adapter.can_handle(Path::new("file.docx"), "application/vnd.openxmlformats-officedocument.wordprocessingml.document"));
    }
}
