use std::sync::Arc;

use crate::db::repositories::memories::Memory;
use crate::llm::LlmProvider;

const PROFILE_PROMPT: &str = r#"Summarize the following facts about a user into a concise profile (2-4 sentences max, ≤150 tokens).
Write in third person. Include only factual information from the list.

Facts:
{facts}

Profile summary:"#;

pub struct UserProfile {
    pub static_summary: String,
    pub dynamic_summary: String,
    pub raw_count: usize,
}

pub struct ProfileService {
    llm: Arc<dyn LlmProvider>,
}

impl ProfileService {
    pub fn new(llm: Arc<dyn LlmProvider>) -> Self {
        Self { llm }
    }

    pub async fn build_profile(&self, memories: &[Memory]) -> anyhow::Result<UserProfile> {
        let cutoff = chrono::Utc::now() - chrono::Duration::days(30);

        let mut static_memories: Vec<&Memory> = Vec::new();
        let mut dynamic_memories: Vec<&Memory> = Vec::new();

        for mem in memories {
            let created = chrono::DateTime::parse_from_rfc3339(&mem.created_at)
                .ok()
                .map(|dt| dt.with_timezone(&chrono::Utc))
                // Fall back to parsing without timezone offset (SQLite datetime format)
                .or_else(|| {
                    chrono::NaiveDateTime::parse_from_str(&mem.created_at, "%Y-%m-%dT%H:%M:%S")
                        .ok()
                        .map(|ndt| ndt.and_utc())
                })
                .or_else(|| {
                    chrono::NaiveDateTime::parse_from_str(&mem.created_at, "%Y-%m-%d %H:%M:%S")
                        .ok()
                        .map(|ndt| ndt.and_utc())
                });

            match created {
                Some(dt) if dt < cutoff => static_memories.push(mem),
                _ => dynamic_memories.push(mem),
            }
        }

        let static_summary = self._summarize(&static_memories).await?;
        let dynamic_summary = self._summarize(&dynamic_memories).await?;

        Ok(UserProfile {
            static_summary,
            dynamic_summary,
            raw_count: memories.len(),
        })
    }

    async fn _summarize(&self, memories: &[&Memory]) -> anyhow::Result<String> {
        if memories.is_empty() {
            return Ok(String::new());
        }

        let facts = memories
            .iter()
            .map(|m| format!("- {}", m.content))
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = PROFILE_PROMPT.replace("{facts}", &facts);
        let response = self.llm.complete(&prompt).await?;
        Ok(response.answer.trim().to_string())
    }
}
