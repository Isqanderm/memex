use std::path::Path;

use super::{DocumentAdapter, ParsedDocument, Section};

/// Adapter for plain-text files.  Reads the whole file as a single section.
pub struct TextAdapter;

impl DocumentAdapter for TextAdapter {
    fn can_handle(&self, path: &Path, mime_type: &str) -> bool {
        mime_type.starts_with("text/")
            || path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| matches!(e.to_ascii_lowercase().as_str(), "txt" | "log" | "text"))
                .unwrap_or(false)
    }

    fn parse(&self, path: &Path) -> anyhow::Result<ParsedDocument> {
        let source = path.to_string_lossy().into_owned();
        let content = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Cannot read text file {}: {}", source, e))?;

        let sections = if content.trim().is_empty() {
            Vec::new()
        } else {
            vec![Section {
                content: content.trim().to_string(),
                heading: None,
                level: 0,
                page_number: None,
            }]
        };

        Ok(ParsedDocument {
            source,
            mime_type: "text/plain".to_string(),
            sections,
            title: None,
            metadata: serde_json::json!({}),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::DocumentAdapter;
    use super::TextAdapter;
    use std::io::Write;
    use std::path::Path;

    #[test]
    fn can_handle_txt_extension() {
        let adapter = TextAdapter;
        assert!(adapter.can_handle(Path::new("notes.txt"), "text/plain"));
        assert!(adapter.can_handle(Path::new("notes.txt"), ""));
        assert!(adapter.can_handle(Path::new("log.log"), ""));
    }

    #[test]
    fn parses_simple_text_file() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        write!(tmp, "Hello world\nSecond line").unwrap();
        let adapter = TextAdapter;
        let doc = adapter.parse(tmp.path()).unwrap();
        assert_eq!(doc.sections.len(), 1);
        assert!(doc.sections[0].content.contains("Hello world"));
    }

    #[test]
    fn empty_text_file_returns_no_sections() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        write!(tmp, "   ").unwrap();
        let adapter = TextAdapter;
        let doc = adapter.parse(tmp.path()).unwrap();
        assert!(doc.sections.is_empty());
    }
}
