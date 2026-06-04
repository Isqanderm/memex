# Task 6: Document Adapters

**Goal:** Парсеры документов для всех форматов: PDF (pdftotext), DOCX (zip+xml), Markdown (pulldown-cmark), Text, XLSX (calamine), PPTX (zip+xml). Аналог Python адаптеров.

**Files:**
- Create: `rust/src/ingestion/adapters/mod.rs`
- Create: `rust/src/ingestion/adapters/pdf.rs`
- Create: `rust/src/ingestion/adapters/docx.rs`
- Create: `rust/src/ingestion/adapters/markdown.rs`
- Create: `rust/src/ingestion/adapters/text.rs`
- Create: `rust/src/ingestion/adapters/xlsx.rs`
- Create: `rust/src/ingestion/adapters/pptx.rs`

---

### Общие типы

- [ ] **Шаг 1: Создать rust/src/ingestion/adapters/mod.rs**

```rust
use std::path::Path;

/// Секция документа с текстом и метаданными.
#[derive(Debug, Clone)]
pub struct Section {
    pub content: String,
    pub heading: Option<String>,
    pub level: u32,       // 0=flat, 1=h1, 2=h2, 3=h3
    pub page_number: Option<u32>,
}

/// Разобранный документ.
#[derive(Debug)]
pub struct ParsedDocument {
    pub source: String,
    pub mime_type: String,
    pub sections: Vec<Section>,
    pub title: Option<String>,
    pub metadata: serde_json::Value,
}

/// Трейт документ-адаптера.
pub trait DocumentAdapter: Send + Sync {
    fn can_handle(&self, path: &Path, mime_type: &str) -> bool;
    fn parse(&self, path: &Path) -> anyhow::Result<ParsedDocument>;
}

/// Реестр адаптеров — пробует каждый по порядку.
pub struct AdapterRegistry {
    adapters: Vec<Box<dyn DocumentAdapter>>,
}

impl AdapterRegistry {
    pub fn new() -> Self {
        Self { adapters: vec![] }
    }

    pub fn register(&mut self, adapter: impl DocumentAdapter + 'static) {
        self.adapters.push(Box::new(adapter));
    }

    pub fn parse(&self, path: &Path, mime_type: &str) -> anyhow::Result<ParsedDocument> {
        for adapter in &self.adapters {
            if adapter.can_handle(path, mime_type) {
                return adapter.parse(path);
            }
        }
        anyhow::bail!("no adapter for mime_type={mime_type} path={}", path.display())
    }
}

pub fn build_default_registry() -> AdapterRegistry {
    let mut r = AdapterRegistry::new();
    r.register(pdf::PdfAdapter);
    r.register(docx::DocxAdapter);
    r.register(xlsx::XlsxAdapter);
    r.register(pptx::PptxAdapter);
    r.register(markdown::MarkdownAdapter);
    r.register(text::TextAdapter);
    r
}

pub mod pdf;
pub mod docx;
pub mod markdown;
pub mod text;
pub mod xlsx;
pub mod pptx;
```

---

### Task 6.1: PDF Adapter (pdftotext)

- [ ] **Шаг 1: Написать тест (требует poppler-utils)**

```rust
// rust/src/ingestion/adapters/pdf.rs
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    // Этот тест требует poppler-utils (pdftotext) и реальный PDF.
    // Запускать только если есть тестовый PDF.
    // Для юнит-теста проверяем what_command_exists().
    #[test]
    fn pdftotext_binary_exists() {
        // На RPi: sudo apt install poppler-utils
        let result = std::process::Command::new("pdftotext")
            .arg("-v")
            .output();
        assert!(result.is_ok(), "pdftotext not found — install: sudo apt install poppler-utils");
    }
}
```

- [ ] **Шаг 2: Реализовать PDF адаптер через subprocess**

```rust
use std::path::Path;
use std::process::Command;
use super::{DocumentAdapter, ParsedDocument, Section};

pub struct PdfAdapter;

impl DocumentAdapter for PdfAdapter {
    fn can_handle(&self, path: &Path, mime_type: &str) -> bool {
        mime_type == "application/pdf"
            || path.extension().map_or(false, |e| e == "pdf")
    }

    fn parse(&self, path: &Path) -> anyhow::Result<ParsedDocument> {
        // pdftotext -layout - : вывод в stdout, один блок на страницу
        let output = Command::new("pdftotext")
            .args(["-layout", path.to_str().unwrap(), "-"])
            .output()
            .map_err(|e| anyhow::anyhow!("pdftotext not found: {e}. Install: sudo apt install poppler-utils"))?;

        if !output.status.success() {
            anyhow::bail!("pdftotext failed: {}", String::from_utf8_lossy(&output.stderr));
        }

        let full_text = String::from_utf8_lossy(&output.stdout).to_string();

        // Разбиваем по form feed (\x0C) — pdftotext вставляет его между страницами
        let sections: Vec<Section> = full_text
            .split('\x0C')
            .enumerate()
            .filter_map(|(page_idx, page_text)| {
                let trimmed = page_text.trim().to_string();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(Section {
                        content: trimmed,
                        heading: None,
                        level: 0,
                        page_number: Some((page_idx + 1) as u32),
                    })
                }
            })
            .collect();

        if sections.is_empty() {
            return Ok(ParsedDocument {
                source: path.to_string_lossy().to_string(),
                mime_type: "application/pdf".to_string(),
                sections: vec![Section { content: String::new(), heading: None, level: 0, page_number: None }],
                title: None,
                metadata: serde_json::json!({}),
            });
        }

        // Попытка извлечь заголовок через pdfinfo
        let title = extract_pdf_title(path);

        Ok(ParsedDocument {
            source: path.to_string_lossy().to_string(),
            mime_type: "application/pdf".to_string(),
            sections,
            title,
            metadata: serde_json::json!({}),
        })
    }
}

fn extract_pdf_title(path: &Path) -> Option<String> {
    let output = Command::new("pdfinfo")
        .arg(path)
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        if let Some(title) = line.strip_prefix("Title:") {
            let t = title.trim().to_string();
            if !t.is_empty() {
                return Some(t);
            }
        }
    }
    None
}
```

---

### Task 6.2: DOCX Adapter

- [ ] **Шаг 1: Написать тест**

```rust
// rust/src/ingestion/adapters/docx.rs
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn can_handle_docx_extension() {
        let adapter = DocxAdapter;
        assert!(adapter.can_handle(Path::new("file.docx"), "application/vnd.openxmlformats-officedocument.wordprocessingml.document"));
        assert!(!adapter.can_handle(Path::new("file.pdf"), "application/pdf"));
    }

    // Тест с реальным файлом: tests/fixtures/sample.docx
    // #[test]
    // fn parse_sample_docx() { ... }
}
```

- [ ] **Шаг 2: Реализовать DOCX адаптер**

```rust
use std::io::{BufReader, Read};
use std::path::Path;
use quick_xml::events::Event;
use quick_xml::Reader;
use zip::ZipArchive;
use super::{DocumentAdapter, ParsedDocument, Section};

pub struct DocxAdapter;

const DOCX_MIME: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";

impl DocumentAdapter for DocxAdapter {
    fn can_handle(&self, path: &Path, mime_type: &str) -> bool {
        mime_type == DOCX_MIME
            || path.extension().map_or(false, |e| e == "docx")
    }

    fn parse(&self, path: &Path) -> anyhow::Result<ParsedDocument> {
        let file = std::fs::File::open(path)?;
        let mut archive = ZipArchive::new(BufReader::new(file))?;

        // word/document.xml содержит основной текст
        let xml_content = {
            let mut f = archive
                .by_name("word/document.xml")
                .map_err(|_| anyhow::anyhow!("word/document.xml not found in DOCX"))?;
            let mut buf = String::new();
            f.read_to_string(&mut buf)?;
            buf
        };

        let sections = parse_docx_xml(&xml_content)?;

        let title = extract_core_title(&mut archive);

        Ok(ParsedDocument {
            source: path.to_string_lossy().to_string(),
            mime_type: DOCX_MIME.to_string(),
            sections: if sections.is_empty() {
                vec![Section { content: String::new(), heading: None, level: 0, page_number: None }]
            } else {
                sections
            },
            title,
            metadata: serde_json::json!({}),
        })
    }
}

fn parse_docx_xml(xml: &str) -> anyhow::Result<Vec<Section>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut sections: Vec<Section> = vec![];
    let mut current_para_texts: Vec<String> = vec![];
    let mut current_text = String::new();
    let mut in_paragraph = false;
    let mut current_style: Option<String> = None;
    let mut buf = Vec::new();

    // Накопленные параграфы для текущей секции
    let mut section_paragraphs: Vec<String> = vec![];
    let mut current_heading: Option<String> = None;
    let mut current_level: u32 = 0;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let name = std::str::from_utf8(e.name().as_ref()).unwrap_or("").to_string();
                match name.as_str() {
                    "w:p" => {
                        in_paragraph = true;
                        current_para_texts.clear();
                        current_style = None;
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(ref e)) => {
                // w:pStyle даёт стиль параграфа
                if std::str::from_utf8(e.name().as_ref()).unwrap_or("") == "w:pStyle" {
                    if let Ok(Some(attr)) = e.try_get_attribute("w:val") {
                        current_style = Some(String::from_utf8_lossy(&attr.value).to_string());
                    }
                }
            }
            Ok(Event::Text(e)) => {
                if in_paragraph {
                    current_para_texts.push(e.unescape().unwrap_or_default().to_string());
                }
            }
            Ok(Event::End(ref e)) => {
                let name = std::str::from_utf8(e.name().as_ref()).unwrap_or("").to_string();
                if name == "w:p" && in_paragraph {
                    let para_text = current_para_texts.join("").trim().to_string();
                    let heading_level = heading_style_level(current_style.as_deref());

                    if let Some(level) = heading_level {
                        // Сохраняем предыдущую секцию если есть
                        if !section_paragraphs.is_empty() {
                            sections.push(Section {
                                content: section_paragraphs.join("\n"),
                                heading: current_heading.clone(),
                                level: current_level,
                                page_number: None,
                            });
                            section_paragraphs.clear();
                        }
                        current_heading = if para_text.is_empty() { None } else { Some(para_text) };
                        current_level = level;
                    } else if !para_text.is_empty() {
                        section_paragraphs.push(para_text);
                    }

                    in_paragraph = false;
                    current_para_texts.clear();
                    current_style = None;
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => anyhow::bail!("XML parse error: {e}"),
            _ => {}
        }
        buf.clear();
    }

    // Последняя секция
    if !section_paragraphs.is_empty() {
        sections.push(Section {
            content: section_paragraphs.join("\n"),
            heading: current_heading,
            level: current_level,
            page_number: None,
        });
    }

    Ok(sections)
}

fn heading_style_level(style: Option<&str>) -> Option<u32> {
    match style? {
        "Heading1" | "1" => Some(1),
        "Heading2" | "2" => Some(2),
        "Heading3" | "3" => Some(3),
        "Title"          => Some(1),
        "Subtitle"       => Some(2),
        _                => None,
    }
}

fn extract_core_title(archive: &mut ZipArchive<BufReader<std::fs::File>>) -> Option<String> {
    let mut f = archive.by_name("docProps/core.xml").ok()?;
    let mut xml = String::new();
    f.read_to_string(&mut xml).ok()?;

    let mut reader = Reader::from_str(&xml);
    let mut buf = Vec::new();
    let mut in_title = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                in_title = std::str::from_utf8(e.name().as_ref()).unwrap_or("") == "dc:title";
            }
            Ok(Event::Text(e)) if in_title => {
                let t = e.unescape().unwrap_or_default().trim().to_string();
                if !t.is_empty() {
                    return Some(t);
                }
            }
            Ok(Event::Eof) => break,
            _ => {}
        }
        buf.clear();
    }
    None
}
```

---

### Task 6.3: Markdown, Text, XLSX, PPTX адаптеры

- [ ] **Шаг 1: Реализовать markdown.rs**

```rust
use std::path::Path;
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag};
use super::{DocumentAdapter, ParsedDocument, Section};

pub struct MarkdownAdapter;

impl DocumentAdapter for MarkdownAdapter {
    fn can_handle(&self, path: &Path, mime_type: &str) -> bool {
        mime_type == "text/markdown"
            || path.extension().map_or(false, |e| e == "md" || e == "markdown")
    }

    fn parse(&self, path: &Path) -> anyhow::Result<ParsedDocument> {
        let content = std::fs::read_to_string(path)?;
        let sections = split_by_headings(&content);

        Ok(ParsedDocument {
            source: path.to_string_lossy().to_string(),
            mime_type: "text/markdown".to_string(),
            sections: if sections.is_empty() {
                vec![Section { content, heading: None, level: 0, page_number: None }]
            } else {
                sections
            },
            title: None,
            metadata: serde_json::json!({}),
        })
    }
}

pub fn split_by_headings(markdown: &str) -> Vec<Section> {
    let parser = Parser::new_ext(markdown, Options::all());
    let mut sections: Vec<Section> = vec![];
    let mut current_text = String::new();
    let mut current_heading: Option<String> = None;
    let mut current_level: u32 = 0;
    let mut in_heading = false;
    let mut heading_text = String::new();

    for event in parser {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                if !current_text.trim().is_empty() {
                    sections.push(Section {
                        content: current_text.trim().to_string(),
                        heading: current_heading.clone(),
                        level: current_level,
                        page_number: None,
                    });
                }
                current_text.clear();
                in_heading = true;
                heading_text.clear();
                current_level = heading_level_to_u32(level);
            }
            Event::End(Tag::Heading { .. }) => {
                current_heading = if heading_text.trim().is_empty() {
                    None
                } else {
                    Some(heading_text.trim().to_string())
                };
                in_heading = false;
            }
            Event::Text(t) | Event::Code(t) => {
                if in_heading {
                    heading_text.push_str(&t);
                } else {
                    current_text.push_str(&t);
                    current_text.push(' ');
                }
            }
            _ => {}
        }
    }

    if !current_text.trim().is_empty() {
        sections.push(Section {
            content: current_text.trim().to_string(),
            heading: current_heading,
            level: current_level,
            page_number: None,
        });
    }

    sections
}

fn heading_level_to_u32(level: HeadingLevel) -> u32 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        _ => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_markdown_by_headings() {
        let md = "# Title\n\nIntro paragraph.\n\n## Section 1\n\nContent here.\n\n## Section 2\n\nMore content.";
        let sections = split_by_headings(md);
        assert_eq!(sections.len(), 3);
        assert_eq!(sections[0].heading.as_deref(), Some("Title"));
        assert_eq!(sections[1].heading.as_deref(), Some("Section 1"));
        assert!(sections[1].content.contains("Content here"));
    }

    #[test]
    fn empty_markdown_returns_empty() {
        let sections = split_by_headings("   ");
        assert!(sections.is_empty());
    }
}
```

- [ ] **Шаг 2: Реализовать text.rs**

```rust
use std::path::Path;
use super::{DocumentAdapter, ParsedDocument, Section};

pub struct TextAdapter;

impl DocumentAdapter for TextAdapter {
    fn can_handle(&self, path: &Path, mime_type: &str) -> bool {
        mime_type.starts_with("text/")
            || path.extension().map_or(false, |e| e == "txt")
    }

    fn parse(&self, path: &Path) -> anyhow::Result<ParsedDocument> {
        let content = std::fs::read_to_string(path)?;
        Ok(ParsedDocument {
            source: path.to_string_lossy().to_string(),
            mime_type: "text/plain".to_string(),
            sections: vec![Section {
                content: content.trim().to_string(),
                heading: None,
                level: 0,
                page_number: None,
            }],
            title: None,
            metadata: serde_json::json!({}),
        })
    }
}
```

- [ ] **Шаг 3: Реализовать xlsx.rs**

```rust
use std::path::Path;
use calamine::{open_workbook_auto, Reader, DataType};
use super::{DocumentAdapter, ParsedDocument, Section};

pub struct XlsxAdapter;

impl DocumentAdapter for XlsxAdapter {
    fn can_handle(&self, path: &Path, mime_type: &str) -> bool {
        matches!(mime_type,
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
            | "application/vnd.ms-excel"
        ) || path.extension().map_or(false, |e| e == "xlsx" || e == "xls")
    }

    fn parse(&self, path: &Path) -> anyhow::Result<ParsedDocument> {
        let mut wb = open_workbook_auto(path)
            .map_err(|e| anyhow::anyhow!("failed to open XLSX: {e}"))?;

        let mut sections = vec![];

        for sheet_name in wb.sheet_names().to_vec() {
            if let Ok(range) = wb.worksheet_range(&sheet_name) {
                let mut rows_text = vec![];
                for row in range.rows() {
                    let cells: Vec<String> = row
                        .iter()
                        .filter_map(|cell| {
                            match cell {
                                DataType::String(s) if !s.trim().is_empty() => Some(s.clone()),
                                DataType::Float(f) => Some(f.to_string()),
                                DataType::Int(i) => Some(i.to_string()),
                                DataType::Bool(b) => Some(b.to_string()),
                                _ => None,
                            }
                        })
                        .collect();
                    if !cells.is_empty() {
                        rows_text.push(cells.join("\t"));
                    }
                }
                if !rows_text.is_empty() {
                    sections.push(Section {
                        content: rows_text.join("\n"),
                        heading: Some(sheet_name),
                        level: 1,
                        page_number: None,
                    });
                }
            }
        }

        Ok(ParsedDocument {
            source: path.to_string_lossy().to_string(),
            mime_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".to_string(),
            sections: if sections.is_empty() {
                vec![Section { content: String::new(), heading: None, level: 0, page_number: None }]
            } else {
                sections
            },
            title: None,
            metadata: serde_json::json!({}),
        })
    }
}
```

- [ ] **Шаг 4: Реализовать pptx.rs**

```rust
use std::io::{BufReader, Read};
use std::path::Path;
use quick_xml::events::Event;
use quick_xml::Reader;
use zip::ZipArchive;
use super::{DocumentAdapter, ParsedDocument, Section};

pub struct PptxAdapter;

const PPTX_MIME: &str = "application/vnd.openxmlformats-officedocument.presentationml.presentation";

impl DocumentAdapter for PptxAdapter {
    fn can_handle(&self, path: &Path, mime_type: &str) -> bool {
        mime_type == PPTX_MIME
            || path.extension().map_or(false, |e| e == "pptx")
    }

    fn parse(&self, path: &Path) -> anyhow::Result<ParsedDocument> {
        let file = std::fs::File::open(path)?;
        let mut archive = ZipArchive::new(BufReader::new(file))?;

        let slide_count = count_slides(&archive);
        let mut sections = vec![];

        for i in 1..=slide_count {
            let xml = match read_slide_xml(&mut archive, i) {
                Ok(xml) => xml,
                Err(_) => continue,
            };
            let text = extract_slide_text(&xml);
            if !text.trim().is_empty() {
                sections.push(Section {
                    content: text.trim().to_string(),
                    heading: Some(format!("Slide {i}")),
                    level: 1,
                    page_number: Some(i as u32),
                });
            }
        }

        Ok(ParsedDocument {
            source: path.to_string_lossy().to_string(),
            mime_type: PPTX_MIME.to_string(),
            sections: if sections.is_empty() {
                vec![Section { content: String::new(), heading: None, level: 0, page_number: None }]
            } else {
                sections
            },
            title: None,
            metadata: serde_json::json!({}),
        })
    }
}

fn count_slides(archive: &ZipArchive<BufReader<std::fs::File>>) -> usize {
    (1..=500)
        .take_while(|&i| {
            archive.index_for_name(&format!("ppt/slides/slide{i}.xml")).is_some()
        })
        .count()
}

fn read_slide_xml(
    archive: &mut ZipArchive<BufReader<std::fs::File>>,
    slide_num: usize,
) -> anyhow::Result<String> {
    let name = format!("ppt/slides/slide{slide_num}.xml");
    let mut f = archive.by_name(&name)?;
    let mut buf = String::new();
    f.read_to_string(&mut buf)?;
    Ok(buf)
}

fn extract_slide_text(xml: &str) -> String {
    let mut reader = Reader::from_str(xml);
    let mut texts = vec![];
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Text(e)) => {
                let t = e.unescape().unwrap_or_default().trim().to_string();
                if !t.is_empty() {
                    texts.push(t);
                }
            }
            Ok(Event::Eof) => break,
            _ => {}
        }
        buf.clear();
    }

    texts.join(" ")
}
```

- [ ] **Шаг 5: Запустить все тесты адаптеров**

```bash
cd rust && cargo test adapters 2>&1
```

Ожидаем: минимум 3 теста (markdown, docx.can_handle, pdf.pdftotext_binary_exists).

- [ ] **Шаг 6: Коммит**

```bash
git add rust/src/ingestion/adapters/
git commit -m "feat(rust): document adapters — PDF(pdftotext), DOCX, XLSX, PPTX, Markdown, Text"
```
