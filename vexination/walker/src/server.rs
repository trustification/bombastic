use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::SystemTime;

use csaf_walker::discover::AsDiscovered;
use csaf_walker::model::metadata::Distribution;
use csaf_walker::report::{render_to_html, DocumentKey, Duplicates, ReportRenderOption, ReportResult};
use csaf_walker::visitors::duplicates::DetectDuplicatesVisitor;
use csaf_walker::visitors::filter::{FilterConfig, FilteringVisitor};
use csaf_walker::{
    retrieve::RetrievingVisitor,
    source::{FileSource, HttpSource},
    validation::{ValidatedAdvisory, ValidationError, ValidationVisitor},
    verification::{
        check::{init_verifying_visitor, CheckError},
        VerificationError, VerifiedAdvisory, VerifyingVisitor,
    },
    walker::Walker,
};
use reqwest::{header, StatusCode};
use trustification_auth::client::{TokenInjector, TokenProvider};
use url::Url;
use walker_common::{fetcher::Fetcher, since::Since, utils::url::Urlify, validate::ValidationOptions};

#[allow(clippy::too_many_arguments)]
pub async fn run(
    workers: usize,
    source: String,
    sink: Url,
    provider: Arc<dyn TokenProvider>,
    options: ValidationOptions,
    output: PathBuf,
    base_url: Option<Url>,
    ignore_distributions: Vec<Url>,
    since_file: Option<PathBuf>,
    additional_root_certificates: Vec<PathBuf>,
    only_prefixes: Vec<String>,
) -> Result<(), anyhow::Error> {
    let fetcher = Fetcher::new(Default::default()).await?;

    let mut client = reqwest::ClientBuilder::new();
    for cert in additional_root_certificates {
        let pem = std::fs::read(&cert)?;
        client = client.add_root_certificate(reqwest::tls::Certificate::from_pem(&pem)?);
    }

    let total = Arc::new(AtomicUsize::default());
    let duplicates: Arc<std::sync::Mutex<Duplicates>> = Default::default();
    let errors: Arc<std::sync::Mutex<BTreeMap<DocumentKey, String>>> = Default::default();
    let warnings: Arc<std::sync::Mutex<BTreeMap<DocumentKey, Vec<CheckError>>>> = Default::default();

    {
        let client = Arc::new(client.build()?);
        let total = total.clone();
        let duplicates = duplicates.clone();
        let errors = errors.clone();
        let warnings = warnings.clone();

        let visitor = move |advisory: Result<
            VerifiedAdvisory<ValidatedAdvisory, &'static str>,
            VerificationError<ValidationError, ValidatedAdvisory>,
        >| {
            (*total).fetch_add(1, Ordering::Release);

            let errors = errors.clone();
            let warnings = warnings.clone();
            let client = client.clone();
            let sink = sink.clone();
            let provider = provider.clone();
            async move {
                let adv = match advisory {
                    Ok(adv) => {
                        let sink = sink.clone();
                        let name = adv
                            .url
                            .path_segments()
                            .and_then(|s| s.last())
                            .unwrap_or_else(|| adv.url.path());

                        match serde_json::to_string(&adv.csaf.clone()) {
                            Ok(b) => {
                                const MAX_RETRIES: usize = 10;
                                for retry in 0..MAX_RETRIES {
                                    match client
                                        .post(sink.clone())
                                        .header(header::CONTENT_TYPE, "application/json")
                                        .body(b.clone())
                                        .inject_token(&provider)
                                        .await?
                                        .send()
                                        .await
                                    {
                                        Ok(r) if r.status() == StatusCode::CREATED => {
                                            log::info!("VEX ({}) stored successfully", &adv.csaf.document.tracking.id);
                                        }
                                        Ok(r) => {
                                            log::warn!(
                                        "(Skipped) {name}: Server's Error when storing VEX: {}, and wait to try again",
                                        r.status()
                                    );
                                            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                                            if retry == MAX_RETRIES - 1 {
                                                log::warn!("(Skipped) {name}: Error storing VEX: {}", r.status());
                                            }
                                        }
                                        Err(e) => {
                                            log::warn!("(Skipped) {name}: Client's Error when storing VEX: {e:?}");
                                            let _ = retry == MAX_RETRIES - 1;
                                        }
                                    };
                                }
                            }
                            Err(parse_err) => {
                                log::warn!("(Skipped) {name}: Serialization Error when storing VEX: {parse_err:?}");
                            }
                        };
                        adv
                    }
                    Err(err) => {
                        let name = match err.as_discovered().relative_base_and_url() {
                            Some((base, relative)) => DocumentKey {
                                distribution_url: base.clone(),
                                url: relative,
                            },
                            None => DocumentKey {
                                distribution_url: err.url().clone(),
                                url: Default::default(),
                            },
                        };

                        errors.lock().unwrap().insert(name, err.to_string());
                        return Ok::<_, anyhow::Error>(());
                    }
                };

                if !adv.failures.is_empty() {
                    let name = DocumentKey::for_document(&adv);
                    warnings
                        .lock()
                        .unwrap()
                        .entry(name)
                        .or_default()
                        .extend(adv.failures.into_values().flatten());
                }

                Ok::<_, anyhow::Error>(())
            }
        };
        let visitor = VerifyingVisitor::with_checks(visitor, init_verifying_visitor());
        let visitor = ValidationVisitor::new(visitor).with_options(options);

        let config = FilterConfig::new()
            .ignored_distributions(None)
            .ignored_prefixes(vec![])
            .only_prefixes(only_prefixes);

        if let Ok(url) = Url::parse(&source) {
            let since = Since::new(None::<SystemTime>, since_file, Default::default())?;

            log::info!("Walking VEX docs: source='{source}' workers={workers}");
            let source = HttpSource::new(url, fetcher, csaf_walker::source::HttpOptions::new().since(since.since));

            let visitor = { RetrievingVisitor::new(source.clone(), visitor) };
            let visitor = DetectDuplicatesVisitor { visitor, duplicates };
            let visitor = FilteringVisitor { visitor, config };
            Walker::new(source.clone())
                .with_distribution_filter(Box::new(move |distribution: &Distribution| {
                    !ignore_distributions.contains(&distribution.directory_url)
                }))
                .walk_parallel(workers, visitor)
                .await?;

            since.store()?;
        } else {
            log::info!("Walking VEX docs: path='{source}' workers={workers}");
            let source = FileSource::new(source, None)?;
            let visitor = { RetrievingVisitor::new(source.clone(), visitor) };
            let visitor = DetectDuplicatesVisitor { visitor, duplicates };
            let visitor = FilteringVisitor { visitor, config };

            Walker::new(source.clone())
                .with_distribution_filter(Box::new(move |distribution: &Distribution| {
                    !ignore_distributions.contains(&distribution.directory_url)
                }))
                .walk(visitor)
                .await?;
        }
    }

    let total = (*total).load(Ordering::Acquire);
    render(
        output,
        base_url,
        ReportResult {
            total,
            duplicates: &duplicates.lock().unwrap(),
            errors: &errors.lock().unwrap(),
            warnings: &warnings.lock().unwrap(),
        },
    )?;
    Ok(())
}

fn render(output: PathBuf, base_url: Option<Url>, report: ReportResult) -> anyhow::Result<()> {
    let mut out = std::fs::File::create(&output)?;
    render_to_html(&mut out, &report, ReportRenderOption { output, base_url })?;

    Ok(())
}
