use std::io::{BufReader, Cursor, Read};
use std::path::Path;

use anyhow::Context;
use quick_xml::events::Event;
use quick_xml::Reader;

use super::{DocumentAdapter, ParsedDocument, Section};

/// Adapter for PPTX (Office Open XML Presentation) files.
///
/// Each slide in `ppt/slides/slideN.xml` becomes one section with heading
/// "Slide N".  Text is extracted from all `<a:t>` elements.
pub struct PptxAdapter;

impl DocumentAdapter for PptxAdapter {
    fn can_handle(&self, path: &Path, mime_type: &str) -> bool {
        let pptx_mime = mime_type
            == "application/vnd.openxmlformats-officedocument.presentationml.presentation";
        let pptx_ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("pptx"))
            .unwrap_or(false);
        pptx_mime || pptx_ext
    }

    fn parse(&self, path: &Path) -> anyhow::Result<ParsedDocument> {
        let source = path.to_string_lossy().into_owned();

        let file = std::fs::File::open(path)
            .with_context(|| format!("Cannot open PPTX file: {}", source))?;
        let mut archive = zip::ZipArchive::new(file)
            .with_context(|| format!("Cannot read ZIP in PPTX: {}", source))?;

        // Discover how many slides exist by probing ppt/slides/slideN.xml
        let mut slide_indices: Vec<usize> = Vec::new();
        for n in 1..=1000 {
            let slide_path = format!("ppt/slides/slide{}.xml", n);
            // Check by iterating names — ZipArchive::by_name does mutable borrow
            // so we use file_names() for the check pass.
            let exists = (0..archive.len()).any(|i| {
                // SAFETY: index is in range
                archive.by_index(i).map(|f| f.name() == slide_path).unwrap_or(false)
            });
            if exists {
                slide_indices.push(n);
            } else {
                break; // slides are numbered consecutively
            }
        }

        let mut sections = Vec::new();
        for n in slide_indices {
            let slide_path = format!("ppt/slides/slide{}.xml", n);
            let xml_bytes = {
                let mut entry = archive
                    .by_name(&slide_path)
                    .with_context(|| format!("Cannot read {} in PPTX", slide_path))?;
                let mut buf = Vec::new();
                entry.read_to_end(&mut buf)?;
                buf
            };

            let text = extract_text_from_slide_xml(&xml_bytes)?;
            sections.push(Section {
                content: text,
                heading: Some(format!("Slide {}", n)),
                level: 1,
                page_number: Some(n as u32),
            });
        }

        Ok(ParsedDocument {
            source,
            mime_type:
                "application/vnd.openxmlformats-officedocument.presentationml.presentation"
                    .to_string(),
            sections,
            title: None,
            metadata: serde_json::json!({}),
        })
    }
}

/// Extract all text nodes from a slide XML file.
fn extract_text_from_slide_xml(xml_bytes: &[u8]) -> anyhow::Result<String> {
    let cursor = Cursor::new(xml_bytes);
    let buf_reader = BufReader::new(cursor);
    let mut reader = Reader::from_reader(buf_reader);
    reader.config_mut().trim_text(false);

    let mut parts: Vec<String> = Vec::new();
    let mut buf = Vec::new();
    let mut in_text = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                // a:t  — DrawingML text run
                if local_name(e.name().into_inner()) == b"t" {
                    in_text = true;
                }
            }
            Ok(Event::End(ref e)) => {
                if local_name(e.name().into_inner()) == b"t" {
                    in_text = false;
                }
            }
            Ok(Event::Text(ref e)) if in_text => {
                if let Ok(t) = e.decode() {
                    let s = t.trim().to_string();
                    if !s.is_empty() {
                        parts.push(s);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(anyhow::anyhow!("XML parse error in slide: {}", e)),
            _ => {}
        }
        buf.clear();
    }

    Ok(parts.join(" "))
}

/// Strip the XML namespace prefix, returning just the local name bytes.
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
    use super::PptxAdapter;
    use std::path::Path;

    #[test]
    fn can_handle_pptx_extension() {
        let adapter = PptxAdapter;
        assert!(adapter.can_handle(
            Path::new("slides.pptx"),
            "application/vnd.openxmlformats-officedocument.presentationml.presentation"
        ));
        assert!(adapter.can_handle(Path::new("slides.pptx"), ""));
        assert!(!adapter.can_handle(Path::new("doc.docx"), ""));
    }
}
