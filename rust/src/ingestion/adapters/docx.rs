use std::io::{BufReader, Cursor, Read};
use std::path::Path;

use anyhow::Context;
use quick_xml::events::Event;
use quick_xml::Reader;
use quick_xml::XmlVersion;

use super::{DocumentAdapter, ParsedDocument, Section};

/// Adapter for DOCX (Office Open XML) files.
///
/// The DOCX format is a ZIP archive containing `word/document.xml`.  Each
/// `<w:p>` is a paragraph; its style (`w:pStyle` val attribute) tells us
/// whether it is a heading.
pub struct DocxAdapter;

impl DocumentAdapter for DocxAdapter {
    fn can_handle(&self, path: &Path, mime_type: &str) -> bool {
        let docx_mime = mime_type
            == "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
        let docx_ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("docx"))
            .unwrap_or(false);
        docx_mime || docx_ext
    }

    fn parse(&self, path: &Path) -> anyhow::Result<ParsedDocument> {
        let source = path.to_string_lossy().into_owned();

        let file = std::fs::File::open(path)
            .with_context(|| format!("Cannot open DOCX file: {}", source))?;
        let mut archive = zip::ZipArchive::new(file)
            .with_context(|| format!("Cannot read ZIP in DOCX: {}", source))?;

        // ------------------------------------------------------------------
        // Try to extract title from docProps/core.xml
        // ------------------------------------------------------------------
        let title = extract_core_title(&mut archive);

        // ------------------------------------------------------------------
        // Parse word/document.xml
        // ------------------------------------------------------------------
        let xml_bytes = {
            let mut entry = archive
                .by_name("word/document.xml")
                .context("word/document.xml not found in DOCX")?;
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf)?;
            buf
        };

        let sections = parse_document_xml(&xml_bytes)?;

        Ok(ParsedDocument {
            source,
            mime_type:
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
                    .to_string(),
            sections,
            title,
            metadata: serde_json::json!({}),
        })
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Heading style names that DOCX uses (case-insensitive prefix match is fine).
fn style_to_level(style: &str) -> Option<u32> {
    match style {
        s if s.eq_ignore_ascii_case("Title") => Some(1),
        s if s.eq_ignore_ascii_case("Subtitle") => Some(2),
        s if s.eq_ignore_ascii_case("Heading1") => Some(1),
        s if s.eq_ignore_ascii_case("Heading2") => Some(2),
        s if s.eq_ignore_ascii_case("Heading3") => Some(3),
        s if s.eq_ignore_ascii_case("Heading4") => Some(4),
        s if s.eq_ignore_ascii_case("Heading5") => Some(5),
        s if s.eq_ignore_ascii_case("Heading6") => Some(6),
        // Some generators emit "heading 1" etc.
        s if s.len() > 8 && s[..8].eq_ignore_ascii_case("heading ") => {
            s[8..].trim().parse::<u32>().ok()
        }
        _ => None,
    }
}

/// Parse the raw bytes of `word/document.xml` into a list of sections.
fn parse_document_xml(xml_bytes: &[u8]) -> anyhow::Result<Vec<Section>> {
    let cursor = Cursor::new(xml_bytes);
    let buf_reader = BufReader::new(cursor);
    let mut reader = Reader::from_reader(buf_reader);
    reader.config_mut().trim_text(true);

    let mut sections: Vec<Section> = Vec::new();

    // Per-paragraph state
    let mut in_para = false;
    let mut para_style: Option<String> = None;
    let mut para_text = String::new();

    // Per-run state
    let mut depth: u32 = 0;
    let mut buf = Vec::new();

    // Tracks the w:p nesting depth so we know when we truly exit a paragraph
    let mut para_depth: u32 = 0;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                depth += 1;
                let local = local_name(e.name().into_inner());
                match local {
                    b"p" => {
                        in_para = true;
                        para_style = None;
                        para_text.clear();
                        para_depth = depth;
                    }
                    b"pStyle" if in_para => {
                        // <w:pStyle w:val="Heading1"/>  — pick up the val attribute
                        for attr in e.attributes().flatten() {
                            if local_name(attr.key.into_inner()) == b"val" {
                                if let Ok(v) = attr.normalized_value(XmlVersion::Implicit1_0) {
                                    para_style = Some(v.into_owned());
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(ref e)) => {
                // Self-closing tags — handle <w:pStyle w:val="…"/>
                let local = local_name(e.name().into_inner());
                if local == b"pStyle" && in_para {
                    for attr in e.attributes().flatten() {
                        if local_name(attr.key.into_inner()) == b"val" {
                            if let Ok(v) = attr.normalized_value(XmlVersion::Implicit1_0) {
                                para_style = Some(v.into_owned());
                            }
                        }
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                let local = local_name(e.name().into_inner());
                if local == b"p" && in_para && depth == para_depth {
                    // End of the paragraph we are tracking
                    in_para = false;
                    let text = para_text.trim().to_string();
                    if !text.is_empty() {
                        let level_opt = para_style
                            .as_deref()
                            .and_then(style_to_level);
                        if let Some(level) = level_opt {
                            // This paragraph is a heading — start a new section
                            sections.push(Section {
                                content: String::new(),
                                heading: Some(text),
                                level,
                                page_number: None,
                            });
                        } else {
                            // Regular paragraph — append to the current section
                            if let Some(last) = sections.last_mut() {
                                if !last.content.is_empty() {
                                    last.content.push('\n');
                                }
                                last.content.push_str(&text);
                            } else {
                                // No section yet — create an implicit flat one
                                sections.push(Section {
                                    content: text,
                                    heading: None,
                                    level: 0,
                                    page_number: None,
                                });
                            }
                        }
                    }
                    para_style = None;
                }
                if depth > 0 {
                    depth -= 1;
                }
            }
            Ok(Event::Text(ref e)) if in_para => {
                if let Ok(t) = e.decode() {
                    para_text.push_str(t.as_ref());
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(anyhow::anyhow!("XML parse error: {}", e)),
            _ => {}
        }
        buf.clear();
    }

    Ok(sections)
}

/// Try to read the document title from `docProps/core.xml`.
fn extract_core_title<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> Option<String> {
    let xml_bytes = {
        let mut entry = archive.by_name("docProps/core.xml").ok()?;
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf).ok()?;
        buf
    };

    let cursor = Cursor::new(xml_bytes);
    let buf_reader = BufReader::new(cursor);
    let mut reader = Reader::from_reader(buf_reader);
    reader.config_mut().trim_text(true);

    let mut in_title = false;
    let mut xml_buf = Vec::new();

    loop {
        match reader.read_event_into(&mut xml_buf) {
            Ok(Event::Start(ref e)) => {
                if local_name(e.name().into_inner()) == b"title" {
                    in_title = true;
                }
            }
            Ok(Event::Text(ref e)) if in_title => {
                if let Ok(t) = e.decode() {
                    let title = t.trim().to_string();
                    if !title.is_empty() {
                        return Some(title);
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                if local_name(e.name().into_inner()) == b"title" {
                    in_title = false;
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        xml_buf.clear();
    }
    None
}

/// Strip the XML namespace prefix and return just the local part as bytes.
/// e.g. `w:p` → `p`,  `p` → `p`
fn local_name(name: &[u8]) -> &[u8] {
    if let Some(pos) = name.iter().rposition(|&b| b == b':') {
        &name[pos + 1..]
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::super::DocumentAdapter;
    use super::DocxAdapter;
    use std::path::Path;

    #[test]
    fn can_handle_docx_extension() {
        let adapter = DocxAdapter;
        assert!(adapter.can_handle(
            Path::new("file.docx"),
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        ));
        assert!(adapter.can_handle(Path::new("file.docx"), ""));
        assert!(!adapter.can_handle(Path::new("file.pdf"), "application/pdf"));
    }

    #[test]
    fn style_heading_detection() {
        use super::style_to_level;
        assert_eq!(style_to_level("Heading1"), Some(1));
        assert_eq!(style_to_level("Heading2"), Some(2));
        assert_eq!(style_to_level("heading 3"), Some(3));
        assert_eq!(style_to_level("Title"), Some(1));
        assert_eq!(style_to_level("Normal"), None);
        assert_eq!(style_to_level(""), None);
    }
}
