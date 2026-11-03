use std::sync::Arc;

use rusqlite::{params, Connection, OptionalExtension};
use tracing::warn;

use crate::db::repositories::memories::{Memory, MemoryRepository};
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

    /// Cached profile: rebuilt when the active-memory fingerprint, the scope or the UTC
    /// date changes (the date keys the static/dynamic split, which uses a 30-day cutoff).
    /// SQLite work is synchronous on `conn`; the LLM is awaited only on a cache miss.
    pub async fn get_or_build(
        &self,
        conn: &Connection,
        scope: Option<&str>,
    ) -> anyhow::Result<UserProfile> {
        let repo = MemoryRepository::new(conn);
        let today = chrono::Utc::now().format("%Y-%m-%d");
        let fp = format!("{}|{}|{today}", scope.unwrap_or("*"), repo.fingerprint()?);

        let cached = conn
            .query_row(
                "SELECT static_summary, dynamic_summary, raw_count FROM profile_cache
                 WHERE id = 1 AND fingerprint = ?1",
                params![fp],
                |r| {
                    Ok(UserProfile {
                        static_summary: r.get(0)?,
                        dynamic_summary: r.get(1)?,
                        raw_count: r.get::<_, i64>(2)? as usize,
                    })
                },
            )
            .optional()?;
        if let Some(p) = cached {
            return Ok(p);
        }

        let memories = repo.get_all_active(scope)?;
        let profile = self.build_profile(&memories).await?;
        if let Err(e) = conn.execute(
            "INSERT OR REPLACE INTO profile_cache
                 (id, fingerprint, static_summary, dynamic_summary, raw_count, built_at)
             VALUES (1, ?1, ?2, ?3, ?4, datetime('now'))",
            params![fp, profile.static_summary, profile.dynamic_summary, profile.raw_count as i64],
        ) {
            warn!("Profile cache write failed, returning the uncached profile: {e}");
        }
        Ok(profile)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::pool::build_pool;
    use crate::db::repositories::memories::NewMemory;
    use crate::llm::test_support::ScriptedLlm;
    use std::sync::atomic::Ordering;

    fn nm(content: &str) -> NewMemory<'_> {
        NewMemory {
            content,
            source: "t",
            source_id: None,
            scope: "default",
            forget_after: None,
            relation: None,
            parent_id: None,
            category: None,
            project: None,
        }
    }

    #[tokio::test]
    async fn second_call_with_same_fingerprint_does_not_call_llm() {
        let pool = build_pool(":memory:").unwrap();
        let conn = pool.get().unwrap();
        MemoryRepository::new(&conn).create(&nm("User likes Rust")).unwrap();
        let llm = Arc::new(ScriptedLlm::new(vec!["summary one", "summary two"]));
        let svc = ProfileService::new(llm.clone());

        let first = svc.get_or_build(&conn, None).await.unwrap();
        let calls = llm.calls.load(Ordering::SeqCst);
        assert!(calls >= 1);
        let second = svc.get_or_build(&conn, None).await.unwrap();
        assert_eq!(llm.calls.load(Ordering::SeqCst), calls);
        assert_eq!(first.dynamic_summary, second.dynamic_summary);
        assert_eq!(second.raw_count, 1);

        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let fp: String = conn
            .query_row("SELECT fingerprint FROM profile_cache WHERE id = 1", [], |r| r.get(0))
            .unwrap();
        assert!(fp.ends_with(&format!("|{today}")), "fingerprint carries the date: {fp}");
    }

    #[tokio::test]
    async fn cache_write_failure_still_returns_profile() {
        let pool = build_pool(":memory:").unwrap();
        let conn = pool.get().unwrap();
        MemoryRepository::new(&conn).create(&nm("User likes Rust")).unwrap();
        let svc = ProfileService::new(Arc::new(ScriptedLlm::new(vec!["summary"])));
        // Reads work, every cache write fails.
        conn.execute_batch(
            "CREATE TRIGGER no_write BEFORE INSERT ON profile_cache
                 BEGIN SELECT RAISE(ABORT, 'read-only'); END;",
        )
        .unwrap();
        let p = svc.get_or_build(&conn, None).await.unwrap();
        assert_eq!(p.raw_count, 1);
    }

    #[tokio::test]
    async fn forget_invalidates_cache() {
        let pool = build_pool(":memory:").unwrap();
        let conn = pool.get().unwrap();
        let repo = MemoryRepository::new(&conn);
        repo.create(&nm("fact a")).unwrap();
        let b = repo.create(&nm("fact b")).unwrap();
        let llm = Arc::new(ScriptedLlm::new(vec!["s1", "s2"]));
        let svc = ProfileService::new(llm.clone());

        svc.get_or_build(&conn, None).await.unwrap();
        let before = llm.calls.load(Ordering::SeqCst);
        repo.deactivate(&b).unwrap();
        let p = svc.get_or_build(&conn, None).await.unwrap();
        assert!(llm.calls.load(Ordering::SeqCst) > before);
        assert_eq!(p.raw_count, 1);
    }
}
