use reqwest::StatusCode;
use std::rc::Rc;
use url::Url;

use super::{Backend, Error};
use crate::backend::{ApplyAccessToken, Endpoint};
use yew_oauth2::prelude::*;

#[allow(unused)]
pub struct SBOMService {
    backend: Rc<Backend>,
    access_token: Option<LatestAccessToken>,
    client: reqwest::Client,
}

#[allow(unused)]
impl SBOMService {
    pub fn new(backend: Rc<Backend>, access_token: Option<LatestAccessToken>) -> Self {
        Self {
            backend,
            access_token,
            client: reqwest::Client::new(),
        }
    }

    pub fn download_href(&self, pkg: impl AsRef<str>) -> Result<Url, Error> {
        let mut url = self.backend.join(Endpoint::Api, "/api/package/sbom")?;

        url.query_pairs_mut().append_pair("purl", pkg.as_ref()).finish();

        Ok(url)
    }

    pub async fn get(&self, id: impl AsRef<str>) -> Result<Option<String>, Error> {
        let mut url = self.backend.join(Endpoint::Api, "/api/v1/package")?;
        url.query_pairs_mut().append_pair("id", id.as_ref()).finish();

        let response = self
            .client
            .get(url)
            .latest_access_token(&self.access_token)
            .send()
            .await?;

        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }

        Ok(Some(response.error_for_status()?.text().await?))
    }
}
