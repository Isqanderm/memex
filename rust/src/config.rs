use std::str::FromStr;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LlmProviderKind {
    Claude,
    OpenAI,
}

impl FromStr for LlmProviderKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "claude" => Ok(LlmProviderKind::Claude),
            "openai" => Ok(LlmProviderKind::OpenAI),
            other => Err(format!("unknown llm provider: {other}")),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub database_path: String,
    pub tantivy_path: String,
    pub upload_dir: String,
    pub local_embedding_model: String,
    pub embedding_dimensions: usize,
    pub llm_provider: LlmProviderKind,
    pub llm_model: String,
    pub llm_max_tokens: u32,
    pub llm_temperature: f32,
    pub anthropic_api_key: Option<String>,
    pub openai_llm_api_key: Option<String>,
    pub l2_chunk_size: usize,
    pub l1_chunk_size: usize,
    pub l2_chunk_overlap: usize,
    pub semantic_top_k: usize,
    pub bm25_top_k: usize,
    pub rrf_k: usize,
    pub reranker_top_n: usize,
    pub host: String,
    pub port: u16,
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        let _ = dotenvy::dotenv();

        let get = |key: &str| -> Result<String, String> {
            std::env::var(key).map_err(|_| format!("missing env var: {key}"))
        };

        let get_or = |key: &str, default: &str| -> String {
            std::env::var(key).unwrap_or_else(|_| default.to_string())
        };

        let get_parse = |key: &str, default: &str| -> Result<usize, String> {
            get_or(key, default)
                .parse::<usize>()
                .map_err(|e| format!("{key}: {e}"))
        };

        let llm_provider = get("LLM_PROVIDER")?.parse::<LlmProviderKind>()?;

        let cfg = Config {
            database_path: get_or("DATABASE_PATH", "data/memex.db"),
            tantivy_path: get_or("TANTIVY_PATH", "data/tantivy"),
            upload_dir: get_or("UPLOAD_DIR", "data/uploads"),
            local_embedding_model: get_or("LOCAL_EMBEDDING_MODEL", "intfloat/multilingual-e5-small"),
            embedding_dimensions: get_parse("EMBEDDING_DIMENSIONS", "384")?,
            llm_provider,
            llm_model: get("LLM_MODEL")?,
            llm_max_tokens: get_or("LLM_MAX_TOKENS", "2048").parse().map_err(|e| format!("LLM_MAX_TOKENS: {e}"))?,
            llm_temperature: get_or("LLM_TEMPERATURE", "0.1").parse().map_err(|e| format!("LLM_TEMPERATURE: {e}"))?,
            anthropic_api_key: std::env::var("ANTHROPIC_API_KEY").ok(),
            openai_llm_api_key: std::env::var("OPENAI_LLM_API_KEY").ok(),
            l2_chunk_size: get_parse("L2_CHUNK_SIZE", "512")?,
            l1_chunk_size: get_parse("L1_CHUNK_SIZE", "128")?,
            l2_chunk_overlap: get_parse("L2_CHUNK_OVERLAP", "64")?,
            semantic_top_k: get_parse("SEMANTIC_TOP_K", "20")?,
            bm25_top_k: get_parse("BM25_TOP_K", "20")?,
            rrf_k: get_parse("RRF_K", "60")?,
            reranker_top_n: get_parse("RERANKER_TOP_N", "5")?,
            host: get_or("HOST", "0.0.0.0"),
            port: get_or("PORT", "8000").parse().map_err(|e| format!("PORT: {e}"))?,
        };

        cfg.validate()?;
        Ok(cfg)
    }

    pub fn validate(&self) -> Result<(), String> {
        match &self.llm_provider {
            LlmProviderKind::Claude if self.anthropic_api_key.is_none() => {
                Err("ANTHROPIC_API_KEY is required when LLM_PROVIDER=claude".to_string())
            }
            LlmProviderKind::OpenAI if self.openai_llm_api_key.is_none() => {
                Err("OPENAI_LLM_API_KEY is required when LLM_PROVIDER=openai".to_string())
            }
            _ => Ok(()),
        }
    }

    #[cfg(test)]
    pub fn default_for_test() -> Self {
        Config {
            database_path: ":memory:".to_string(),
            tantivy_path: "/tmp/tantivy_test".to_string(),
            upload_dir: "/tmp/uploads_test".to_string(),
            local_embedding_model: "intfloat/multilingual-e5-small".to_string(),
            embedding_dimensions: 384,
            llm_provider: LlmProviderKind::Claude,
            llm_model: "claude-haiku-4-5-20251001".to_string(),
            llm_max_tokens: 256,
            llm_temperature: 0.1,
            anthropic_api_key: Some("sk-test".to_string()),
            openai_llm_api_key: None,
            l2_chunk_size: 512,
            l1_chunk_size: 128,
            l2_chunk_overlap: 64,
            semantic_top_k: 20,
            bm25_top_k: 20,
            rrf_k: 60,
            reranker_top_n: 5,
            host: "127.0.0.1".to_string(),
            port: 8000,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_test_config_is_valid() {
        let cfg = Config::default_for_test();
        assert_eq!(cfg.embedding_dimensions, 384);
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn validate_fails_when_claude_without_key() {
        let cfg = Config {
            anthropic_api_key: None,
            ..Config::default_for_test()
        };
        assert!(cfg.validate().is_err());
    }
}
