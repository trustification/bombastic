use reqwest::{Response, Url};
use trustification_api::search::SearchResult;
use trustification_auth::client::{TokenInjector, TokenProvider};
use trustification_infrastructure::tracing::PropagateCurrentContext;
use v11y_model::search::{SearchDocument, SearchHit};

pub use v11y_model::*;

pub struct V11yUrl {
    base_url: Url,
}

impl V11yUrl {
    pub fn new(base_url: Url) -> Self {
        Self { base_url }
    }

    pub fn vulnerability_url(&self) -> Url {
        self.base_url.join("/api/v1/vulnerability").unwrap()
    }

    pub fn get_cve_url(&self, id: impl AsRef<str>) -> Url {
        let mut url = self.base_url.join("/api/v1/cve").unwrap();
        url.path_segments_mut().unwrap().push(id.as_ref());
        url
    }

    pub fn search_url(&self) -> Url {
        self.base_url.join("/api/v1/search").unwrap()
    }

    pub fn get_vulnerability_url(&self, id: impl AsRef<str>) -> Url {
        self.base_url
            .join("/api/v1/vulnerability/")
            .unwrap()
            .join(id.as_ref())
            .unwrap()
    }

    pub fn get_vulnerability_by_alias_url(&self, alias: impl AsRef<str>) -> Url {
        self.base_url
            .join("/api/v1/vulnerability/by-alias/")
            .unwrap()
            .join(alias.as_ref())
            .unwrap()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("http error: {0}")]
    Http(reqwest::Error),
    #[error("auth error: {0}")]
    Auth(trustification_auth::client::Error),
}

impl From<reqwest::Error> for Error {
    fn from(inner: reqwest::Error) -> Self {
        Self::Http(inner)
    }
}

impl From<trustification_auth::client::Error> for Error {
    fn from(inner: trustification_auth::client::Error) -> Self {
        Self::Auth(inner)
    }
}

#[allow(unused)]
pub struct V11yClient {
    client: reqwest::Client,
    v11y_url: V11yUrl,
    provider: Box<dyn TokenProvider>,
}

impl V11yClient {
    pub fn new<P: TokenProvider>(client: reqwest::Client, url: Url, provider: P) -> Self
    where
        P: TokenProvider + 'static,
    {
        Self {
            client,
            v11y_url: V11yUrl::new(url),
            provider: Box::new(provider),
        }
    }

    pub async fn ingest_vulnerability(&self, vuln: &Vulnerability) -> Result<(), anyhow::Error> {
        Ok(self
            .client
            .post(self.v11y_url.vulnerability_url())
            .propagate_current_context()
            .inject_token(self.provider.as_ref())
            .await?
            .json(&vuln)
            .send()
            .await
            .map(|_| ())?)
    }

    pub async fn get_cve(&self, id: &str) -> Result<Response, anyhow::Error> {
        Ok(self
            .client
            .get(self.v11y_url.get_cve_url(id))
            .propagate_current_context()
            .inject_token(self.provider.as_ref())
            .await?
            .send()
            .await?)
    }

    pub async fn get_vulnerability(&self, id: &str) -> Result<Vec<Vulnerability>, Error> {
        Ok(self
            .client
            .get(self.v11y_url.get_vulnerability_url(id))
            .propagate_current_context()
            .inject_token(self.provider.as_ref())
            .await?
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    pub async fn get_vulnerability_by_alias(&self, alias: &str) -> Result<Vec<Vulnerability>, anyhow::Error> {
        Ok(self
            .client
            .get(self.v11y_url.get_vulnerability_by_alias_url(alias))
            .propagate_current_context()
            .inject_token(self.provider.as_ref())
            .await?
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    pub async fn search(
        &self,
        q: &str,
        limit: usize,
        offset: usize,
    ) -> Result<SearchResult<Vec<SearchHit<SearchDocument>>>, anyhow::Error> {
        Ok(self
            .client
            .get(self.v11y_url.search_url())
            .query(&[("q", q)])
            .query(&[("limit", limit), ("offset", offset)])
            .propagate_current_context()
            .inject_token(self.provider.as_ref())
            .await?
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }
}

#[cfg(test)]
mod test {
    use crate::Vulnerability;

    #[tokio::test]
    async fn serialization() -> Result<(), anyhow::Error> {
        let json = r#"
            {
                "origin": "osv",
                "id": "CVE-123",
                "modified": "2023-08-08T18:17:02Z",
                "published": "2023-08-08T18:17:02Z",
                "summary": "This is my summary",
                "details": "And\nhere are some\ndetails",
                "related": [
                    "related-foo",
                    "related-bar"
                ]
            }
        "#;

        let vuln: Vulnerability = serde_json::from_str(json)?;

        assert_eq!("osv", vuln.origin);
        assert_eq!("CVE-123", vuln.id);

        Ok(())
    }
}
