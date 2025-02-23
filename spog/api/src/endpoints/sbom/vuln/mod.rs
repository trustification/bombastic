mod analyze;
mod backtrace;
mod vex;

use crate::app_state::AppState;
use crate::endpoints::sbom::vuln::analyze::AnalyzeOutcome;
use crate::error::Error;
use crate::search::QueryParams;
use crate::service::{guac::GuacService, v11y::V11yService};
use actix_web::cookie::time;
use actix_web::{web, HttpResponse};
use actix_web_httpauth::extractors::bearer::BearerAuth;
use analyze::analyze_spdx;
use bombastic_model::data::SBOM;
use bytes::BytesMut;
use cve::Cve;
use futures::stream::iter;
use futures::{StreamExt, TryStreamExt};
use serde_json::Value;
use spdx_rs::models::{PackageInformation, SPDX};
use spog_model::{
    prelude::{SbomReport, SummaryEntry},
    vuln::{SbomReportVulnerability, SourceDetails},
};
use std::collections::{BTreeMap, HashMap};
use std::str::FromStr;
use time::macros::format_description;
use time::OffsetDateTime;
use tracing::{info_span, instrument, Instrument};
use trustification_api::search::SearchResult;
use trustification_auth::client::TokenProvider;
use trustification_common::error::ErrorInformation;
use utoipa::IntoParams;
use v11y_model::search::SearchDocument;

/// chunk size for finding VEX by CVE IDs
const SEARCH_CHUNK_SIZE: usize = 10;
/// number of parallel fetches for VEX documents
const PARALLEL_FETCH_VEX: usize = 4;

#[derive(Debug, serde::Deserialize, IntoParams)]
pub struct GetParams {
    /// ID of the SBOM to get vulnerabilities for
    pub id: String,
    pub offset: Option<i64>,
    pub limit: Option<i64>,
    pub retrieve_remediation: Option<bool>,
}

#[utoipa::path(
    get,
    path = "/api/v1/sbom/vulnerabilities",
    responses(
        (status = OK, description = "Processing succeeded", body = SbomReport),
        (status = NOT_FOUND, description = "SBOM was not found")
    ),
    params(GetParams)
)]
#[instrument(skip(state, v11y, guac, access_token), err)]
pub async fn get_vulnerabilities(
    state: web::Data<AppState>,
    v11y: web::Data<V11yService>,
    guac: web::Data<GuacService>,
    params: web::Query<GetParams>,
    access_token: Option<BearerAuth>,
) -> actix_web::Result<HttpResponse> {
    if let Some(result) = process_get_vulnerabilities(&state, &v11y, &guac, &access_token, &params).await? {
        Ok(HttpResponse::Ok().json(result))
    } else {
        Ok(HttpResponse::NotFound().json(ErrorInformation {
            error: "NoPackageInformation".to_string(),
            message: "The selected SBOM did not contain any packages describing its content".to_string(),
            details: String::new(),
        }))
    }
}

#[instrument(skip(state, guac, v11y, access_token), err)]
pub async fn process_get_vulnerabilities(
    state: &AppState,
    v11y: &V11yService,
    guac: &GuacService,
    access_token: &dyn TokenProvider,
    params: &GetParams,
) -> Result<Option<SbomReport>, Error> {
    let id = &params.id;
    let offset = params.offset;
    let limit = params.limit;
    let retrieve_remediation = params.retrieve_remediation;
    // FIXME: avoid getting the full SBOM, but the search document fields only
    let sbom: BytesMut = state
        .get_sbom(id, access_token)
        .await?
        .try_collect()
        .instrument(info_span!("download SBOM data"))
        .await?;

    let sbom = SBOM::parse(&sbom).map_err(|err| Error::Generic(format!("Unable to parse SBOM: {err}")))?;
    let (name, version, created, analyze, backtraces) = match sbom {
        SBOM::SPDX(spdx) => {
            // get the main packages
            let main = find_main(&spdx);

            let AnalyzeOutcome {
                cve_to_purl,
                purl_to_backtrace,
            } = analyze_spdx(
                state,
                guac,
                access_token,
                &spdx.document_creation_information.spdx_document_namespace,
                offset,
                limit,
                retrieve_remediation,
            )
            .await?;

            let version = Some(
                main.iter()
                    .flat_map(|pi| pi.package_version.as_deref())
                    .collect::<Vec<_>>()
                    .join(", "),
            )
            .filter(|s| !s.is_empty());
            let name = spdx.document_creation_information.document_name;
            let created = time::OffsetDateTime::from_unix_timestamp(
                spdx.document_creation_information.creation_info.created.timestamp(),
            )
            .ok();

            (name, version, created, cve_to_purl, purl_to_backtrace)
        }
        SBOM::CycloneDX(cyclone) => {
            let name = cyclone
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.component.as_ref())
                .map(|component| component.name.to_string())
                .unwrap_or("".to_string());
            let version = Some(
                cyclone
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.component.as_ref())
                    .and_then(|component| component.version.as_ref().map(|version| version.to_string()))
                    .unwrap_or("".to_string()),
            );
            let created = cyclone
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.timestamp.as_ref())
                .and_then(|timestamp| {
                    let format = format_description!("[year]-[month]-[day]");
                    match OffsetDateTime::parse(timestamp.as_ref(), &format) {
                        Ok(time) => Some(time),
                        Err(_) => None,
                    }
                });

            let sbom_id = cyclone.serial_number.map(|e| e.to_string()).unwrap_or("".to_string());
            let AnalyzeOutcome {
                cve_to_purl,
                purl_to_backtrace,
            } = analyze_spdx(state, guac, access_token, &sbom_id, offset, limit, retrieve_remediation).await?;

            (name, version, created, cve_to_purl, purl_to_backtrace)
        }
    };

    // fetch CVE details

    let details = iter(analyze)
        .map(|(id, affected_packages)| async move {
            let q = format!("id:\"{}\"", id.clone());
            log::debug!("querying for {}", q);
            let query: QueryParams = QueryParams {
                q,
                offset: 0,
                limit: 100,
            };
            let SearchResult { result, total } = v11y.search(query).await.map_err(Error::V11y)?;
            log::debug!("{}/{:?} results found for {}", result.len(), total, id);
            match total {
                Some(1..) => {
                    result
                        .into_iter()
                        // in case the search returned multiple results, the one with the right id
                        // has to be picked to fill the response
                        .find(|cve| {
                            log::debug!("found {} while searching for {}", cve.document.id, id);
                            cve.document.id.to_lowercase() == id.to_lowercase()
                        })
                        .map_or(Ok(None), |cve| {
                            let mut sources = HashMap::new();
                            let score = Option::from(cve.document.cvss3x_score.unwrap_or(0f64) as f32);
                            log::debug!("score is {:?} for {}", score, id);
                            sources.insert("mitre".to_string(), SourceDetails { score });

                            let result = Ok(Some(SbomReportVulnerability {
                                id: cve.document.id.clone(),
                                description: get_description(&cve.document),
                                sources,
                                published: cve.document.date_published,
                                updated: cve.document.date_updated,
                                affected_packages,
                            }));
                            log::debug!("result is {:?}", result);
                            result
                        })
                }
                _ => Ok(None),
            }
        })
        .buffer_unordered(4)
        // filter out missing ones
        .try_filter_map(|r| async move { Ok::<_, Error>(r) })
        .try_collect::<Vec<_>>()
        .await?;

    // summarize scores

    let summary = summarize_vulns(&details)
        .into_iter()
        .map(|(source, counts)| {
            (
                source,
                counts
                    .into_iter()
                    .map(|(severity, count)| SummaryEntry { severity, count })
                    .collect::<Vec<_>>(),
            )
        })
        .collect();

    // done

    Ok(Some(SbomReport {
        name,
        version,
        created,
        summary,
        details,
        backtraces,
    }))
}

pub(crate) fn into_severity(score: f32) -> cvss::Severity {
    if score >= 9.0 {
        cvss::Severity::Critical
    } else if score >= 7.0 {
        cvss::Severity::High
    } else if score >= 4.0 {
        cvss::Severity::Medium
    } else if score > 0.0 {
        cvss::Severity::Low
    } else {
        cvss::Severity::None
    }
}

/// get the description
fn get_description(cve: &SearchDocument) -> Option<String> {
    Some(
        match cve.published {
            true => {
                if let Some(title) = cve.title.clone() {
                    return Some(title);
                }
                &cve.descriptions
            }
            false => &cve.descriptions,
        }
        .join(" :: "),
    )
}

/// get the CVSS score as a plain number
pub(crate) fn get_score(cve: &Cve) -> Option<f32> {
    let p = match cve {
        Cve::Published(p) => p,
        Cve::Rejected(_) => return None,
    };

    let score = |value: &Value| {
        value["vectorString"]
            .as_str()
            .and_then(|s| cvss::v3::Base::from_str(s).ok())
            .map(|base| base.score().value() as f32)
    };

    let mut v3_1 = None;
    let mut v3_0 = None;
    let mut v2_0 = None;

    for m in &p.containers.cna.metrics {
        if let Some(m) = m.cvss_v3_1.as_ref().and_then(score) {
            v3_1 = Some(m);
        } else if let Some(m) = m.cvss_v3_0.as_ref().and_then(score) {
            v3_0 = Some(m);
        } else if let Some(m) = m.cvss_v2_0.as_ref().and_then(score) {
            v2_0 = Some(m);
        }
    }

    // FIXME: we need to provide some indication what score version this was

    v3_1.or(v3_0).or(v2_0)
}

/// Collect a summary of count, based on CVSS v3 severities
fn summarize_vulns<'a>(
    vulnerabilities: impl IntoIterator<Item = &'a SbomReportVulnerability>,
) -> BTreeMap<String, BTreeMap<Option<cvss::Severity>, usize>> {
    let mut result = BTreeMap::<String, BTreeMap<_, _>>::new();

    for v in vulnerabilities.into_iter() {
        for (source, details) in &v.sources {
            let result = result.entry(source.clone()).or_default();
            let score = details.score.map(into_severity);
            *result.entry(score).or_default() += 1;
        }
    }

    result
}

/// Extract all purls which are referenced by "document describes"
#[instrument(skip_all)]
fn find_main(spdx: &SPDX) -> Vec<&PackageInformation> {
    let mut main = vec![];
    for desc in &spdx.document_creation_information.document_describes {
        for pi in &spdx.package_information {
            if &pi.package_spdx_identifier == desc {
                main.push(pi);
            }
        }
    }

    // FIXME: drop workaround, once the duplicate ID issue is fixed

    main.sort_unstable_by_key(|pi| &pi.package_spdx_identifier);
    main.dedup_by_key(|pi| &pi.package_spdx_identifier);

    // return

    main
}

/// map package information to it's purls
#[allow(unused)]
fn map_purls(pi: &PackageInformation) -> impl IntoIterator<Item = String> + '_ {
    pi.external_reference.iter().filter_map(|er| {
        if er.reference_type == "purl" {
            Some(er.reference_locator.clone())
        } else {
            None
        }
    })
}
