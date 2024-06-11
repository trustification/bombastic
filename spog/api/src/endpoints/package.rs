use crate::app_state::AppState;
use crate::error::Error;
use crate::search;
use crate::service::guac::GuacService;
use actix_web::{
    web::{self, ServiceConfig},
    HttpResponse,
};
use actix_web_httpauth::extractors::bearer::BearerAuth;
use spog_model::package_info::{PackageInfo, V11yRef};
use spog_model::prelude::PackageProductDetails;
use std::sync::Arc;
use trustification_api::search::{SearchOptions, SearchResult};
use trustification_auth::authenticator::Authenticator;
use trustification_infrastructure::new_auth;
use utoipa::IntoParams;

pub(crate) fn configure(auth: Option<Arc<Authenticator>>) -> impl FnOnce(&mut ServiceConfig) {
    |config: &mut ServiceConfig| {
        config.service(
            web::scope("/api/v1/package")
                .wrap(new_auth!(auth))
                .service(web::resource("/search").to(package_search))
                .service(web::resource("/related").to(get_related))
                .service(web::resource("/dependencies").to(get_dependencies))
                .service(web::resource("/dependents").to(get_dependents))
                // these must come last, otherwise the path parameter will eat the rest
                .service(web::resource("/{id}").to(package_get))
                .service(web::resource("/{id}/related-products").to(package_related_products)),
        );
    }
}

#[utoipa::path(
    get,
    path = "/api/v1/package/search",
    responses(
        (status = 200, description = "packages search was successful", body = SearchResultPackage),
    ),
    params()
)]
pub async fn package_search(
    state: web::Data<AppState>,
    params: web::Query<search::QueryParams>,
    options: web::Query<SearchOptions>,
    access_token: Option<BearerAuth>,
) -> actix_web::Result<HttpResponse> {
    let params = params.into_inner();
    log::trace!("Querying package using {}", params.q);
    let data = state
        .search_package(
            &params.q,
            params.offset,
            params.limit,
            options.into_inner(),
            &access_token,
        )
        .await?;
    let mut m: Vec<PackageInfo> = Vec::with_capacity(data.result.len());
    for item in data.result {
        let item = item.document;
        m.push(PackageInfo {
            purl: item.purl,
            vulnerabilities: vec![],
        });
    }

    let result = SearchResult {
        total: Some(data.total),
        result: m,
    };

    Ok(HttpResponse::Ok().json(result))
}

#[utoipa::path(
    get,
    path = "/api/v1/package/{id}",
    responses(
        (status = OK, description = "packages was found", body = Vec<PackageInfo>),
        (status = NOT_FOUND, description = "packages was not found"),
    ),
    params(
        ("id" = Url, Path, description = "The ID of the package to retrieve")
    )
)]
pub async fn package_get(guac: web::Data<GuacService>, path: web::Path<String>) -> Result<HttpResponse, Error> {
    let purl = path.into_inner();
    let vex_results = guac.certify_vex(&purl).await?;
    let mut vex = vex_results
        .iter()
        .flat_map(|vex| {
            vex.vulnerability
                .vulnerability_ids
                .iter()
                .map(|id| V11yRef {
                    cve: id.vulnerability_id.clone().to_uppercase(),
                    severity: "unknown".to_string(),
                })
                .collect::<Vec<V11yRef>>()
        })
        .collect::<Vec<V11yRef>>();

    let vuln_results = guac.certify_vuln(&purl).await?;
    let mut vulns = vuln_results
        .iter()
        .flat_map(|vuln| {
            vuln.vulnerability
                .vulnerability_ids
                .iter()
                .map(|id| V11yRef {
                    cve: id.vulnerability_id.clone().to_uppercase(),
                    severity: "unknown".to_string(),
                })
                .collect::<Vec<V11yRef>>()
        })
        .collect::<Vec<V11yRef>>();
    vulns.append(&mut vex);

    let pkg = PackageInfo {
        purl,
        vulnerabilities: vulns,
    };
    Ok(HttpResponse::Ok().json(&pkg))
}

#[derive(Debug, serde::Deserialize, IntoParams)]
pub struct GetParams {
    /// ID of the SBOM to get vulnerabilities for
    pub offset: Option<i64>,
    pub limit: Option<i64>,
}

#[utoipa::path(
    get,
    path = "/api/v1/package/{id}/related-products",
    responses(
        (status = 200, description = "related products search was successful", body = PackageProductDetails),
    ),
    params(
        ("id" = Url, Path, description = "The ID of the package to retrieve"),
        GetParams
    )
)]
pub async fn package_related_products(
    guac: web::Data<GuacService>,
    path: web::Path<String>,
    params: web::Query<GetParams>,
) -> actix_web::Result<HttpResponse> {
    let id = path.into_inner();
    let related_products = guac.product_by_package(&id, params.offset, params.limit).await?;

    let result = PackageProductDetails { related_products };
    Ok(HttpResponse::Ok().json(&result))
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, IntoParams)]
pub struct GetPackage {
    pub purl: String,
}

#[utoipa::path(
    get,
    path = "/api/v1/package/related",
    responses(
        (status = OK, description = "Package was found", body = PackageRefList),
        (status = NOT_FOUND, description = "Package was not found")
    ),
    params(GetPackage)
)]
pub async fn get_related(
    guac: web::Data<GuacService>,
    web::Query(GetPackage { purl }): web::Query<GetPackage>,
) -> actix_web::Result<HttpResponse> {
    let pkgs = guac.get_packages(&purl).await?;

    Ok(HttpResponse::Ok().json(pkgs))
}

#[utoipa::path(
    get,
    path = "/api/v1/package/dependencies",
    responses(
        (status = OK, description = "Package was found", body = inline(spog_model::pkg::PackageDependencies)),
        (status = NOT_FOUND, description = "Package was not found")
    ),
    params(GetPackage)
)]
pub async fn get_dependencies(
    guac: web::Data<GuacService>,
    web::Query(GetPackage { purl }): web::Query<GetPackage>,
) -> actix_web::Result<HttpResponse> {
    let deps = guac.get_dependencies(&purl).await?;

    Ok(HttpResponse::Ok().json(deps))
}

#[utoipa::path(
    get,
    path = "/api/v1/package/dependents",
    responses(
        (status = OK, description = "Package was found", body = inline(spog_model::pkg::PackageDependents)),
        (status = NOT_FOUND, description = "Package was not found")
    ),
    params(GetPackage)
)]
pub async fn get_dependents(
    guac: web::Data<GuacService>,
    web::Query(GetPackage { purl }): web::Query<GetPackage>,
) -> actix_web::Result<HttpResponse> {
    let deps = guac.get_dependents(&purl).await?;

    Ok(HttpResponse::Ok().json(deps))
}
