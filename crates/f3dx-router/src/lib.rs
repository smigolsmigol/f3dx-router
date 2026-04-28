//! f3dx-router: in-process Rust router for LLM providers.
//!
//! Composes with hosted gateways (llmkit at llmkit.sh, Helicone, Portkey)
//! rather than competing. The hosted gateway owns billing, dashboards,
//! multi-tenant config, audit logs. f3dx-router owns the in-process hot
//! path inside an agent loop where the network hop to a hosted gateway
//! is too expensive.
//!
//! Routing modes (RoutingPolicy):
//!   Sequential    fire to providers in order; on 429/5xx fall through
//!                 to the next. Lowest cost, highest latency on failures.
//!   Hedged        fire to top-K providers in parallel; return the first
//!                 non-error response, cancel the rest. Higher cost,
//!                 latency = min over K. Default K = 2.
//!   WeightedRR    weighted round-robin across providers; respects each
//!                 provider's rate-limit budget independently.
//!
//! Failure model: any HTTP 4xx other than 429 is a HARD failure (auth,
//! invalid request) and bubbles up immediately - retrying makes things
//! worse. 429 + 5xx + connection-reset are SOFT failures that trigger
//! the next provider in the policy.
//!
//! V0 ships Sequential + Hedged. WeightedRR + classifier-driven routing
//! land with the f3dx-trace integration in V0.1 + V0.2.

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RouterError {
    #[error("all providers exhausted: {causes:?}")]
    AllExhausted { causes: Vec<String> },
    #[error("hard failure on provider {provider:?}: {status} {body}")]
    HardFailure {
        provider: String,
        status: u16,
        body: String,
    },
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("no providers configured")]
    NoProviders,
}

pub type Result<T> = std::result::Result<T, RouterError>;

/// One upstream provider endpoint. The `kind` field shapes the URL
/// construction at request time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provider {
    pub name: String,
    pub kind: ProviderKind,
    pub base_url: String,
    pub api_key: String,
    /// Hard ceiling on per-request latency. Routes that exceed this get
    /// treated as a soft failure so the policy can move on.
    #[serde(
        default = "default_timeout_ms",
        skip_serializing_if = "is_default_timeout"
    )]
    pub timeout_ms: u64,
    /// Weight in WeightedRR mode. Ignored in Sequential and Hedged.
    #[serde(default = "default_weight", skip_serializing_if = "is_default_weight")]
    pub weight: u32,
}

fn default_timeout_ms() -> u64 {
    30_000
}

fn is_default_timeout(t: &u64) -> bool {
    *t == 30_000
}

fn default_weight() -> u32 {
    1
}

fn is_default_weight(w: &u32) -> bool {
    *w == 1
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    /// OpenAI-shaped chat-completions endpoint. Covers vLLM, Mistral,
    /// xAI, Groq, Together, Fireworks, DeepSeek, llmkit, OpenRouter, etc.
    OpenAI,
    /// Anthropic Messages API.
    Anthropic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RoutingPolicy {
    Sequential,
    Hedged,
}

#[derive(Debug, Clone)]
pub struct RouterConfig {
    pub providers: Vec<Provider>,
    pub policy: RoutingPolicy,
    /// Number of providers fired in parallel under the Hedged policy.
    /// Clamped to [1, providers.len()] at request time. Ignored in
    /// Sequential mode.
    pub hedge_k: usize,
}

impl RouterConfig {
    pub fn validate(&self) -> Result<()> {
        if self.providers.is_empty() {
            return Err(RouterError::NoProviders);
        }
        Ok(())
    }
}

pub struct Router {
    config: RouterConfig,
    client: Arc<reqwest::Client>,
}

impl Router {
    pub fn new(config: RouterConfig) -> Result<Self> {
        config.validate()?;
        let client = reqwest::Client::builder()
            .pool_max_idle_per_host(16)
            .build()?;
        Ok(Self {
            config,
            client: Arc::new(client),
        })
    }

    /// Send a chat-completions request through the configured policy.
    /// `body` is the OpenAI-shaped JSON. The router is JSON-shape-agnostic
    /// past the auth header; if you point the router at an Anthropic
    /// provider, send the Anthropic-shaped JSON yourself - this V0 does
    /// not translate request shapes.
    pub async fn chat_completions(&self, body: serde_json::Value) -> Result<serde_json::Value> {
        match self.config.policy {
            RoutingPolicy::Sequential => self.run_sequential(body).await,
            RoutingPolicy::Hedged => self.run_hedged(body).await,
        }
    }

    async fn run_sequential(&self, body: serde_json::Value) -> Result<serde_json::Value> {
        let mut causes = Vec::new();
        for p in &self.config.providers {
            match self.try_provider(p, &body).await {
                Ok(v) => return Ok(v),
                Err(RouterError::HardFailure {
                    provider,
                    status,
                    body,
                }) => {
                    return Err(RouterError::HardFailure {
                        provider,
                        status,
                        body,
                    });
                }
                Err(e) => {
                    causes.push(format!("{}: {}", p.name, e));
                }
            }
        }
        Err(RouterError::AllExhausted { causes })
    }

    async fn run_hedged(&self, body: serde_json::Value) -> Result<serde_json::Value> {
        use futures::stream::{FuturesUnordered, StreamExt};
        let k = self.config.hedge_k.clamp(1, self.config.providers.len());
        let body = Arc::new(body);
        let mut futs = FuturesUnordered::new();
        for p in self.config.providers.iter().take(k) {
            let body = Arc::clone(&body);
            let p = p.clone();
            let this = self;
            futs.push(async move { this.try_provider(&p, &body).await.map(|v| (p.name, v)) });
        }

        let mut causes = Vec::new();
        while let Some(res) = futs.next().await {
            match res {
                Ok((_name, value)) => return Ok(value),
                Err(RouterError::HardFailure {
                    provider,
                    status,
                    body,
                }) => {
                    return Err(RouterError::HardFailure {
                        provider,
                        status,
                        body,
                    });
                }
                Err(e) => causes.push(e.to_string()),
            }
        }
        Err(RouterError::AllExhausted { causes })
    }

    async fn try_provider(
        &self,
        provider: &Provider,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let url = build_url(provider);
        let resp = self
            .client
            .post(&url)
            .timeout(Duration::from_millis(provider.timeout_ms))
            .header("authorization", format!("Bearer {}", provider.api_key))
            .header("content-type", "application/json")
            .json(body)
            .send()
            .await?;
        let status = resp.status();
        if status.is_success() {
            return Ok(resp.json::<serde_json::Value>().await?);
        }
        let status_u16 = status.as_u16();
        let body_text = resp.text().await.unwrap_or_default();
        // 429 + 5xx + 408 (request timeout) = soft failure, route can
        // try the next provider. Everything else (auth, validation,
        // 404 model-not-found) is a hard failure that won't go away
        // by retrying elsewhere with the same payload.
        if status_u16 == 429 || status_u16 == 408 || (500..600).contains(&status_u16) {
            return Err(RouterError::AllExhausted {
                causes: vec![format!("{} -> {} {}", provider.name, status_u16, body_text)],
            });
        }
        Err(RouterError::HardFailure {
            provider: provider.name.clone(),
            status: status_u16,
            body: body_text,
        })
    }
}

fn build_url(provider: &Provider) -> String {
    match provider.kind {
        ProviderKind::OpenAI => format!(
            "{}/chat/completions",
            provider.base_url.trim_end_matches('/')
        ),
        ProviderKind::Anthropic => format!("{}/messages", provider.base_url.trim_end_matches('/')),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_providers_fails_validation() {
        let cfg = RouterConfig {
            providers: vec![],
            policy: RoutingPolicy::Sequential,
            hedge_k: 2,
        };
        assert!(matches!(Router::new(cfg), Err(RouterError::NoProviders)));
    }

    #[test]
    fn build_url_openai() {
        let p = Provider {
            name: "test".into(),
            kind: ProviderKind::OpenAI,
            base_url: "https://api.openai.com/v1".into(),
            api_key: "sk-test".into(),
            timeout_ms: 30_000,
            weight: 1,
        };
        assert_eq!(build_url(&p), "https://api.openai.com/v1/chat/completions");
    }

    #[test]
    fn build_url_strips_trailing_slash() {
        let p = Provider {
            name: "test".into(),
            kind: ProviderKind::OpenAI,
            base_url: "https://api.openai.com/v1/".into(),
            api_key: "sk-test".into(),
            timeout_ms: 30_000,
            weight: 1,
        };
        assert_eq!(build_url(&p), "https://api.openai.com/v1/chat/completions");
    }

    #[test]
    fn build_url_anthropic() {
        let p = Provider {
            name: "anthropic".into(),
            kind: ProviderKind::Anthropic,
            base_url: "https://api.anthropic.com/v1".into(),
            api_key: "sk-ant".into(),
            timeout_ms: 30_000,
            weight: 1,
        };
        assert_eq!(build_url(&p), "https://api.anthropic.com/v1/messages");
    }
}
