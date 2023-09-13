use std::net::SocketAddr;
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use reqwest::Url;
use tokio::sync::RwLock;

use trustification_auth::client::{OpenIdTokenProviderConfigArguments, TokenProvider};
use trustification_infrastructure::endpoint::CollectorNvd;
use trustification_infrastructure::{
    endpoint::{self, Endpoint, EndpointServerConfig},
    Infrastructure, InfrastructureConfig,
};
use v11y_client::{ScoreType, Severity};

use crate::client::schema::{Reference, Vulnerability};
use crate::client::NvdClient;
use crate::server::{deregister_with_collectorist, register_with_collectorist};

mod client;
mod server;

#[derive(clap::Args, Debug)]
#[command(about = "Run the api server", args_conflicts_with_subcommands = true)]
pub struct Run {
    #[command(flatten)]
    pub api: EndpointServerConfig<CollectorNvd>,

    #[arg(long = "devmode", default_value_t = false)]
    pub devmode: bool,

    #[command(flatten)]
    pub infra: InfrastructureConfig,

    #[arg(
        env,
        short = 'u',
        long = "collectorist-url",
        default_value_t = endpoint::Collectorist::url()
    )]
    pub(crate) collectorist_url: Url,

    #[arg(
        env,
        short = 'v',
        long = "v11y-url",
        default_value_t = endpoint::V11y::url()
    )]
    pub(crate) v11y_url: Url,

    #[arg(env, long = "nvd-api-key")]
    pub(crate) nvd_api_key: String,

    #[command(flatten)]
    pub(crate) oidc: OpenIdTokenProviderConfigArguments,
}

impl Run {
    pub async fn run(mut self) -> anyhow::Result<ExitCode> {
        if self.devmode {
            self.v11y_url = Url::parse("http://localhost:8087").unwrap();
            self.collectorist_url = Url::parse("http://localhost:8088").unwrap();
        }

        Infrastructure::from(self.infra)
            .run("collector-nvd", |_metrics| async move {
                let provider = self.oidc.into_provider_or_devmode(self.devmode).await?;
                let state = Self::configure(
                    "nvd".into(),
                    self.collectorist_url,
                    self.v11y_url,
                    self.nvd_api_key,
                    provider,
                )
                .await?;
                let server = server::run(state.clone(), self.api.socket_addr()?);
                let register = register_with_collectorist(&state);

                tokio::select! {
                     _ = server => { }
                     _ = register => { }
                }

                deregister_with_collectorist(&state).await;
                Ok(())
            })
            .await?;

        Ok(ExitCode::SUCCESS)
    }

    async fn configure<P>(
        collector_id: String,
        collectorist_url: Url,
        v11y_url: Url,
        nvd_api_key: String,
        provider: P,
    ) -> anyhow::Result<Arc<AppState>>
    where
        P: TokenProvider + Clone + 'static,
    {
        let state = Arc::new(AppState::new(
            collector_id,
            collectorist_url,
            v11y_url,
            nvd_api_key,
            provider,
        ));
        Ok(state)
    }
}

pub(crate) type SharedState = Arc<AppState>;

pub struct AppState {
    addr: RwLock<Option<SocketAddr>>,
    connected: AtomicBool,
    collectorist_client: collectorist_client::CollectoristClient,
    v11y_client: v11y_client::V11yClient,
    guac_url: RwLock<Option<Url>>,
    nvd: NvdClient,
}

impl AppState {
    pub fn new<P>(collector_id: String, collectorist_url: Url, v11y_url: Url, nvd_api_key: String, provider: P) -> Self
    where
        P: TokenProvider + Clone + 'static,
    {
        Self {
            addr: RwLock::new(None),
            connected: AtomicBool::new(false),
            collectorist_client: collectorist_client::CollectoristClient::new(
                collector_id,
                collectorist_url,
                provider.clone(),
            ),
            v11y_client: v11y_client::V11yClient::new(v11y_url, provider),
            guac_url: RwLock::new(None),
            nvd: NvdClient::new(&nvd_api_key),
        }
    }
}

impl From<Vulnerability> for v11y_client::Vulnerability {
    fn from(vuln: Vulnerability) -> Self {
        Self {
            origin: "cve".to_string(),
            id: vuln.cve.id,
            modified: vuln.cve.last_modified.and_utc(),
            published: vuln.cve.published.and_utc(),
            withdrawn: None,
            summary: "".to_string(),
            details: vuln
                .cve
                .descriptions
                .iter()
                .find(|e| e.lang == "en")
                .map(|e| e.value.clone())
                .unwrap_or("".to_string()),
            aliases: vec![],
            affected: vec![],
            severities: vuln
                .cve
                .metrics
                .map(|inner| {
                    let mut severities = Vec::new();
                    for sev in inner.cvss_metric_v2 {
                        severities.push(Severity {
                            r#type: ScoreType::Cvss2,
                            source: sev.source.clone(),
                            score: sev.cvss_data.base_score,
                            additional: Some(sev.cvss_data.vector_string.clone()),
                        });
                    }

                    for sev in inner.cvss_metric_v30 {
                        severities.push(Severity {
                            r#type: ScoreType::Cvss3,
                            source: sev.source.clone(),
                            score: sev.cvss_data.base_score,
                            additional: Some(sev.cvss_data.vector_string.clone()),
                        });
                    }

                    for sev in inner.cvss_metric_v31 {
                        severities.push(Severity {
                            r#type: ScoreType::Cvss3,
                            source: sev.source.clone(),
                            score: sev.cvss_data.base_score,
                            additional: Some(sev.cvss_data.vector_string.clone()),
                        });
                    }

                    severities
                })
                .unwrap_or(vec![]),
            related: vec![],
            references: vuln.cve.references.iter().map(|e| e.into()).collect(),
        }
    }
}

impl From<&Reference> for v11y_client::Reference {
    fn from(reference: &Reference) -> Self {
        Self {
            r#type: "UNKNOWN".to_string(),
            url: reference.url.clone(),
        }
    }
}
