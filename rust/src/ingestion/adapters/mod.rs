use std::path::Path;

pub mod docx;
pub mod epub;
pub mod markdown;
pub mod pdf;
pub mod pptx;
pub mod text;
pub mod xlsx;

pub use docx::DocxAdapter;
pub use epub::EpubAdapter;
pub use markdown::MarkdownAdapter;
pub use pdf::PdfAdapter;
pub use pptx::PptxAdapter;
pub use text::TextAdapter;
pub use xlsx::XlsxAdapter;

/// A single logical section extracted from a document.
#[derive(Debug, Clone)]
pub struct Section {
    /// The text content of this section.
    pub content: String,
    /// The heading that introduces this section, if any.
    pub heading: Option<String>,
    /// Heading depth: 0 = flat/no-heading, 1 = h1, 2 = h2, 3 = h3.
    pub level: u32,
    /// Page number (1-based), if the format tracks page breaks.
    pub page_number: Option<u32>,
}

/// The result of parsing a document file.
#[derive(Debug)]
pub struct ParsedDocument {
    /// Absolute path (or identifier) of the source file.
    pub source: String,
    /// MIME type used to dispatch the adapter.
    pub mime_type: String,
    /// Ordered list of sections extracted from the document.
    pub sections: Vec<Section>,
    /// Document title, if determinable.
    pub title: Option<String>,
    /// Arbitrary additional metadata (author, page-count, …).
    pub metadata: serde_json::Value,
}

/// Trait implemented by every format-specific parser.
pub trait DocumentAdapter: Send + Sync {
    /// Returns `true` when this adapter can handle the given file/MIME-type.
    fn can_handle(&self, path: &Path, mime_type: &str) -> bool;

    /// Parse the file and return a structured document.
    fn parse(&self, path: &Path) -> anyhow::Result<ParsedDocument>;
}

/// A registry that dispatches to the first matching adapter.
pub struct AdapterRegistry {
    adapters: Vec<Box<dyn DocumentAdapter>>,
}

impl AdapterRegistry {
    pub fn new() -> Self {
        AdapterRegistry { adapters: Vec::new() }
    }

    pub fn register(&mut self, adapter: impl DocumentAdapter + 'static) {
        self.adapters.push(Box::new(adapter));
    }

    /// Find the first adapter that claims it can handle the file and parse it.
    pub fn parse(&self, path: &Path, mime_type: &str) -> anyhow::Result<ParsedDocument> {
        for adapter in &self.adapters {
            if adapter.can_handle(path, mime_type) {
                return adapter.parse(path);
            }
        }
        anyhow::bail!(
            "No adapter found for path={} mime={}",
            path.display(),
            mime_type
        )
    }
}

impl Default for AdapterRegistry {
    fn default() -> Self {
        build_default_registry()
    }
}

/// Build a registry with all built-in adapters registered.
pub fn build_default_registry() -> AdapterRegistry {
    let mut registry = AdapterRegistry::new();
    registry.register(PdfAdapter);
    registry.register(DocxAdapter);
    registry.register(XlsxAdapter);
    registry.register(PptxAdapter);
    registry.register(EpubAdapter);
    registry.register(MarkdownAdapter);
    registry.register(TextAdapter);
    registry
}
