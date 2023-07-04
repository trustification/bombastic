use std::{
    net::{SocketAddr, TcpListener},
    process::ExitCode,
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use actix_web::{dev::Service, error::ErrorUnauthorized};
use actix_web::{
    http::{header::Header, Method},
    web, App, HttpServer,
};
use actix_web_httpauth::headers::authorization::{Authorization, Bearer};
use prometheus::Registry;
use tokio::sync::RwLock;
use trustification_index::{IndexConfig, IndexStore};
use trustification_infrastructure::{Infrastructure, InfrastructureConfig};
use trustification_storage::{Storage, StorageConfig};

mod sbom;
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

    /// Require a secret token for publishing publishing SBOMs.
    #[arg(env, long = "publish-secret")]
    pub publish_secret_token: Option<String>,

    #[command(flatten)]
    pub index: IndexConfig,

    #[command(flatten)]
    pub storage: StorageConfig,

    #[command(flatten)]
    pub infra: InfrastructureConfig,
}

impl Run {
    pub async fn run(self, listener: Option<TcpListener>) -> anyhow::Result<ExitCode> {
        let index = self.index;
        let storage = self.storage;
        Infrastructure::from(self.infra)
            .run("bombastic-api", |metrics| async move {
                let state = Self::configure(index, storage, metrics.registry(), self.devmode)?;
                let mut srv = HttpServer::new(move || {
                    let secret = self.publish_secret_token.clone();
                    App::new()
                        // NOTE: Workaround until we have an authentication and authorization scheme
                        .wrap_fn(move |req, srv| {
                            let secret = secret.clone();
                            let mut auth = Err(ErrorUnauthorized("Not authorized to write"));
                            if req.method() == Method::POST
                                || req.method() == Method::PUT
                                || req.method() == Method::DELETE
                            {
                                if let Some(secret) = secret {
                                    if let Ok(r) = Authorization::<Bearer>::parse(&req) {
                                        if r.as_ref().token() == secret {
                                            auth = Ok(())
                                        }
                                    }
                                } else {
                                    auth = Ok(())
                                }
                            } else {
                                auth = Ok(())
                            }

                            let fut = srv.call(req);
                            async move {
                                match auth {
                                    Ok(_) => {
                                        let res = fut.await?;
                                        Ok(res)
                                    }
                                    Err(e) => Err(e),
                                }
                            }
                        })
                        .app_data(web::Data::new(state.clone()))
                        .configure(server::config)
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
        index: IndexConfig,
        mut storage: StorageConfig,
        registry: &Registry,
        devmode: bool,
    ) -> anyhow::Result<Arc<AppState>> {
        let sync_interval: Duration = index.sync_interval.into();
        let index = IndexStore::new(&index, bombastic_index::Index::new(), registry)?;
        let storage = storage.create("bombastic", devmode, registry)?;

        let state = Arc::new(AppState {
            storage: RwLock::new(storage),
            index: RwLock::new(index),
        });

        let sinker = state.clone();
        tokio::task::spawn(async move {
            loop {
                if sinker.sync_index().await.is_ok() {
                    log::info!("Initial bombastic index synced");
                    break;
                } else {
                    log::warn!("Bombastic index not yet available");
                }
                tokio::time::sleep(Duration::from_secs(5)).await;
            }

            loop {
                if let Err(e) = sinker.sync_index().await {
                    log::info!("Unable to synchronize bombastic index: {:?}", e);
                }
                tokio::time::sleep(sync_interval).await;
            }
        });

        Ok(state)
    }
}

pub(crate) type Index = IndexStore<bombastic_index::Index>;
pub struct AppState {
    storage: RwLock<Storage>,
    index: RwLock<Index>,
}

pub(crate) type SharedState = Arc<AppState>;

impl AppState {
    async fn sync_index(&self) -> Result<(), anyhow::Error> {
        let data = {
            let storage = self.storage.read().await;
            storage.get_index().await?
        };

        let mut index = self.index.write().await;
        index.reload(&data[..])?;
        log::debug!("Bombastic index reloaded");
        Ok(())
    }
}
