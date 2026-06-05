use crate::ingestion::adapters::Section;

/// A single chunk produced by the chunker.
#[derive(Debug, Clone)]
pub struct ChunkData {
    pub content: String,
    /// "parent" (L2, larger) or "leaf" (L1, smaller)
    pub chunk_role: String,
    pub chunk_index: usize,
    pub language: String,
    pub section_heading: Option<String>,
    pub section_level: u32,
    pub page_number: Option<u32>,
    pub embedding: Option<Vec<f32>>,
    /// For leaf chunks: the chunk_index of the parent in the same output Vec.
    pub parent_temp_index: Option<usize>,
}

/// Small-to-big chunker: splits each section into large parent (L2) windows and
/// small leaf (L1) windows.  Each leaf references its parent via `parent_temp_index`.
pub struct SmallToBigChunker {
    /// Parent chunk word count (L2 — the "big" chunk stored in `chunks` table).
    pub l2_size: usize,
    /// Leaf chunk word count (L1 — the "small" chunk indexed in tantivy/vectors).
    pub l1_size: usize,
    /// Overlap in words between adjacent leaf chunks.
    pub l2_overlap: usize,
}

impl SmallToBigChunker {
    pub fn new(l2_size: usize, l1_size: usize, l2_overlap: usize) -> Self {
        Self { l2_size, l1_size, l2_overlap }
    }

    /// Chunk all sections into a flat list of ChunkData (parents first within each
    /// section, then leaves for each parent).
    pub fn chunk(&self, sections: &[Section]) -> Vec<ChunkData> {
        let mut result: Vec<ChunkData> = Vec::new();
        let mut global_index: usize = 0;

        for section in sections {
            let words: Vec<&str> = section.content.split_whitespace().collect();
            if words.is_empty() {
                continue;
            }

            // Step size for parent windows (l2_size - l2_overlap).
            let l2_step = if self.l2_size > self.l2_overlap {
                self.l2_size - self.l2_overlap
            } else {
                // Guard: if overlap >= l2_size, step by 1 to avoid infinite loop
                1
            };

            // Collect parent windows
            let mut parent_start = 0usize;
            while parent_start < words.len() {
                let parent_end = (parent_start + self.l2_size).min(words.len());
                let parent_content = words[parent_start..parent_end].join(" ");

                let parent_idx = global_index;
                result.push(ChunkData {
                    content: parent_content.clone(),
                    chunk_role: "parent".to_string(),
                    chunk_index: parent_idx,
                    language: "en".to_string(), // filled in later by pipeline
                    section_heading: section.heading.clone(),
                    section_level: section.level,
                    page_number: section.page_number,
                    embedding: None,
                    parent_temp_index: None,
                });
                global_index += 1;

                // Generate leaf (L1) sub-chunks within this parent window.
                let parent_words = &words[parent_start..parent_end];
                let mut leaf_start = 0usize;
                while leaf_start < parent_words.len() {
                    let leaf_end = (leaf_start + self.l1_size).min(parent_words.len());
                    let leaf_content = parent_words[leaf_start..leaf_end].join(" ");

                    if !leaf_content.is_empty() {
                        result.push(ChunkData {
                            content: leaf_content,
                            chunk_role: "leaf".to_string(),
                            chunk_index: global_index,
                            language: "en".to_string(), // filled in later
                            section_heading: section.heading.clone(),
                            section_level: section.level,
                            page_number: section.page_number,
                            embedding: None,
                            parent_temp_index: Some(parent_idx),
                        });
                        global_index += 1;
                    }

                    if leaf_end == parent_words.len() {
                        break;
                    }
                    leaf_start += self.l1_size;
                }

                if parent_end == words.len() {
                    break;
                }
                parent_start += l2_step;
            }
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_section(content: &str) -> Section {
        Section {
            content: content.to_string(),
            heading: None,
            level: 0,
            page_number: None,
        }
    }

    #[test]
    fn chunk_creates_parent_leaf_pairs() {
        // l2_size=6, l1_size=3, overlap=0 — 9-word content → 2 parent windows
        let chunker = SmallToBigChunker::new(6, 3, 0);
        let sections = vec![make_section("one two three four five six seven eight nine")];
        let chunks = chunker.chunk(&sections);

        // Must have at least one parent and at least one leaf
        let parents: Vec<_> = chunks.iter().filter(|c| c.chunk_role == "parent").collect();
        let leaves: Vec<_> = chunks.iter().filter(|c| c.chunk_role == "leaf").collect();
        assert!(!parents.is_empty(), "should produce parent chunks");
        assert!(!leaves.is_empty(), "should produce leaf chunks");

        // Every leaf must have a parent_temp_index that references a parent chunk_index
        let parent_indices: std::collections::HashSet<usize> =
            parents.iter().map(|p| p.chunk_index).collect();
        for leaf in &leaves {
            let pidx = leaf.parent_temp_index.expect("leaf should have parent_temp_index");
            assert!(
                parent_indices.contains(&pidx),
                "leaf's parent_temp_index {pidx} must point to a valid parent"
            );
        }
    }

    #[test]
    fn leaf_word_count_does_not_exceed_l1_size() {
        let l1_size = 4;
        let chunker = SmallToBigChunker::new(12, l1_size, 2);
        let text = "a b c d e f g h i j k l m n o p q r s t";
        let sections = vec![make_section(text)];
        let chunks = chunker.chunk(&sections);

        for chunk in chunks.iter().filter(|c| c.chunk_role == "leaf") {
            let word_count = chunk.content.split_whitespace().count();
            assert!(
                word_count <= l1_size,
                "leaf word count {word_count} exceeds l1_size {l1_size}: {:?}",
                chunk.content
            );
        }
    }

    #[test]
    fn empty_section_produces_no_chunks() {
        let chunker = SmallToBigChunker::new(10, 5, 2);
        let sections = vec![make_section("   \t\n  ")];
        let chunks = chunker.chunk(&sections);
        assert!(chunks.is_empty(), "whitespace-only content should produce no chunks");
    }

    #[test]
    fn section_heading_propagated_to_chunks() {
        let chunker = SmallToBigChunker::new(5, 3, 0);
        let sections = vec![Section {
            content: "one two three four five".to_string(),
            heading: Some("Test Heading".to_string()),
            level: 1,
            page_number: Some(2),
        }];
        let chunks = chunker.chunk(&sections);
        for chunk in &chunks {
            assert_eq!(chunk.section_heading, Some("Test Heading".to_string()));
            assert_eq!(chunk.section_level, 1);
            assert_eq!(chunk.page_number, Some(2));
        }
    }
}
