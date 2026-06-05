use crate::db::repositories::chunks::L2Chunk;
use crate::search::memory_search::MemoryHit;

/// The built query context ready to be passed to the LLM.
#[derive(Debug, Clone)]
pub struct QueryContext {
    /// The complete formatted prompt (system + sources + question).
    pub prompt: String,
    /// Metadata for each document source chunk (for the API response).
    pub sources: Vec<serde_json::Value>,
}

const SYSTEM_V2: &str = "\
You are a question-answering assistant with access to two types of context:

1. PERSONAL MEMORY FACTS — atomic facts about the user (high signal, always current).
   Use these for questions about the user's life, preferences, location, work, etc.

2. DOCUMENT SOURCES — detailed content from indexed documents.
   Use these for specifics, evidence, quotes, and facts from documents.
   This is your primary source for detailed information.

Today's date: {date}

Instructions:
- For questions about the user, prioritize memory facts over documents.
- For questions about topics/documents, use document sources for details.
- Memory facts are summaries — if a document source contains more detail, use it.
- If neither memory nor documents contain the answer, say \"I don't know\" explicitly.
- Cite document sources as [1], [2], etc. Cite memory facts as [memory].";

/// Builds the formatted LLM prompt from retrieval results.
pub struct ContextBuilder;

impl ContextBuilder {
    /// Build a [`QueryContext`] from retrieved chunks and memory hits.
    pub fn build(
        &self,
        query: &str,
        chunks: &[L2Chunk],
        memory_hits: &[MemoryHit],
        today: &str,
    ) -> QueryContext {
        let system = SYSTEM_V2.replace("{date}", today);

        let mut sources_text = String::new();
        let mut sources_meta: Vec<serde_json::Value> = Vec::new();

        // Personal memory facts section
        if !memory_hits.is_empty() {
            sources_text.push_str("\nPersonal memory facts:\n");
            for hit in memory_hits.iter().take(5) {
                let mut parts = vec!["memory".to_string()];
                if let Some(cat) = &hit.category {
                    parts.push(cat.clone());
                }
                if let Some(proj) = &hit.project {
                    parts.push(proj.clone());
                }
                // Include date portion of created_at (first 10 chars)
                if !hit.created_at.is_empty() {
                    parts.push(hit.created_at.chars().take(10).collect());
                }
                let tag = parts.join(" | ");
                sources_text.push_str(&format!("  [{tag}] {}\n", hit.content));
            }
        }

        // Document sources section
        if !chunks.is_empty() {
            sources_text.push_str("\nDocument sources:\n");
            for (i, chunk) in chunks.iter().enumerate() {
                let idx = i + 1;
                let mut header_parts = vec![format!("[{idx}]")];
                if let Some(title) = &chunk.doc_title {
                    header_parts.push(title.clone());
                }
                if let Some(heading) = &chunk.section_heading {
                    header_parts.push(format!("— {heading}"));
                }
                if let Some(page) = chunk.page_number {
                    header_parts.push(format!("(p. {page})"));
                }

                sources_text.push('\n');
                sources_text.push_str(&header_parts.join(" "));
                sources_text.push('\n');
                sources_text.push_str("---\n");
                sources_text.push_str(&chunk.content);
                sources_text.push('\n');

                // Build filename: last component of the source path, then split by '-' and take 6th part
                let filename = chunk
                    .doc_source
                    .as_deref()
                    .and_then(|s| s.split('/').next_back())
                    .map(|raw_name| {
                        // Files stored as "{16-char checksum}-{original_name}" — strip prefix
                        if raw_name.len() > 17 && raw_name.chars().nth(16) == Some('-') {
                            raw_name[17..].to_string()
                        } else {
                            raw_name.to_string()
                        }
                    });

                let preview: String = chunk.content.chars().take(200).collect();

                sources_meta.push(serde_json::json!({
                    "index": idx,
                    "doc_id": chunk.doc_id,
                    "title": chunk.doc_title,
                    "section": chunk.section_heading,
                    "page": chunk.page_number,
                    "preview": preview,
                    "filename": filename,
                }));
            }
        }

        let prompt = format!("{system}\n{sources_text}\nQuestion: {query}");

        QueryContext { prompt, sources: sources_meta }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_chunk(content: &str, title: &str) -> L2Chunk {
        L2Chunk {
            chunk_id: "chunk-1".to_string(),
            content: content.to_string(),
            doc_id: "doc-1".to_string(),
            section_heading: None,
            page_number: None,
            doc_title: Some(title.to_string()),
            doc_source: Some("files/abc-def-ghi-jkl-mno-filename.md".to_string()),
        }
    }

    #[test]
    fn build_context_includes_chunks() {
        let builder = ContextBuilder;
        let chunk = make_chunk("This is the document body.", "My Document");
        let ctx = builder.build("What is this about?", &[chunk], &[], "2026-06-04");

        // Prompt should contain the chunk content
        assert!(ctx.prompt.contains("This is the document body."), "prompt should contain chunk content");
        // Prompt should contain the doc title
        assert!(ctx.prompt.contains("My Document"), "prompt should contain doc title");
        // Prompt should contain the query
        assert!(ctx.prompt.contains("What is this about?"), "prompt should contain the query");
        // Prompt should contain today's date
        assert!(ctx.prompt.contains("2026-06-04"), "prompt should contain today's date");

        // sources should have 1 entry
        assert_eq!(ctx.sources.len(), 1, "should have exactly 1 source entry");
        assert_eq!(ctx.sources[0]["index"], 1);
        assert_eq!(ctx.sources[0]["doc_id"], "doc-1");
    }

    #[test]
    fn build_context_with_memory_hits() {
        let builder = ContextBuilder;
        let mem_hit = MemoryHit {
            memory_id: "mem-1".to_string(),
            content: "User lives in Belgrade".to_string(),
            score: 0.9,
            source: "chat".to_string(),
            category: Some("personal".to_string()),
            project: None,
            created_at: "2026-01-15T10:00:00".to_string(),
        };
        let ctx = builder.build("Where does the user live?", &[], &[mem_hit], "2026-06-04");

        assert!(ctx.prompt.contains("User lives in Belgrade"), "prompt should contain memory content");
        assert!(ctx.prompt.contains("personal"), "prompt should contain memory category");
        assert!(ctx.sources.is_empty(), "no document sources");
    }

    #[test]
    fn filename_extraction() {
        let builder = ContextBuilder;
        let chunk = L2Chunk {
            chunk_id: "c1".to_string(),
            content: "text".to_string(),
            doc_id: "d1".to_string(),
            section_heading: None,
            page_number: None,
            doc_title: None,
            doc_source: Some("store/abcdef0123456789-myfile.pdf".to_string()),
        };
        let ctx = builder.build("q", &[chunk], &[], "2026-06-04");
        assert_eq!(ctx.sources[0]["filename"], "myfile.pdf");
    }
}
