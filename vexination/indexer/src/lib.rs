use std::process::ExitCode;

use trustification_event_bus::EventBusConfig;
use trustification_index::{IndexConfig, IndexStore};
use trustification_infrastructure::{Infrastructure, InfrastructureConfig};
use trustification_storage::StorageConfig;
use vexination_index::Index;

mod indexer;

#[derive(clap::Args, Debug)]
#[command(about = "Run the indexer", args_conflicts_with_subcommands = true)]
pub struct Run {
    #[arg(long = "stored-topic", default_value = "vex-stored")]
    pub(crate) stored_topic: String,

    #[arg(long = "indexed-topic", default_value = "vex-indexed")]
    pub(crate) indexed_topic: String,

    #[arg(long = "failed-topic", default_value = "vex-failed")]
    pub(crate) failed_topic: String,

    #[arg(long = "devmode", default_value_t = false)]
    pub(crate) devmode: bool,

    #[command(flatten)]
    pub(crate) bus: EventBusConfig,

    #[command(flatten)]
    pub(crate) index: IndexConfig,

    #[command(flatten)]
    pub(crate) storage: StorageConfig,

    #[command(flatten)]
    pub(crate) infra: InfrastructureConfig,
}

impl Run {
    pub async fn run(mut self) -> anyhow::Result<ExitCode> {
        Infrastructure::from(self.infra)
            .run(|| async {
                let index = IndexStore::new(&self.index, Index::new())?;
                let storage = self.storage.create("vexination", self.devmode)?;
                let interval = self.index.sync_interval.into();
                let bus = self.bus.create().await?;
                if self.devmode {
                    bus.create(&[self.stored_topic.as_str()]).await?;
                }
                indexer::run(
                    index,
                    storage,
                    bus,
                    self.stored_topic.as_str(),
                    self.indexed_topic.as_str(),
                    self.failed_topic.as_str(),
                    interval,
                )
                .await
            })
            .await?;

        Ok(ExitCode::SUCCESS)
    }
}
