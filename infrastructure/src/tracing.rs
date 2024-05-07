use core::fmt;
use opentelemetry::propagation::Injector;
use opentelemetry::Context;
use reqwest::RequestBuilder;

#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq)]
pub enum Tracing {
    #[clap(name = "disabled")]
    Disabled,
    #[clap(name = "enabled")]
    Enabled,
}

impl Default for Tracing {
    fn default() -> Self {
        Self::Disabled
    }
}

impl fmt::Display for Tracing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Tracing::Disabled => write!(f, "disabled"),
            Tracing::Enabled => write!(f, "enabled"),
        }
    }
}

pub trait PropagateCurrentContext {
    fn propagate_current_context(self) -> Self
    where
        Self: Sized;
}

impl PropagateCurrentContext for reqwest::RequestBuilder {
    #[inline]
    fn propagate_current_context(self) -> Self
    where
        Self: Sized,
    {
        self.propagate_context(&Context::current())
    }
}

pub trait WithTracing {
    fn propagate_context(self, cx: &Context) -> Self;
}

impl WithTracing for RequestBuilder {
    fn propagate_context(self, cx: &Context) -> Self {
        let headers = opentelemetry::global::get_text_map_propagator(|prop| {
            let mut injector = HeaderInjector::new();
            prop.inject_context(cx, &mut injector);
            injector.0
        });
        self.headers(headers)
    }
}

struct HeaderInjector(http::HeaderMap);

impl HeaderInjector {
    pub fn new() -> Self {
        Self(Default::default())
    }
}

impl Injector for HeaderInjector {
    /// Set a key and value in the HeaderMap.  Does nothing if the key or value are not valid inputs.
    fn set(&mut self, key: &str, value: String) {
        if let Ok(name) = http::header::HeaderName::from_bytes(key.as_bytes()) {
            if let Ok(val) = http::header::HeaderValue::from_str(&value) {
                self.0.insert(name, val);
            }
        }
    }
}

/// Try getting the sampling rate from the environment variables
fn sampling_from_env() -> Option<f64> {
    std::env::var_os("OTEL_TRACES_SAMPLER_ARG").and_then(|s| s.to_str().and_then(|s| s.parse::<f64>().ok()))
}

fn sampler() -> opentelemetry::sdk::trace::Sampler {
    if let Some(p) = sampling_from_env() {
        opentelemetry::sdk::trace::Sampler::TraceIdRatioBased(p)
    } else {
        opentelemetry::sdk::trace::Sampler::TraceIdRatioBased(0.001)
    }
}

pub fn init_tracing(name: &str, tracing: Tracing) {
    match tracing {
        Tracing::Disabled => {
            init_no_tracing();
        }
        Tracing::Enabled => {
            init_jaeger(name);
        }
    }
}

fn init_jaeger(name: &str) {
    use tracing_subscriber::prelude::*;

    opentelemetry::global::set_text_map_propagator(opentelemetry::sdk::propagation::TraceContextPropagator::new());
    let pipeline = opentelemetry_jaeger::new_agent_pipeline()
        .with_service_name(name)
        .with_auto_split_batch(true)
        .with_trace_config(
            opentelemetry::sdk::trace::Config::default()
                .with_sampler(opentelemetry::sdk::trace::Sampler::ParentBased(Box::new(sampler()))),
        );

    println!("Using Jaeger tracing.");
    println!("{:#?}", pipeline);

    let tracer = pipeline
        .install_batch(opentelemetry::runtime::Tokio)
        .expect("unable to setup tracing pipeline");

    let formatting_layer = tracing_subscriber::fmt::Layer::default();

    if let Err(e) = tracing_subscriber::Registry::default()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_opentelemetry::layer().with_tracer(tracer))
        .with(formatting_layer)
        .try_init()
    {
        eprintln!("Error initializing tracing: {:?}", e);
    }
}

fn init_no_tracing() {
    if let Err(e) = env_logger::builder().format_timestamp_millis().try_init() {
        eprintln!("Error initializing logging: {:?}", e);
    }
}
