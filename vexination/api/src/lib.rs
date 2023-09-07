use actix_cors::Cors;
use std::{
    net::{SocketAddr, TcpListener},
    process::ExitCode,
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use actix_web::{web, HttpServer};
use actix_web_prom::PrometheusMetricsBuilder;
use anyhow::anyhow;
use prometheus::Registry;
use tokio::task::block_in_place;
use trustification_auth::{
    auth::AuthConfigArguments,
    authenticator::Authenticator,
    authorizer::Authorizer,
    swagger_ui::{SwaggerUiOidc, SwaggerUiOidcConfig},
};
use trustification_index::{IndexConfig, IndexStore};
use trustification_infrastructure::{
    app::{new_app, AppOptions},
    Infrastructure, InfrastructureConfig,
};
use trustification_storage::{Storage, StorageConfig};

mod server;

#[derive(clap::Args, Debug)]
#[command(about = "Run the api server", args_conflicts_with_subcommands = true)]
pub struct Run {
    #[arg(short, long, default_value = "0.0.0.0")]
    pub bind: String,

    #[arg(short = 'p', long = "port", default_value_t = 8080)]
    pub port: u16,

    #[arg(long = "devmode", default_value_t = false)]
    pub devmode: bool,

    #[command(flatten)]
    pub storage: StorageConfig,

    #[command(flatten)]
    pub infra: InfrastructureConfig,

    #[command(flatten)]
    pub auth: AuthConfigArguments,

    #[command(flatten)]
    pub swagger_ui_oidc: SwaggerUiOidcConfig,

    #[command(flatten)]
    pub index: IndexConfig,
}

impl Run {
    pub async fn run(self, listener: Option<TcpListener>) -> anyhow::Result<ExitCode> {
        let index = self.index;
        let storage = self.storage;

        let (authn, authz) = self.auth.split(self.devmode)?.unzip();
        let authenticator: Option<Arc<Authenticator>> = Authenticator::from_config(authn).await?.map(Arc::new);
        let authorizer = Authorizer::new(authz);

        let swagger_oidc: Option<Arc<SwaggerUiOidc>> =
            SwaggerUiOidc::from_devmode_or_config(self.devmode, self.swagger_ui_oidc)
                .await?
                .map(Arc::new);

        if authenticator.is_none() {
            log::warn!("Authentication is disabled");
        }

        Infrastructure::from(self.infra)
            .run("vexination-api", |metrics| async move {
                let state = Self::configure(index, storage, metrics.registry(), self.devmode)?;
                let http_metrics = PrometheusMetricsBuilder::new("vexination_api")
                    .registry(metrics.registry().clone())
                    .build()
                    .map_err(|_| anyhow!("Error registering HTTP metrics"))?;
                let mut srv = HttpServer::new(move || {
                    let http_metrics = http_metrics.clone();
                    let cors = Cors::permissive();
                    let authenticator = authenticator.clone();
                    let authorizer = authorizer.clone();
                    let swagger_oidc = swagger_oidc.clone();

                    new_app(AppOptions {
                        cors: Some(cors),
                        metrics: Some(http_metrics),
                        authenticator: None,
                        authorizer,
                    })
                    .app_data(web::Data::new(state.clone()))
                    .configure(move |svc| server::config(svc, authenticator.clone(), swagger_oidc.clone()))
                });
                srv = match listener {
                    Some(v) => srv.listen(v)?,
                    None => {
                        let addr = SocketAddr::from_str(&format!("{}:{}", self.bind, self.port))?;
                        srv.bind(addr)?
                    }
                };
                srv.run().await.map_err(anyhow::Error::msg)
            })
            .await?;
        Ok(ExitCode::SUCCESS)
    }

    fn configure(
        index_config: IndexConfig,
        storage: StorageConfig,
        registry: &Registry,
        devmode: bool,
    ) -> anyhow::Result<Arc<AppState>> {
        let index =
            block_in_place(|| IndexStore::new(&storage, &index_config, vexination_index::Index::new(), registry))?;
        let storage = Storage::new(storage.process("vexination", devmode), registry)?;

        let state = Arc::new(AppState { storage, index });

        let sinker = state.clone();
        let sync_interval = index_config.sync_interval.into();
        tokio::task::spawn(async move {
            loop {
                if sinker.sync_index().await.is_ok() {
                    log::info!("Initial vexination index synced");
                    break;
                } else {
                    log::warn!("Vexination index not yet available");
                }
                tokio::time::sleep(Duration::from_secs(5)).await;
            }

            loop {
                if let Err(e) = sinker.sync_index().await {
                    log::info!("Unable to synchronize vexination index: {:?}", e);
                }
                tokio::time::sleep(sync_interval).await;
            }
        });

        Ok(state)
    }
}

pub(crate) type Index = IndexStore<vexination_index::Index>;
pub struct AppState {
    storage: Storage,
    index: Index,
}

pub(crate) type SharedState = Arc<AppState>;

impl AppState {
    async fn sync_index(&self) -> Result<(), anyhow::Error> {
        let storage = &self.storage;
        let index = &self.index;
        index.sync(storage).await?;
        Ok(())
    }
}
