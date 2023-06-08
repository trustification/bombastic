mod search;

use search::*;

use csaf::{
    definitions::{NoteCategory, ProductIdT},
    product_tree::ProductTree,
    Csaf,
};
use serde_json::{Map, Value};
use sikula::prelude::*;

use std::ops::Bound;

use tracing::info;
use trustification_index::{
    create_boolean_query, create_date_query, primary2occur,
    tantivy::query::{BooleanQuery, Occur, Query, RangeQuery},
    tantivy::schema::{Field, Schema, Term, FAST, INDEXED, STORED, STRING, TEXT},
    tantivy::{doc, DateTime},
    term2query, Document, Error as SearchError,
};
use vexination_model::prelude::*;

pub struct Index {
    schema: Schema,
    fields: Fields,
}

struct Fields {
    id: Field,
    document_status: Field,
    title: Field,
    description: Field,
    cve: Field,
    advisory_initial: Field,
    advisory_current: Field,
    cve_release: Field,
    cve_discovery: Field,
    severity: Field,
    cvss: Field,
    product_status: Field,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ProductPackage {
    cpe: Option<String>,
    purl: Option<String>,
}

impl trustification_index::Index for Index {
    type MatchedDocument = SearchDocument;
    type Document = Csaf;

    fn index_doc(&self, csaf: &Csaf) -> Result<Vec<Document>, SearchError> {
        let mut documents = Vec::new();
        let id = &csaf.document.tracking.id;
        let document_status = match &csaf.document.tracking.status {
            csaf::document::Status::Draft => "draft",
            csaf::document::Status::Interim => "interim",
            csaf::document::Status::Final => "final",
        };

        if let Some(vulns) = &csaf.vulnerabilities {
            for vuln in vulns {
                let mut title = String::new();
                let mut description = String::new();
                let mut product_status: Map<String, Value> = Map::new();
                let mut cve = String::new();
                let mut severity = String::new();

                if let Some(t) = &vuln.title {
                    title = t.clone();
                }

                if let Some(c) = &vuln.cve {
                    cve = c.clone();
                }

                let mut score_value = 0.0;
                if let Some(scores) = &vuln.scores {
                    for score in scores {
                        if let Some(cvss3) = &score.cvss_v3 {
                            severity = cvss3.severity().as_str().to_string();
                            score_value = cvss3.score().value();
                            break;
                        }
                    }
                }

                if let Some(status) = &vuln.product_status {
                    let mut affected: Vec<ProductPackage> = Vec::new();
                    if let Some(products) = &status.known_affected {
                        for product in products {
                            if let Some(p) = find_product_package(csaf, product) {
                                affected.push(p);
                            }
                        }
                    }

                    let mut fixed: Vec<ProductPackage> = Vec::new();
                    if let Some(products) = &status.fixed {
                        for product in products {
                            if let Some(p) = find_product_package(csaf, product) {
                                fixed.push(p);
                            }
                        }
                    }

                    if let Ok(value) = serde_json::to_value(affected) {
                        product_status.insert("known_affected".to_string(), value);
                    }

                    if let Ok(value) = serde_json::to_value(fixed) {
                        product_status.insert("fixed".to_string(), value);
                    }
                }

                if let Some(notes) = &vuln.notes {
                    for note in notes {
                        if let NoteCategory::Description = note.category {
                            description = note.text.clone();
                            break;
                        }
                    }

                    tracing::debug!(
                        "Indexing with id {} title {} description {}, cve {}, product_status {:?} score {}",
                        id,
                        title,
                        description,
                        cve,
                        serde_json::to_string(&product_status).ok(),
                        score_value,
                    );
                    let mut document = doc!(
                        self.fields.id => id.as_str(),
                        self.fields.title => title,
                        self.fields.description => description,
                        self.fields.cve => cve,
                        self.fields.document_status => document_status,
                        self.fields.product_status => product_status,
                        self.fields.severity => severity,
                        self.fields.cvss => score_value,
                    );

                    document.add_date(
                        self.fields.advisory_initial,
                        DateTime::from_timestamp_millis(csaf.document.tracking.initial_release_date.timestamp_millis()),
                    );

                    document.add_date(
                        self.fields.advisory_current,
                        DateTime::from_timestamp_millis(csaf.document.tracking.current_release_date.timestamp_millis()),
                    );

                    if let Some(discovery_date) = &vuln.discovery_date {
                        document.add_date(
                            self.fields.cve_discovery,
                            DateTime::from_timestamp_millis(discovery_date.timestamp_millis()),
                        );
                    }

                    if let Some(release_date) = &vuln.release_date {
                        document.add_date(
                            self.fields.cve_release,
                            DateTime::from_timestamp_millis(release_date.timestamp_millis()),
                        );
                    }

                    documents.push(document);
                }
            }
        }
        Ok(documents)
    }

    fn schema(&self) -> Schema {
        self.schema.clone()
    }

    fn prepare_query(&self, q: &str) -> Result<Box<dyn Query>, SearchError> {
        let mut query = Vulnerabilities::parse(q).map_err(|err| SearchError::Parser(err.to_string()))?;

        query.term = query.term.compact();

        info!("Query: {query:?}");

        let query = term2query(&query.term, &|resource| self.resource2query(resource));

        info!("Processed query: {:?}", query);
        Ok(query)
    }

    fn process_hit(&self, doc: Document) -> Result<Self::MatchedDocument, SearchError> {
        // TODO Find a better way to do this
        if let Some(Some(advisory)) = doc.get_first(self.fields.id).map(|s| s.as_text()) {
            if let Some(Some(cve)) = doc.get_first(self.fields.cve).map(|s| s.as_text()) {
                if let Some(Some(title)) = doc.get_first(self.fields.title).map(|s| s.as_text()) {
                    if let Some(Some(description)) = doc.get_first(self.fields.description).map(|s| s.as_text()) {
                        if let Some(Some(cvss)) = doc.get_first(self.fields.cvss).map(|s| s.as_f64()) {
                            if let Some(Some(product_status)) =
                                doc.get_first(self.fields.product_status).map(|s| s.as_json())
                            {
                                if let Some(Some(release)) = doc.get_first(self.fields.cve_release).map(|s| s.as_date())
                                {
                                    let affected = product_status
                                        .get("affected")
                                        .map(|val| serde_json::from_value(val.clone()).unwrap_or(Vec::new()))
                                        .unwrap_or(Vec::new());

                                    let fixed = product_status
                                        .get("fixed")
                                        .map(|val| serde_json::from_value(val.clone()).unwrap_or(Vec::new()))
                                        .unwrap_or(Vec::new());
                                    return Ok(SearchDocument {
                                        advisory: advisory.to_string(),
                                        cve: cve.to_string(),
                                        title: title.to_string(),
                                        description: description.to_string(),
                                        cvss,
                                        release: release.into_utc(),
                                        affected,
                                        fixed,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
        Err(SearchError::NotFound)
    }
}

impl Default for Index {
    fn default() -> Self {
        Self::new()
    }
}

impl Index {
    pub fn new() -> Self {
        let mut schema = Schema::builder();
        let id = schema.add_text_field("id", STRING | FAST | STORED);
        let title = schema.add_text_field("title", TEXT | STORED);
        let description = schema.add_text_field("description", TEXT | STORED);
        let cve = schema.add_text_field("cve", STRING | FAST | STORED);
        let severity = schema.add_text_field("severity", STRING | FAST);
        let document_status = schema.add_text_field("document_status", STRING);
        let product_status = schema.add_json_field("product_status", STRING | STORED);
        let cvss = schema.add_f64_field("cvss", FAST | INDEXED | STORED);
        let advisory_initial = schema.add_date_field("advisory_initial_date", INDEXED);
        let advisory_current = schema.add_date_field("advisory_current_date", INDEXED);
        let cve_discovery = schema.add_date_field("cve_discovery_date", INDEXED);
        let cve_release = schema.add_date_field("cve_release_date", INDEXED | STORED);
        Self {
            schema: schema.build(),
            fields: Fields {
                id,
                cvss,
                title,
                description,
                cve,
                severity,
                document_status,
                product_status,
                advisory_initial,
                advisory_current,
                cve_discovery,
                cve_release,
            },
        }
    }

    fn resource2query(&self, resource: &Vulnerabilities) -> Box<dyn Query> {
        match resource {
            Vulnerabilities::Id(primary) => {
                let (occur, value) = primary2occur(primary);
                let term = Term::from_field_text(self.fields.id, value);
                create_boolean_query(occur, term)
            }

            Vulnerabilities::Cve(primary) => {
                let (occur, value) = primary2occur(primary);
                let term = Term::from_field_text(self.fields.cve, value);
                create_boolean_query(occur, term)
            }

            Vulnerabilities::Description(primary) => {
                let (occur, value) = primary2occur(primary);
                let term = Term::from_field_text(self.fields.description, value);
                create_boolean_query(occur, term)
            }

            Vulnerabilities::Title(primary) => {
                let (occur, value) = primary2occur(primary);
                let term = Term::from_field_text(self.fields.title, value);
                create_boolean_query(occur, term)
            }

            Vulnerabilities::Package(primary) => {
                let (occur, value) = primary2occur(primary);
                let term = Term::from_field_text(self.fields.product_status, value);
                create_boolean_query(occur, term)
            }

            Vulnerabilities::Severity(primary) => {
                let (occur, value) = primary2occur(primary);
                let term = Term::from_field_text(self.fields.severity, value);
                create_boolean_query(occur, term)
            }

            Vulnerabilities::Status(primary) => {
                let (occur, value) = primary2occur(primary);
                let term = Term::from_field_text(self.fields.document_status, value);
                create_boolean_query(occur, term)
            }
            Vulnerabilities::Final => {
                create_boolean_query(Occur::Must, Term::from_field_text(self.fields.document_status, "final"))
            }
            Vulnerabilities::Critical => {
                create_boolean_query(Occur::Must, Term::from_field_text(self.fields.severity, "critical"))
            }
            Vulnerabilities::High => {
                create_boolean_query(Occur::Must, Term::from_field_text(self.fields.severity, "high"))
            }
            Vulnerabilities::Medium => {
                create_boolean_query(Occur::Must, Term::from_field_text(self.fields.severity, "medium"))
            }
            Vulnerabilities::Low => {
                create_boolean_query(Occur::Must, Term::from_field_text(self.fields.severity, "low"))
            }
            Vulnerabilities::Cvss(ordered) => match ordered {
                PartialOrdered::Less(e) => Box::new(RangeQuery::new_f64_bounds(
                    self.fields.cvss,
                    Bound::Unbounded,
                    Bound::Excluded(*e),
                )),
                PartialOrdered::LessEqual(e) => Box::new(RangeQuery::new_f64_bounds(
                    self.fields.cvss,
                    Bound::Unbounded,
                    Bound::Included(*e),
                )),
                PartialOrdered::Greater(e) => Box::new(RangeQuery::new_f64_bounds(
                    self.fields.cvss,
                    Bound::Excluded(*e),
                    Bound::Unbounded,
                )),
                PartialOrdered::GreaterEqual(e) => Box::new(RangeQuery::new_f64_bounds(
                    self.fields.cvss,
                    Bound::Included(*e),
                    Bound::Unbounded,
                )),
                PartialOrdered::Range(from, to) => Box::new(RangeQuery::new_f64_bounds(self.fields.cvss, *from, *to)),
            },
            Vulnerabilities::Initial(ordered) => create_date_query(self.fields.advisory_initial, ordered),
            Vulnerabilities::Release(ordered) => {
                let q1 = create_date_query(self.fields.advisory_current, ordered);
                let q2 = create_date_query(self.fields.cve_release, ordered);
                Box::new(BooleanQuery::union(vec![q1, q2]))
            }
            Vulnerabilities::Discovery(ordered) => create_date_query(self.fields.cve_discovery, ordered),
        }
    }
}

use csaf::definitions::{BranchesT, ProductIdentificationHelper};
fn find_product_identifier<'m, F: Fn(&'m ProductIdentificationHelper) -> Option<R>, R>(
    branches: &'m BranchesT,
    product_id: &'m ProductIdT,
    f: &'m F,
) -> Option<R> {
    for branch in branches.0.iter() {
        if branch.name == product_id.0 {
            if let Some(name) = &branch.product {
                if let Some(helper) = &name.product_identification_helper {
                    if let Some(ret) = f(&helper) {
                        return Some(ret);
                    }
                }
            }
        }

        if let Some(branches) = &branch.branches {
            if let Some(ret) = find_product_identifier(&branches, product_id, f) {
                return Some(ret);
            }
        }
    }
    None
}

fn find_product_ref<'m>(tree: &'m ProductTree, product_id: &ProductIdT) -> Option<&'m ProductIdT> {
    if let Some(rs) = &tree.relationships {
        for r in rs {
            if r.full_product_name.product_id.0 == product_id.0 {
                return Some(&r.product_reference);
            }
        }
    }
    None
}

fn find_product_package(csaf: &Csaf, product_id: &ProductIdT) -> Option<ProductPackage> {
    if let Some(tree) = &csaf.product_tree {
        if let Some(r) = find_product_ref(tree, product_id) {
            if let Some(branches) = &tree.branches {
                return find_product_identifier(&branches, r, &|helper: &ProductIdentificationHelper| {
                    Some(ProductPackage {
                        purl: helper.purl.as_ref().map(|p| p.to_string()),
                        cpe: helper.cpe.as_ref().map(|p| p.to_string()),
                    })
                });
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use trustification_index::IndexStore;

    fn assert_free_form<F>(f: F)
    where
        F: FnOnce(IndexStore<Index>),
    {
        let _ = env_logger::try_init();

        let data = std::fs::read_to_string("../testdata/rhsa-2023_1441.json").unwrap();
        let csaf: Csaf = serde_json::from_str(&data).unwrap();
        let index = Index::new();
        let mut store = IndexStore::new_in_memory(index).unwrap();
        let mut writer = store.indexer().unwrap();
        writer.index(store.index(), &csaf).unwrap();
        writer.commit().unwrap();
        f(store);
    }

    #[tokio::test]
    async fn test_free_form_simple_primary() {
        assert_free_form(|index| {
            let result = index.search("openssl", 0, 100).unwrap();
            assert_eq!(result.0.len(), 1);
        });
    }

    #[tokio::test]
    async fn test_free_form_simple_primary_2() {
        assert_free_form(|index| {
            let result = index.search("CVE-2023-0286", 0, 100).unwrap();
            assert_eq!(result.0.len(), 1);
        });
    }

    #[tokio::test]
    async fn test_free_form_simple_primary_3() {
        assert_free_form(|index| {
            let result = index.search("RHSA-2023:1441", 0, 100).unwrap();
            assert_eq!(result.0.len(), 1);
        });
    }

    #[tokio::test]
    async fn test_free_form_primary_scoped() {
        assert_free_form(|index| {
            let result = index.search("RHSA-2023:1441 in:id", 0, 100).unwrap();
            assert_eq!(result.0.len(), 1);
        });
    }

    #[tokio::test]
    async fn test_free_form_predicate_final() {
        assert_free_form(|index| {
            let result = index.search("is:final", 0, 100).unwrap();
            assert_eq!(result.0.len(), 1);
        });
    }

    #[tokio::test]
    async fn test_free_form_predicate_high() {
        assert_free_form(|index| {
            let result = index.search("is:high", 0, 100).unwrap();
            assert_eq!(result.0.len(), 1);
        });
    }

    #[tokio::test]
    async fn test_free_form_predicate_critical() {
        assert_free_form(|index| {
            let result = index.search("is:critical", 0, 100).unwrap();
            assert_eq!(result.0.len(), 0);
        });
    }

    #[tokio::test]
    async fn test_free_form_ranges() {
        assert_free_form(|index| {
            let result = index.search("cvss:>5", 0, 100).unwrap();
            assert_eq!(result.0.len(), 1);

            let result = index.search("cvss:<5", 0, 100).unwrap();
            assert_eq!(result.0.len(), 0);
        });
    }

    #[tokio::test]
    async fn test_free_form_dates() {
        assert_free_form(|index| {
            let result = index.search("initial:>2022-01-01", 0, 100).unwrap();
            assert_eq!(result.0.len(), 1);

            let result = index.search("discovery:>2022-01-01", 0, 100).unwrap();
            assert_eq!(result.0.len(), 1);

            let result = index.search("release:>2022-01-01", 0, 100).unwrap();
            assert_eq!(result.0.len(), 1);

            let result = index.search("release:>2023-02-08", 0, 100).unwrap();
            assert_eq!(result.0.len(), 1);

            let result = index.search("release:2022-01-01..2023-01-01", 0, 100).unwrap();
            assert_eq!(result.0.len(), 0);

            let result = index.search("release:2022-01-01..2024-01-01", 0, 100).unwrap();
            assert_eq!(result.0.len(), 1);
        });
    }
}
