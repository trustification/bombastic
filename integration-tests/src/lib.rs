#![allow(clippy::unwrap_used)]

mod bom;
mod config;
mod provider;
mod spog;
mod ui;
mod vex;

pub mod runner;

pub use bom::*;
pub use provider::*;
pub use spog::*;
pub use ui::*;
pub use vex::*;

use core::future::Future;
use reqwest::{StatusCode, Url};
use serde_json::Value;
use std::time::Duration;
use tokio::select;
use trustification_auth::{auth::AuthConfigArguments, client::TokenInjector, devmode};
use trustification_event_bus::EventBusConfig;
use {
    spog_api::DEFAULT_CRDA_PAYLOAD_LIMIT, std::net::TcpListener, trustification_auth::swagger_ui::SwaggerUiOidcConfig,
    trustification_event_bus::EventBusType, trustification_index::IndexConfig,
    trustification_infrastructure::InfrastructureConfig, trustification_storage::StorageConfig,
};

const STORAGE_ENDPOINT: &str = "http://localhost:9000";
const KAFKA_BOOTSTRAP_SERVERS: &str = "localhost:9092";

pub async fn get_response(url: &Url, exp_status: StatusCode, context: &ProviderContext) -> Option<Value> {
    let response = reqwest::Client::new()
        .get(url.to_owned())
        .inject_token(&context.provider_manager)
        .await
        .unwrap()
        .send()
        .await
        .unwrap();
    assert_eq!(
        exp_status,
        response.status(),
        "Expected response code does not match with actual response"
    );
    if matches!(exp_status, StatusCode::BAD_REQUEST | StatusCode::NOT_FOUND) {
        None
    } else {
        response.json().await.unwrap()
    }
}

/// Return a unique ID
pub fn id(prefix: &str) -> String {
    let uuid = uuid::Uuid::new_v4();
    format!("{prefix}-{uuid}")
}

pub trait Urlifier {
    fn base_url(&self) -> &Url;
    fn urlify<S: Into<String>>(&self, path: S) -> Url {
        self.base_url().join(&path.into()).unwrap()
    }
}

fn testing_auth() -> AuthConfigArguments {
    AuthConfigArguments {
        disabled: false,
        config: Some("config/auth.yaml".into()),
        clients: Default::default(),
    }
}

fn testing_swagger_ui_oidc() -> SwaggerUiOidcConfig {
    SwaggerUiOidcConfig {
        tls_insecure: false,
        ca_certificates: vec![],
        swagger_ui_oidc_issuer_url: Some(devmode::issuer_url()),
        swagger_ui_oidc_client_id: "frontend".to_string(),
    }
}
