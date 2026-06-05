use std::path::Path;

use anyhow::Context;

use super::{DocumentAdapter, ParsedDocument, Section};

pub struct EpubAdapter;

impl DocumentAdapter for EpubAdapter {
    fn can_handle(&self, path: &Path, _mime_type: &str) -> bool {
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("epub"))
            .unwrap_or(false)
    }

    fn parse(&self, path: &Path) -> anyhow::Result<ParsedDocument> {
        let source = path.to_string_lossy().into_owned();
        let mut doc = epub::doc::EpubDoc::new(path)
            .with_context(|| format!("Cannot open EPUB: {source}"))?;

        let title = doc.get_title();

        let spine_ids: Vec<String> = doc.spine.iter().map(|s| s.idref.clone()).collect();
        let mut sections = Vec::new();

        for (idx, id) in spine_ids.iter().enumerate() {
            if let Some((content, _mime)) = doc.get_resource(id) {
                let text = strip_html(&String::from_utf8_lossy(&content));
                let trimmed = text.trim().to_string();
                if !trimmed.is_empty() {
                    sections.push(Section {
                        content: trimmed,
                        heading: None,
                        level: 0,
                        page_number: Some(idx as u32 + 1),
                    });
                }
            }
        }

        Ok(ParsedDocument {
            source,
            mime_type: "application/epub+zip".to_string(),
            sections,
            title,
            metadata: serde_json::json!({}),
        })
    }
}

fn strip_html(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }
    result
}
