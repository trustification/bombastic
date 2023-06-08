use std::{net::SocketAddr, process::ExitCode, str::FromStr, time::Duration};

use trustification_infrastructure::{Infrastructure, InfrastructureConfig};

mod sbom;
mod server;

#[derive(clap::Args, Debug)]
#[command(about = "Run the api server", args_conflicts_with_subcommands = true)]
pub struct Run {
    #[arg(short, long, default_value = "0.0.0.0")]
    pub(crate) bind: String,

    #[arg(short = 'p', long = "port", default_value_t = 8080)]
    pub(crate) port: u16,

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
        Infrastructure::from(self.infra)
            .run(|| async {
                let storage = trustification_storage::create("bombastic", self.devmode, self.storage_endpoint)?;
                let addr = SocketAddr::from_str(&format!("{}:{}", self.bind, self.port))?;
                let interval = Duration::from_secs(self.sync_interval_seconds);

                server::run(storage, addr, interval).await
            })
            .await?;

        Ok(ExitCode::SUCCESS)
    }
}
