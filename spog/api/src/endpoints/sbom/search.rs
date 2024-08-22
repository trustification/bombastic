use crate::app_state::AppState;
use crate::endpoints::sbom::process_get_vulnerabilities;
use crate::search;
use crate::search::QueryParams;
use crate::service::guac::GuacService;
use crate::service::v11y::V11yService;
use actix_web::web::Query;
use actix_web::{web, HttpResponse};
use actix_web_httpauth::extractors::bearer::BearerAuth;
use serde::{Deserialize, Serialize};
use spog_model::search::SbomSummary;
use time::macros::offset;
use tracing::instrument;
use trustification_api::search::{SearchOptions, SearchResult};
use trustification_auth::client::TokenProvider;

#[utoipa::path(
    get,
    path = "/api/v1/sbom/search",
    responses(
        (status = OK, description = "Search was performed successfully", body = SearchResultSbom),
    ),
    params(
        search::QueryParams,
        SearchOptions,
    )
)]
#[instrument(skip(state, access_token), err)]
pub async fn search(
    state: web::Data<AppState>,
    params: web::Query<search::QueryParams>,
    options: web::Query<SearchOptions>,
    access_token: Option<BearerAuth>,
) -> actix_web::Result<HttpResponse> {
    let params = params.into_inner();
    log::trace!("Querying SBOM using {}", params.q);
    let data = state
        .search_sbom(
            &params.q,
            params.offset,
            params.limit,
            options.into_inner(),
            &access_token,
        )
        .await?;
    let mut m: Vec<SbomSummary> = Vec::with_capacity(data.result.len());
    for item in data.result {
        let metadata = item.metadata.unwrap_or_default();
        let item = item.document;
        m.push(SbomSummary {
            id: item.id.clone(),
            purl: item.purl,
            name: item.name,
            cpe: item.cpe,
            version: item.version,
            sha256: item.sha256,
            license: item.license,
            snippet: item.snippet,
            classifier: item.classifier,
            supplier: item.supplier.trim_start_matches("Organization: ").to_string(),
            href: format!("/api/v1/sbom?id={}", item.id),
            description: item.description,
            dependencies: item.dependencies,
            vulnerabilities: vec![],
            advisories: None,
            created: item.created,
            metadata,
        });
    }

    let mut result = SearchResult {
        total: Some(data.total),
        result: m,
    };

    // TODO: Use guac to lookup advisories for each sbom!
    search_advisories(state, &mut result.result, &access_token).await;
    Ok(HttpResponse::Ok().json(result))
}

#[instrument(skip_all)]
async fn search_advisories(state: web::Data<AppState>, sboms: &mut Vec<SbomSummary>, provider: &dyn TokenProvider) {
    for sbom in sboms {
        if let Some(q) = sbom.advisories_query() {
            if let Ok(result) = state
                .search_vex(
                    &q,
                    0,
                    100000,
                    SearchOptions {
                        explain: false,
                        metadata: false,
                        summaries: false,
                    },
                    provider,
                )
                .await
            {
                sbom.advisories = Some(result.total as u64);
            }
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Vulnerabilities {
    none: usize,
    low: usize,
    medium: usize,
    high: usize,
    critical: usize,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SbomVulnerabilitySummary {
    sbom_id: String,
    sbom_name: String,
    vulnerabilities: Vulnerabilities,
}

pub async fn sboms_with_vulnerability_summary(
    state: web::Data<AppState>,
    options: web::Query<SearchOptions>,
    access_token: Option<BearerAuth>,
    guac: web::Data<GuacService>,
    v11y: web::Data<V11yService>,
) -> actix_web::Result<HttpResponse> {
    let data = state
        .search_sbom("-sort:indexedTimestamp", 0, 10, options.into_inner(), &access_token)
        .await?;

    let mut summary: Vec<SbomVulnerabilitySummary> = vec![];
    for item in data.result {
        let metadata = item.metadata.unwrap_or_default();
        let item = item.document;
        let sbomReport = process_get_vulnerabilities(
            &state.clone(),
            &v11y.clone(),
            &guac.clone(),
            &access_token.clone(),
            &item.id,
            Some(0),
            Some(100000),
        )
        .await?;
        let mut vulnSummary: Vulnerabilities = Vulnerabilities {
            none: 0,
            low: 0,
            medium: 0,
            high: 0,
            critical: 0,
        };
        let sbomReportSummary = sbomReport.unwrap().summary;
        for (key, value) in sbomReportSummary {
            for summaryEntry in value {
                let severity = summaryEntry.severity.unwrap();
                let count = summaryEntry.count;
                match severity.as_str() {
                    "Low" => vulnSummary.low += count,
                    "Medium" => vulnSummary.medium += count,
                    "High" => vulnSummary.high += count,
                    "Critical" => vulnSummary.critical += count,
                    _ => vulnSummary.none += count,
                }
            }
        }
        let sbomVulnSum: SbomVulnerabilitySummary = SbomVulnerabilitySummary {
            sbom_id: item.id,
            sbom_name: item.name,
            vulnerabilities: vulnSummary,
        };
        summary.push(sbomVulnSum);
    }
    Ok(HttpResponse::Ok().json(summary))
}
