// pub mod data;

pub mod data {
    pub use spog_model::prelude::*;
}

mod access_token;
mod config;
mod pkg;
mod sbom;
mod search;
mod version;
mod vuln;

pub use access_token::*;
pub use config::*;
pub use pkg::*;
pub use sbom::*;
pub use search::*;
pub use version::*;
pub use vuln::*;

use url::{ParseError, Url};
use yew::html::IntoPropValue;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Backend {
    pub endpoints: Endpoints,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct OpenIdConnect {
    pub issuer: String,
    #[serde(default = "default::client_id")]
    pub client_id: String,
    #[serde(default = "default::scopes")]
    pub scopes: Vec<String>,
    #[serde(default = "default::after_logout")]
    pub after_logout: String,
}

mod default {
    pub fn client_id() -> String {
        "frontend".to_string()
    }

    pub fn scopes() -> Vec<String> {
        vec!["openid".to_string()]
    }

    pub fn after_logout() -> String {
        "/notloggedin".to_string()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct Endpoints {
    pub url: Url,
    pub bombastic: Url,
    pub vexination: Url,

    pub oidc: OpenIdConnect,
}

impl Endpoints {
    pub fn get(&self, endpoint: Endpoint) -> &Url {
        match endpoint {
            Endpoint::Api => &self.url,
            Endpoint::Vexination => &self.vexination,
            Endpoint::Bombastic => &self.bombastic,
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq)]
pub enum Endpoint {
    Api,
    Vexination,
    Bombastic,
}

impl Backend {
    pub fn join(&self, endpoint: Endpoint, input: &str) -> Result<Url, Error> {
        Ok(self.endpoints.get(endpoint).join(input)?)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Failed to parse backend URL: {0}")]
    Url(#[from] ParseError),
    #[error("Failed to request: {0}")]
    Request(#[from] reqwest::Error),
}

impl IntoPropValue<String> for Error {
    fn into_prop_value(self) -> String {
        self.to_string()
    }
}

impl IntoPropValue<String> for &Error {
    fn into_prop_value(self) -> String {
        self.to_string()
    }
}
