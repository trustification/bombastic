use crate::Event;
use aws_config::{BehaviorVersion, SdkConfig};
use aws_credential_types::{credential_fn::provide_credentials_fn, provider::SharedCredentialsProvider};
use aws_sdk_sqs::{
    config::{Credentials, Region},
    operation::receive_message::ReceiveMessageOutput,
    types::Message,
    Client, Error,
};

impl From<Error> for crate::Error {
    fn from(e: Error) -> Self {
        match e {
            #[allow(deprecated)]
            Error::Unhandled(_) => Self::Critical(e.to_string()),
            Error::QueueDoesNotExist(_) => Self::Critical(e.to_string()),
            _ => Self::Transient(e.to_string()),
        }
    }
}

#[allow(unused)]
pub struct SqsEventBus {
    client: Client,
}

impl SqsEventBus {
    pub(crate) async fn new(
        access_key: impl Into<String>,
        secret_key: impl Into<String>,
        region: impl Into<String>,
    ) -> Result<Self, Error> {
        let creds = Credentials::new(access_key, secret_key, None, None, "trustification");
        let region = Region::new(region.into());
        let config = SdkConfig::builder()
            .region(region)
            .behavior_version(BehaviorVersion::latest())
            .credentials_provider(SharedCredentialsProvider::new(provide_credentials_fn(move || {
                let creds = creds.clone();
                async { Ok(creds) }
            })))
            .build();
        let client = Client::new(&config);
        Ok(Self { client })
    }
}

impl SqsEventBus {
    pub(crate) async fn create(&self, topics: &[&str]) -> Result<(), Error> {
        for topic in topics.iter() {
            self.client.create_queue().queue_name(topic.to_string()).send().await?;
        }
        Ok(())
    }

    pub(crate) async fn subscribe(&self, _group: &str, topics: &[&str]) -> Result<SqsConsumer, Error> {
        Ok(SqsConsumer {
            client: self.client.clone(),
            queues: topics.iter().map(|s| s.to_string()).collect(),
        })
    }

    pub(crate) async fn send(&self, topic: &str, data: &[u8]) -> Result<(), Error> {
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

pub struct SqsConsumer {
    client: Client,
    queues: Vec<String>,
}

impl SqsConsumer {
    pub(crate) async fn next(&self) -> Result<Option<SqsEvent<'_>>, Error> {
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
        if let Some(messages) = message.messages {
            if let Some(message) = messages.first() {
                return Ok(Some(SqsEvent {
                    queue: topic.as_str(),
                    message: message.clone(),
                }));
            }
        }
        Ok(None)
    }

    pub(crate) async fn commit<'m>(&'m self, events: &[Event<'m>]) -> Result<(), Error> {
        for event in events {
            if let Event::Sqs(event) = event {
                self.client
                    .delete_message()
                    .queue_url(event.queue)
                    .set_receipt_handle(event.message.receipt_handle().map(|s| s.into()))
                    .send()
                    .await?;
            }
        }
        Ok(())
    }
}

pub struct SqsEvent<'m> {
    queue: &'m str,
    message: Message,
}

impl<'m> SqsEvent<'m> {
    pub(crate) fn topic(&self) -> &str {
        self.queue
    }

    pub(crate) fn payload(&self) -> Option<&[u8]> {
        self.message.body().map(|m| m.as_bytes())
    }
}
