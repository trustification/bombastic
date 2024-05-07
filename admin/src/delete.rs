use std::process::ExitCode;

use reqwest::StatusCode;
use trustification_auth::client::OpenIdTokenProviderConfigArguments;
use trustification_auth::client::TokenInjector;
use trustification_auth::client::TokenProvider;
use trustification_infrastructure::endpoint;
use trustification_infrastructure::endpoint::Endpoint;
use url::Url;

/// Delete documents from trustification
#[derive(clap::Subcommand, Debug)]
pub enum Delete {
    Bombastic(BombasticDelete),
    Vexination(VexinationDelete),
}

impl Delete {
    pub async fn run(self) -> anyhow::Result<ExitCode> {
        match self {
            Self::Bombastic(run) => run.run().await,
            Self::Vexination(run) => run.run().await,
        }
    }
}

#[derive(clap::Args, Debug)]
#[command(about = "Delete documents from Bombastic", args_conflicts_with_subcommands = true)]
pub struct BombasticDelete {
    /// URL of the Bombastic instance
    #[arg(short = 'u', long = "url", default_value_t = endpoint::Bombastic::url())]
    pub url: Url,

    /// ID of document to delete
    #[arg(short = 'i', long = "id", conflicts_with_all = ["all", "matches"])]
    pub id: Option<String>,

    /// Delete all documents
    #[arg(short = 'a', long = "all", conflicts_with_all = ["id", "matches"], default_value_t = false)]
    pub all: bool,

    /// Delete all documents matching regexp
    #[arg(short = 'm', long = "matches", conflicts_with_all = ["all", "id"])]
    pub matches: Option<String>,

    /// OIDC parameters
    #[command(flatten)]
    pub oidc: OpenIdTokenProviderConfigArguments,

    /// Development mode
    #[arg(long = "devmode", default_value_t = false)]
    pub devmode: bool,
}

impl BombasticDelete {
    pub async fn run(self) -> anyhow::Result<ExitCode> {
        let client = reqwest::Client::new();
        let provider = self.oidc.clone().into_provider_or_devmode(self.devmode).await?;
        if let Some(id) = &self.id {
            self.delete(&client, &provider, id).await?;
        } else if let Some(matches) = &self.matches {
            let matches = regex::Regex::new(matches)?;
            self.delete_all(&client, &provider, Some(matches)).await?;
        } else if self.all {
            self.delete_all(&client, &provider, None).await?;
        }

        Ok(ExitCode::SUCCESS)
    }

    async fn delete_all(
        &self,
        client: &reqwest::Client,
        provider: &impl TokenProvider,
        matches: Option<regex::Regex>,
    ) -> anyhow::Result<()> {
        const LIMIT: usize = 1000;
        let mut offset = 0;
        loop {
            match client
                .get(format!("{}api/v1/sbom/search", &self.url))
                .query(&[
                    ("q", "".to_string()),
                    ("offset", format!("{}", offset)),
                    ("limit", format!("{}", LIMIT)),
                ])
                .inject_token(provider)
                .await?
                .send()
                .await?
            {
                r if r.status() == StatusCode::OK => {
                    let response = r.json::<bombastic_model::prelude::SearchResult>().await?;
                    if response.result.is_empty() {
                        break;
                    }
                    log::info!("[Offset {}]: Got {} documents", offset, response.result.len());
                    for result in response.result.iter() {
                        if let Some(matches) = &matches {
                            if matches.is_match(&result.document.id) {
                                self.delete(client, provider, &result.document.id).await?;
                            }
                        } else {
                            self.delete(client, provider, &result.document.id).await?;
                        }
                    }

                    if response.result.len() < LIMIT {
                        break;
                    }

                    offset += response.result.len();
                }
                r => {
                    log::warn!("Failed to list documents: {}", r.status());
                    break;
                }
            }
        }
        Ok(())
    }

    async fn delete(&self, client: &reqwest::Client, provider: &impl TokenProvider, id: &str) -> anyhow::Result<()> {
        match client
            .delete(format!("{}api/v1/sbom", &self.url))
            .query(&[("id", id)])
            .inject_token(provider)
            .await?
            .send()
            .await
        {
            Ok(r) if r.status() == StatusCode::OK || r.status() == StatusCode::NO_CONTENT => {
                log::info!("Deleted document {}", id);
            }
            Ok(r) => {
                log::warn!("Failed to delete document {}: {}", id, r.status());
            }
            Err(e) => {
                log::warn!("Failed to delete document {}: {e}", id);
            }
        }
        Ok(())
    }
}

#[derive(clap::Args, Debug)]
#[command(about = "Delete documents from Vexination", args_conflicts_with_subcommands = true)]
pub struct VexinationDelete {
    /// URL of the Vexination instance
    #[arg(short = 'u', long = "url", default_value_t = endpoint::Vexination::url())]
    pub url: Url,

    /// ID of document to delete
    #[arg(short = 'i', long = "id", conflicts_with_all = ["all", "matches"])]
    pub id: Option<String>,

    /// Delete all documents
    #[arg(short = 'a', long = "all", conflicts_with_all = ["id", "matches"], default_value_t = false)]
    pub all: bool,

    /// Delete all documents matching regexp
    #[arg(short = 'm', long = "matches", conflicts_with_all = ["all", "id"])]
    pub matches: Option<String>,

    /// OIDC parameters
    #[command(flatten)]
    pub oidc: OpenIdTokenProviderConfigArguments,

    /// Development mode
    #[arg(long = "devmode", default_value_t = false)]
    pub devmode: bool,
}

impl VexinationDelete {
    pub async fn run(self) -> anyhow::Result<ExitCode> {
        let client = reqwest::Client::new();
        let provider = self.oidc.clone().into_provider_or_devmode(self.devmode).await?;
        if let Some(id) = &self.id {
            self.delete(&client, &provider, id).await?;
        } else if let Some(matches) = &self.matches {
            let matches = regex::Regex::new(matches)?;
            self.delete_all(&client, &provider, Some(matches)).await?;
        } else if self.all {
            self.delete_all(&client, &provider, None).await?;
        }

        Ok(ExitCode::SUCCESS)
    }

    async fn delete_all(
        &self,
        client: &reqwest::Client,
        provider: &impl TokenProvider,
        matches: Option<regex::Regex>,
    ) -> anyhow::Result<()> {
        const LIMIT: usize = 1000;
        let mut offset = 0;
        loop {
            match client
                .get(format!("{}api/v1/vex/search", &self.url))
                .query(&[
                    ("q", "".to_string()),
                    ("offset", format!("{}", offset)),
                    ("limit", format!("{}", LIMIT)),
                ])
                .inject_token(provider)
                .await?
                .send()
                .await?
            {
                r if r.status() == StatusCode::OK => {
                    let response = r.json::<vexination_model::prelude::SearchResult>().await?;
                    if response.result.is_empty() {
                        break;
                    }
                    log::info!("[Offset {}]: Got {} documents", offset, response.result.len());
                    for result in response.result.iter() {
                        if let Some(matches) = &matches {
                            log::info!("CHecking if {} matches {}", matches, result.document.advisory_id);
                            if matches.is_match(&result.document.advisory_id) {
                                self.delete(client, provider, &result.document.advisory_id).await?;
                            }
                        } else {
                            self.delete(client, provider, &result.document.advisory_id).await?;
                        }
                    }

                    if response.result.len() < LIMIT {
                        break;
                    }

                    offset += response.result.len();
                }
                r => {
                    log::warn!("Failed to list documents: {}", r.status());
                    break;
                }
            }
        }
        Ok(())
    }

    async fn delete(&self, client: &reqwest::Client, provider: &impl TokenProvider, id: &str) -> anyhow::Result<()> {
        match client
            .delete(format!("{}api/v1/vex", &self.url))
            .query(&[("advisory", id)])
            .inject_token(provider)
            .await?
            .send()
            .await
        {
            Ok(r) if r.status() == StatusCode::OK || r.status() == StatusCode::NO_CONTENT => {
                log::info!("Deleted document {}", id);
            }
            Ok(r) => {
                log::warn!("Failed to delete document {}: {}", id, r.status());
            }
            Err(e) => {
                log::warn!("Failed to delete document {}: {e}", id);
            }
        }
        Ok(())
    }
}
