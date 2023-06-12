#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unused_imports)]
use std::{net::SocketAddr, path::PathBuf, process::ExitCode, str::FromStr, sync::Arc, time::Duration};

use tokio::sync::RwLock;
use trustification_index::IndexStore;
use trustification_infrastructure::{Infrastructure, InfrastructureConfig};
use trustification_storage::Storage;

mod server;

#[derive(clap::Args, Debug)]
#[command(about = "Run the api server", args_conflicts_with_subcommands = true)]
pub struct Run {
    #[arg(short, long, default_value = "0.0.0.0")]
    pub(crate) bind: String,

    #[arg(short = 'p', long = "port", default_value_t = 8080)]
    pub(crate) port: u16,

    #[arg(short = 'i', long = "index")]
    pub(crate) index: Option<std::path::PathBuf>,

    #[arg(long = "sync-interval-seconds", default_value_t = 10)]
    pub(crate) sync_interval_seconds: u64,

    #[arg(long = "devmode", default_value_t = false)]
    pub(crate) devmode: bool,

    #[arg(long = "storage-endpoint", default_value = None)]
    pub(crate) storage_endpoint: Option<String>,

    #[command(flatten)]
    pub(crate) infra: InfrastructureConfig,
}

impl Run {
    pub async fn run(self) -> anyhow::Result<ExitCode> {
        let state = self.configure()?;
        Infrastructure::from(self.infra)
            .run(|| async {
                let addr = SocketAddr::from_str(&format!("{}:{}", self.bind, self.port))?;

                server::run(state, addr).await
            })
            .await?;

        Ok(ExitCode::SUCCESS)
    }

    fn configure(&self) -> anyhow::Result<Arc<AppState>> {
        let vexination_dir = self.index.clone().unwrap_or_else(|| {
            use rand::RngCore;
            let r = rand::thread_rng().next_u32();
            std::env::temp_dir().join(format!("vexination-index.{}", r))
        });

        std::fs::create_dir(&vexination_dir)?;

        let vexination_index = IndexStore::new(&vexination_dir, vexination_index::Index::new())?;
        let vexination_storage =
            trustification_storage::create("vexination", self.devmode, self.storage_endpoint.clone())?;

        let state = Arc::new(AppState {
            storage: RwLock::new(vexination_storage),
            index: RwLock::new(vexination_index),
        });

        let sync_interval = Duration::from_secs(self.sync_interval_seconds);

        let sinker = state.clone();
        tokio::task::spawn(async move {
            loop {
                if sinker.sync_index().await.is_ok() {
                    tracing::info!("Initial vexination index synced");
                    break;
                } else {
                    tracing::warn!("Vexination index not yet available");
                }
                tokio::time::sleep(Duration::from_secs(5)).await;
            }

            loop {
                if let Err(e) = sinker.sync_index().await {
                    tracing::info!("Unable to synchronize vexination index: {:?}", e);
                }
                tokio::time::sleep(sync_interval).await;
            }
        });

        Ok(state)
    }
}

pub(crate) type Index = IndexStore<vexination_index::Index>;
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
        tracing::debug!("Vexination index reloaded");
        Ok(())
    }
}
