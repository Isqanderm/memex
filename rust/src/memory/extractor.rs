use std::sync::Arc;

use crate::llm::LlmProvider;

const EXTRACT_PROMPT: &str = r#"Extract atomic facts about the user from the following text.
Rules:
- Each fact is one statement, no pronouns — use "User" as subject.
- Include: identity, skills, location, work, relationships, projects, preferences, events the user participated in.
- Exclude: opinions and emotional reactions, third-party info.
- Normalize state: prefer "User uses X" over "User switched from Y to X".
- Time-bound facts: add "forget_after" as ISO datetime. Permanent facts: omit "forget_after".
- Set "category" to: research | reminder | decision | preference | insight (or omit if unclear).
- Set "project" to project/context name if applicable. Omit if unclear.

Text: {text}

Return JSON only:
{"facts": [{"content": "...", "forget_after": "...or omit", "category": "...or omit", "project": "...or omit"}]}"#;

const RESOLVE_PROMPT: &str = r#"New fact: "{new_fact}"

Existing similar facts:
{existing}

For each existing fact determine the relation:
- updates: new fact contradicts and supersedes the old one
- extends: new fact adds detail without contradiction
- derives: new fact is a logical conclusion from the old one
- new: not meaningfully related

Return JSON only:
{"relations": [{"id": "...", "type": "updates|extends|derives|new"}]}"#;

const VALID_CATEGORIES: &[&str] = &["research", "reminder", "insight", "decision", "preference"];

#[derive(Debug)]
pub struct ExtractedFact {
    pub content: String,
    pub forget_after: Option<chrono::DateTime<chrono::Utc>>,
    pub category: Option<String>,
    pub project: Option<String>,
}

#[derive(Debug)]
pub struct RelationResult {
    pub memory_id: String,
    pub relation: String,
}

pub struct FactExtractor {
    llm: Arc<dyn LlmProvider>,
}

impl FactExtractor {
    pub fn new(llm: Arc<dyn LlmProvider>) -> Self {
        Self { llm }
    }

    pub async fn extract_facts(&self, text: &str) -> anyhow::Result<Vec<ExtractedFact>> {
        let prompt = EXTRACT_PROMPT.replace("{text}", text);
        let response = self.llm.complete(&prompt).await?;
        Ok(parse_facts_json(&response.answer).unwrap_or_default())
    }

    pub async fn resolve_relations(
        &self,
        new_fact: &str,
        existing: &[(String, String)],
    ) -> anyhow::Result<Vec<RelationResult>> {
        let existing_str: String = existing
            .iter()
            .map(|(id, content)| format!("id={id}: {content}"))
            .collect::<Vec<_>>()
            .join("\n");
        let prompt = RESOLVE_PROMPT
            .replace("{new_fact}", new_fact)
            .replace("{existing}", &existing_str);
        let response = self.llm.complete(&prompt).await?;
        Ok(parse_relations_json(&response.answer).unwrap_or_default())
    }
}

/// Extract JSON substring from LLM response: find first `{` and last `}`.
fn extract_json(s: &str) -> Option<&str> {
    let start = s.find('{')?;
    let end = s.rfind('}')?;
    if end >= start {
        Some(&s[start..=end])
    } else {
        None
    }
}

pub fn parse_facts_json(raw: &str) -> anyhow::Result<Vec<ExtractedFact>> {
    let json_str = match extract_json(raw) {
        Some(s) => s,
        None => return Ok(vec![]),
    };

    let value: serde_json::Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => return Ok(vec![]),
    };

    let facts_arr = match value.get("facts").and_then(|v| v.as_array()) {
        Some(arr) => arr,
        None => return Ok(vec![]),
    };

    let mut facts = Vec::new();
    for item in facts_arr {
        let content = match item.get("content").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };

        let forget_after = item
            .get("forget_after")
            .and_then(|v| v.as_str())
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc));

        let category = item
            .get("category")
            .and_then(|v| v.as_str())
            .filter(|s| VALID_CATEGORIES.contains(s))
            .map(|s| s.to_string());

        let project = item
            .get("project")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        facts.push(ExtractedFact {
            content,
            forget_after,
            category,
            project,
        });
    }

    Ok(facts)
}

pub fn parse_relations_json(raw: &str) -> anyhow::Result<Vec<RelationResult>> {
    let json_str = match extract_json(raw) {
        Some(s) => s,
        None => return Ok(vec![]),
    };

    let value: serde_json::Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => return Ok(vec![]),
    };

    let relations_arr = match value.get("relations").and_then(|v| v.as_array()) {
        Some(arr) => arr,
        None => return Ok(vec![]),
    };

    let mut relations = Vec::new();
    for item in relations_arr {
        let id = match item.get("id").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        let relation_type = match item.get("type").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        relations.push(RelationResult {
            memory_id: id,
            relation: relation_type,
        });
    }

    Ok(relations)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_facts_from_json() {
        let json = r#"{"facts": [
            {"content": "User works at Acme Corp", "category": "preference"},
            {"content": "User meeting tomorrow", "forget_after": "2026-06-05T09:00:00Z", "category": "reminder"}
        ]}"#;
        let facts = parse_facts_json(json).unwrap();
        assert_eq!(facts.len(), 2);
        assert_eq!(facts[0].category.as_deref(), Some("preference"));
        assert!(facts[1].forget_after.is_some());
    }

    #[test]
    fn parse_facts_handles_invalid_json() {
        let facts = parse_facts_json("not json at all").unwrap();
        assert!(facts.is_empty());
    }

    #[test]
    fn parse_relations_from_json() {
        let json = r#"{"relations": [{"id": "550e8400-e29b-41d4-a716-446655440000", "type": "updates"}]}"#;
        let rels = parse_relations_json(json).unwrap();
        assert_eq!(rels.len(), 1);
        assert_eq!(rels[0].relation, "updates");
    }

    #[test]
    fn parse_facts_filters_invalid_category() {
        let json = r#"{"facts": [{"content": "User likes Rust", "category": "invalid_cat"}]}"#;
        let facts = parse_facts_json(json).unwrap();
        assert_eq!(facts.len(), 1);
        assert!(facts[0].category.is_none(), "invalid category should be filtered out");
    }

    #[test]
    fn parse_facts_with_project() {
        let json = r#"{"facts": [{"content": "User is building memex", "category": "insight", "project": "memex"}]}"#;
        let facts = parse_facts_json(json).unwrap();
        assert_eq!(facts[0].project.as_deref(), Some("memex"));
    }

    #[test]
    fn extract_json_handles_extra_text() {
        let raw = "Here is the result:\n{\"facts\": []}\nEnd of output.";
        let extracted = extract_json(raw).unwrap();
        assert!(extracted.contains("facts"));
    }

    #[test]
    fn parse_relations_handles_invalid_json() {
        let rels = parse_relations_json("garbage {{}").unwrap();
        assert!(rels.is_empty());
    }
}
