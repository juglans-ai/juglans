// src/providers/search/tavily.rs
//
// Tavily API implementation of SearchProvider

use async_trait::async_trait;
use serde::Deserialize;

use super::{SearchProvider, SearchResponse, SearchResult};

/// Tavily API response (subset of fields we care about)
#[derive(Debug, Deserialize)]
struct TavilyResponse {
    #[serde(default)]
    results: Vec<TavilyResult>,
    #[serde(default)]
    answer: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TavilyResult {
    title: String,
    url: String,
    content: String,
}

pub struct TavilySearch {
    http_client: reqwest::Client,
    api_key: Option<String>,
}

impl TavilySearch {
    pub fn new(http_client: reqwest::Client) -> Self {
        let api_key = std::env::var("TAVILY_API_KEY").ok();
        if api_key.is_none() {
            tracing::warn!("TAVILY_API_KEY not set — /api/search will return errors");
        }
        Self {
            http_client,
            api_key,
        }
    }
}

#[async_trait]
impl SearchProvider for TavilySearch {
    async fn search(&self, query: &str) -> anyhow::Result<SearchResponse> {
        let api_key = self
            .api_key
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("TAVILY_API_KEY is not configured"))?;

        let body = serde_json::json!({
            "api_key": api_key,
            "query": query,
            "include_answer": true,
            "max_results": 10,
        });

        let resp = self
            .http_client
            .post("https://api.tavily.com/search")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Tavily API returned {}: {}", status, text));
        }

        let tavily: TavilyResponse = resp.json().await?;

        Ok(SearchResponse {
            results: tavily
                .results
                .into_iter()
                .map(|r| SearchResult {
                    title: r.title,
                    url: r.url,
                    content: r.content,
                })
                .collect(),
            answer: tavily.answer,
        })
    }
}
