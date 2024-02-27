use crate::client::schema::{Issue, Response};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use url::ParseError;

pub mod schema;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("HTTP error: {0}")]
    Http(reqwest::Error),
    #[error("Serialization error: {0}")]
    Serialization(serde_json::Error),
    #[error("Snyk error: {0:?}")]
    Snyk(Vec<schema::Error>),
    #[error("URL error: {0:?}")]
    Url(#[from] ParseError),
}

impl From<reqwest::Error> for Error {
    fn from(inner: reqwest::Error) -> Self {
        Self::Http(inner)
    }
}

impl From<serde_json::Error> for Error {
    fn from(inner: serde_json::Error) -> Self {
        Self::Serialization(inner)
    }
}

struct SnykUrl(&'static str);

impl SnykUrl {
    const fn new(base: &'static str) -> Self {
        Self(base)
    }

    /*
    pub fn batch_issues(&self, org_id: &str) -> Url {
        Url::parse(&format!("{}/orgs/{}/packages/issues", self.0, org_id)).unwrap()
    }
     */

    pub fn issues(&self, org_id: &str, purl: &str) -> Result<Url, ParseError> {
        Url::parse(&format!(
            "{}/orgs/{}/packages/{}/issues",
            self.0,
            org_id,
            url_escape::encode_component(purl),
        ))
    }
}

const SNYK_URL: SnykUrl = SnykUrl::new("https://api.snyk.io/rest");

#[derive(Clone, Serialize, Deserialize)]
pub struct IssuesRequest {
    data: IssuesRequestData,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct IssuesRequestData {
    attributes: IssuesRequestAttributes,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct IssuesRequestAttributes {
    purls: Vec<String>,
}

pub struct SnykClient {
    org_id: String,
    token: String,
    client: reqwest::Client,
}

impl SnykClient {
    pub fn new(org_id: &str, token: &str) -> Self {
        Self {
            org_id: org_id.to_string(),
            token: token.to_string(),
            client: reqwest::Client::new(),
        }
    }

    /*
    pub async fn batch_issues(&self, purls: Vec<String>) -> Result<(), anyhow::Error> {
        println!("{}", SNYK_URL.batch_issues(&self.org_id));
        let result: serde_json::Map<_, _> = reqwest::Client::new()
            .post(SNYK_URL.batch_issues(&self.org_id))
            .header("Authorization", format!("token {}", &self.token))
            .header("Content-Type", "application/vnd.api+json")
            .query(&[("version", "2023-08-31~beta")])
            .json(&IssuesRequest {
                data: IssuesRequestData {
                    attributes: IssuesRequestAttributes {
                        purls
                    }
                }
            })
            .send()
            .await?
            .json()
            .await?;

        println!("{:#?}", result);

        Ok(())
    }
     */

    pub async fn issues(&self, purl: &str) -> Result<Vec<Issue>, Error> {
        let result: Response<Vec<Issue>> = self
            .client
            .get(SNYK_URL.issues(&self.org_id, purl)?)
            .header("Authorization", format!("token {}", &self.token))
            .header("Content-Type", "application/vnd.api+json")
            .query(&[("version", "2023-08-31~beta")])
            .send()
            .await?
            .json()
            .await?;

        if let Some(issues) = result.data {
            Ok(issues)
        } else {
            Err(Error::Snyk(result.errors.unwrap_or_default()))
        }
    }
}

#[cfg(test)]
mod test {

    use crate::client::SnykClient;

    pub fn client() -> Result<SnykClient, anyhow::Error> {
        let org_id = std::env::var("SNYK_ORG_ID")?;
        let token = std::env::var("SNYK_TOKEN")?;

        Ok(SnykClient::new(&org_id, &token))
    }

    /*
    #[test_with::env(SNYK_ORG_ID,SNYK_TOKEN)]
    #[tokio::test]
    async fn batch() -> Result<(), anyhow::Error> {
        client()?
            .batch_issues(vec!["pkg:maven/org.apache.logging.log4j/log4j-core@2.13.3".to_string()])
            .await?;
        Ok(())
    }

     */

    #[test_with::env(SNYK_ORG_ID, SNYK_TOKEN)]
    #[tokio::test]
    async fn single() -> Result<(), anyhow::Error> {
        let _result = client()?
            .issues("pkg:maven/org.apache.logging.log4j/log4j-core@2.13.3")
            .await?;

        //println!("{:#?}", result);
        Ok(())
    }
}
