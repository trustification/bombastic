use aws_config::meta::region::RegionProviderChain;
use aws_sdk_sqs::{
    config::Region, operation::receive_message::ReceiveMessageOutput, types::Message, Client, Error as SqsSdkError,
};
use thiserror::Error;

use crate::{Event, EventBus, EventConsumer};

#[derive(Debug, Error)]
pub enum SqsError {
    #[error("Error from SQS: {0}")]
    Sqs(SqsSdkError),
    #[error("No message received")]
    NoMessage,
}

impl From<SqsSdkError> for SqsError {
    fn from(e: SqsSdkError) -> Self {
        Self::Sqs(e)
    }
}

#[allow(unused)]
pub struct SqsEventBus {
    client: Client,
}

impl SqsEventBus {
    pub async fn new() -> Result<Self, anyhow::Error> {
        let region_provider = RegionProviderChain::default_provider().or_else(Region::new("eu-west-1"));
        let config = aws_config::from_env().region(region_provider).load().await;
        let client = Client::new(&config);
        Ok(Self { client })
    }
}

#[async_trait::async_trait]
impl EventBus for SqsEventBus {
    type Consumer<'m> = SqsConsumer<'m>;

    async fn create(&self, topics: &[&str]) -> Result<(), anyhow::Error> {
        for topic in topics.iter() {
            self.client.create_queue().queue_name(topic.to_string()).send().await?;
        }
        Ok(())
    }

    async fn subscribe(&self, _group: &str, topics: &[&str]) -> Result<Self::Consumer<'_>, anyhow::Error> {
        Ok(SqsConsumer {
            client: &self.client,
            queues: topics.iter().map(|s| s.to_string()).collect(),
        })
    }

    async fn send(&self, topic: &str, data: &[u8]) -> Result<(), anyhow::Error> {
        // TODO
        let s = core::str::from_utf8(data).unwrap();
        self.client
            .send_message()
            .queue_url(topic)
            .message_body(s)
            .send()
            .await?;
        Ok(())
    }
}

pub struct SqsConsumer<'m> {
    client: &'m Client,
    queues: Vec<String>,
}

#[async_trait::async_trait]
impl<'d> EventConsumer for SqsConsumer<'d> {
    type Event<'m> = SqsEvent<'m> where Self: 'm;
    async fn next<'m>(&'m self) -> Result<Option<Self::Event<'m>>, anyhow::Error> {
        let queue_futs: Vec<_> = self
            .queues
            .iter()
            .map(|q| {
                Box::pin(
                    self.client
                        .receive_message()
                        .set_wait_time_seconds(Some(20))
                        .set_max_number_of_messages(Some(1))
                        .queue_url(q.as_str())
                        .send(),
                )
            })
            .collect();

        let (result, idx, _) = futures::future::select_all(queue_futs).await;
        let topic = &self.queues[idx];
        let message: ReceiveMessageOutput = result?;
        if let Some(messages) = message.messages() {
            if let Some(message) = messages.first() {
                return Ok(Some(SqsEvent {
                    queue: topic.as_str(),
                    message: message.clone(),
                }));
            }
        }
        Ok(None)
    }

    async fn commit<'m>(&'m self, events: &[Self::Event<'m>]) -> Result<(), anyhow::Error> {
        for event in events {
            self.client
                .delete_message()
                .queue_url(event.queue)
                .set_receipt_handle(event.message.receipt_handle().map(|s| s.into()))
                .send()
                .await?;
        }
        Ok(())
    }
}

pub struct SqsEvent<'m> {
    queue: &'m str,
    message: Message,
}

#[async_trait::async_trait]
impl<'m> Event for SqsEvent<'m> {
    fn payload(&self) -> Option<&[u8]> {
        return self.message.body().map(|s| s.as_bytes());
    }

    fn topic(&self) -> &str {
        self.queue
    }
}
