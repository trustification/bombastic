use std::time::Duration;

use bombastic_index::Index;
use futures::pin_mut;
use tokio::select;
use trustification_event_bus::{Event, EventBus, EventConsumer};
use trustification_index::IndexStore;
use trustification_storage::{EventType, Storage};

pub async fn run<E: EventBus>(
    mut index: IndexStore<Index>,
    storage: Storage,
    bus: E,
    stored_topic: &str,
    indexed_topic: &str,
    failed_topic: &str,
    sync_interval: Duration,
) -> Result<(), anyhow::Error> {
    // Load initial index from storage.
    // TODO: Stream directly to file
    if let Ok(data) = storage.get_index().await {
        index.reload(&data[..])?;
    }

    let mut interval = tokio::time::interval(sync_interval);
    let mut events = 0;
    let mut indexer = Some(index.indexer()?);
    let consumer = bus.subscribe("indexer", &[stored_topic]).await?;
    let mut uncommitted_events = Vec::new();
    loop {
        let tick = interval.tick();
        pin_mut!(tick);
        select! {
            event = consumer.next() => match event {
                Ok(Some(event)) => {
                    if let Some(payload) = event.payload() {
                        if let Ok(data) = storage.decode_event(payload) {
                            for data in data.records {
                                if data.event_type() == EventType::Put {
                                    if storage.is_index(data.key()) {
                                        tracing::trace!("It's an index event, ignoring");
                                    } else {
                                        let key = data.key();
                                        match storage.get_for_event(&data).await {
                                            Ok(data) => {
                                                if let Ok(doc) = bombastic_index::SBOM::parse(&data) {
                                                    match indexer.as_mut().unwrap().index(index.index(), &doc) {
                                                        Ok(_) => {
                                                            tracing::trace!("Inserted entry into index");
                                                            bus.send(indexed_topic, key.as_bytes()).await?;
                                                            events += 1;
                                                        }
                                                        Err(e) => {
                                                            let failure = serde_json::json!( {
                                                                "key": key,
                                                                "error": e.to_string(),
                                                            }).to_string();
                                                            bus.send(failed_topic, failure.as_bytes()).await?;
                                                            tracing::warn!("Error inserting entry into index: {:?}", e)
                                                        }
                                                    }
                                                } else {
                                                    tracing::warn!("Error parsing SBOM for key {}, ignored", key);
                                                }
                                            }
                                            Err(e) => {
                                                tracing::debug!("Error retrieving document event data, ignoring (error: {:?})", e);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    uncommitted_events.push(event);
                }
                Ok(None) => {
                    tracing::debug!("Polling returned no events, retrying");
                }
                Err(e) => {
                    tracing::warn!("Error polling for event: {:?}", e);
                }
            },
            _ = tick => {
                if events > 0 {
                    tracing::debug!("{} new events added, pushing new index to storage", events);
                    match index.snapshot(indexer.take().unwrap()) {
                        Ok(data) => {
                            match storage.put_index(&data).await {
                                Ok(_) => {
                                    tracing::trace!("Index updated successfully");
                                    match consumer.commit(&uncommitted_events[..]).await {
                                        Ok(_) => {
                                            tracing::trace!("Event committed successfully");
                                            uncommitted_events.clear();
                                        }
                                        Err(e) => {
                                            tracing::warn!("Error committing event: {:?}", e)
                                        }
                                    }
                                    events = 0;
                                }
                                Err(e) => {
                                    tracing::warn!("Error updating index: {:?}", e)
                                }
                            }

                            indexer.replace(index.indexer()?);
                        }
                        Err(e) => {
                            tracing::warn!("Error taking index snapshot: {:?}", e);
                        }
                    }
                } else {
                    tracing::trace!("No changes to index");
                }
            }
        }
    }
}
