use crate::api_bridge::auth_provider_from_auth;
use crate::auth::AuthManager;
use crate::default_client::build_reqwest_client;
use crate::model_provider_info::ModelProviderInfo;
use anyhow::Context;
use anyhow::Result;
use codex_api::AuthProvider;
use codex_api::Provider;
use reqwest::header::AUTHORIZATION;
use reqwest::header::HeaderMap;
use serde::Deserialize;
use serde::Serialize;
use std::sync::Arc;

pub struct EmbeddingClient {
    provider: Provider,
    auth_header: Option<String>,
    client: reqwest::Client,
}

impl EmbeddingClient {
    pub async fn new(
        provider: ModelProviderInfo,
        auth_manager: Option<Arc<AuthManager>>,
    ) -> Result<Self> {
        let auth = auth_manager.as_ref().and_then(|m| m.auth());
        let provider_info = provider
            .to_api_provider(auth.as_ref().map(|a| a.mode))
            .context("failed to resolve embedding provider")?;
        let auth_provider = auth_provider_from_auth(auth, &provider).await?;
        let auth_header = auth_provider
            .bearer_token()
            .map(|token| format!("Bearer {token}"));
        let client = build_reqwest_client();
        Ok(Self {
            provider: provider_info,
            auth_header,
            client,
        })
    }

    pub async fn embed(&self, model: &str, inputs: &[String]) -> Result<Vec<Vec<f32>>> {
        let url = self.provider.url_for_path("embeddings");
        let mut headers = HeaderMap::new();
        headers.extend(self.provider.headers.clone());
        if let Some(auth_header) = &self.auth_header
            && let Ok(value) = auth_header.parse()
        {
            headers.insert(AUTHORIZATION, value);
        }
        let payload = EmbeddingRequest {
            model,
            input: inputs,
        };
        let response = self
            .client
            .post(url)
            .headers(headers)
            .json(&payload)
            .send()
            .await
            .context("failed to send embeddings request")?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("embeddings request failed with {status}: {body}");
        }
        let data: EmbeddingResponse = response.json().await?;
        let mut embeddings = data.data;
        embeddings.sort_by_key(|item| item.index);
        Ok(embeddings.into_iter().map(|item| item.embedding).collect())
    }
}

#[derive(Debug, Serialize)]
struct EmbeddingRequest<'a> {
    model: &'a str,
    input: &'a [String],
}

#[derive(Debug, Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingItem>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingItem {
    index: usize,
    embedding: Vec<f32>,
}
