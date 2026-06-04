use std::path::Path;

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use super::{DocumentAdapter, ParsedDocument, Section};

/// Adapter for Markdown files.  Uses `pulldown-cmark` to split the document
/// on headings, preserving heading text and depth.
pub struct MarkdownAdapter;

impl DocumentAdapter for MarkdownAdapter {
    fn can_handle(&self, path: &Path, mime_type: &str) -> bool {
        let has_md_ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| matches!(e.to_ascii_lowercase().as_str(), "md" | "markdown" | "mdown"))
            .unwrap_or(false);
        let has_md_mime = mime_type == "text/markdown" || mime_type == "text/x-markdown";
        has_md_ext || has_md_mime
    }

    fn parse(&self, path: &Path) -> anyhow::Result<ParsedDocument> {
        let source = path.to_string_lossy().into_owned();
        let raw = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Cannot read markdown file {}: {}", source, e))?;

        let sections = split_by_headings(&raw);
        let title = sections
            .iter()
            .find(|s| s.level == 1)
            .and_then(|s| s.heading.clone());

        Ok(ParsedDocument {
            source,
            mime_type: "text/markdown".to_string(),
            sections,
            title,
            metadata: serde_json::json!({}),
        })
    }
}

/// Split a Markdown string into sections, one per heading.  Content that
/// precedes the first heading is returned as a section with `level = 0`.
///
/// This function is public so it can be unit-tested directly.
pub fn split_by_headings(markdown: &str) -> Vec<Section> {
    if markdown.trim().is_empty() {
        return Vec::new();
    }

    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);

    let parser = Parser::new_ext(markdown, options);

    struct PendingSection {
        heading: Option<String>,
        level: u32,
        content_parts: Vec<String>,
    }

    let mut sections: Vec<Section> = Vec::new();
    let mut current: PendingSection = PendingSection {
        heading: None,
        level: 0,
        content_parts: Vec::new(),
    };

    // State for collecting heading text
    let mut in_heading: bool = false;
    let mut heading_level: u32 = 0;
    let mut heading_parts: Vec<String> = Vec::new();

    let flush = |current: &mut PendingSection, sections: &mut Vec<Section>| {
        let content = current.content_parts.join("").trim().to_string();
        // Only emit if there is content OR a heading
        if !content.is_empty() || current.heading.is_some() {
            sections.push(Section {
                content,
                heading: current.heading.take(),
                level: current.level,
                page_number: None,
            });
        }
        current.content_parts.clear();
    };

    for event in parser {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                // Flush the previous section before starting a new one
                flush(&mut current, &mut sections);
                in_heading = true;
                heading_level = heading_level_to_u32(level);
                heading_parts.clear();
            }
            Event::End(TagEnd::Heading(_)) => {
                in_heading = false;
                let heading_text = heading_parts.join("").trim().to_string();
                current.heading = if heading_text.is_empty() { None } else { Some(heading_text) };
                current.level = heading_level;
                heading_parts.clear();
            }
            Event::Text(text) => {
                if in_heading {
                    heading_parts.push(text.into_string());
                } else {
                    current.content_parts.push(text.into_string());
                }
            }
            Event::Code(code) => {
                if !in_heading {
                    current.content_parts.push(code.into_string());
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if !in_heading {
                    current.content_parts.push("\n".to_string());
                }
            }
            _ => {}
        }
    }

    // Flush the last section
    flush(&mut current, &mut sections);

    sections
}

fn heading_level_to_u32(level: HeadingLevel) -> u32 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

#[cfg(test)]
mod tests {
    use super::{split_by_headings, MarkdownAdapter, DocumentAdapter};
    use std::path::Path;

    #[test]
    fn split_markdown_by_headings() {
        let md = "# Title\n\nIntro paragraph.\n\n## Section 1\n\nContent here.\n\n## Section 2\n\nMore content.";
        let sections = split_by_headings(md);
        assert_eq!(sections.len(), 3);
        assert_eq!(sections[0].heading.as_deref(), Some("Title"));
        assert_eq!(sections[0].level, 1);
        assert_eq!(sections[1].heading.as_deref(), Some("Section 1"));
        assert!(sections[1].content.contains("Content here"));
        assert_eq!(sections[2].heading.as_deref(), Some("Section 2"));
        assert!(sections[2].content.contains("More content"));
    }

    #[test]
    fn empty_markdown_returns_empty() {
        assert!(split_by_headings("   ").is_empty());
    }

    #[test]
    fn content_before_first_heading() {
        let md = "Some intro text.\n\n# Heading\n\nBody.";
        let sections = split_by_headings(md);
        // First section has no heading, second has "Heading"
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].heading, None);
        assert_eq!(sections[0].level, 0);
        assert!(sections[0].content.contains("intro"));
        assert_eq!(sections[1].heading.as_deref(), Some("Heading"));
    }

    #[test]
    fn heading_only_document() {
        let md = "# Just a title";
        let sections = split_by_headings(md);
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].heading.as_deref(), Some("Just a title"));
    }

    #[test]
    fn nested_headings() {
        let md = "# H1\n\ntext1\n\n## H2\n\ntext2\n\n### H3\n\ntext3";
        let sections = split_by_headings(md);
        assert_eq!(sections.len(), 3);
        assert_eq!(sections[0].level, 1);
        assert_eq!(sections[1].level, 2);
        assert_eq!(sections[2].level, 3);
    }

    #[test]
    fn does_not_handle_plain_text_files() {
        let adapter = MarkdownAdapter;
        assert!(!adapter.can_handle(Path::new("file.txt"), "text/plain"));
        assert!(adapter.can_handle(Path::new("file.md"), "text/plain")); // md extension wins
        assert!(adapter.can_handle(Path::new("file.md"), "text/markdown"));
    }
}
