mod details;
mod search;

pub use details::*;
pub use search::*;

use crate::{common::CardWrapper, cvss::CvssMap, download::Download, severity::Severity, table_wrapper::TableWrapper};
use csaf::{
    definitions::{Branch, Note, NoteCategory, ProductIdT, Reference, ReferenceCategory},
    document::{PublisherCategory, Status},
    product_tree::RelationshipCategory,
    vulnerability::{ProductStatus, RemediationCategory},
    Csaf,
};
use patternfly_yew::prelude::*;
use spog_model::csaf::{ProductsCache, RelationshipsCache};
use spog_model::prelude::*;
use spog_ui_backend::{use_backend, Endpoint};
use spog_ui_common::{
    components::Markdown,
    utils::{csaf::trace_product, time::date},
};
use spog_ui_navigation::{AppRoute, View};
use std::borrow::Cow;
use std::collections::HashSet;
use std::rc::Rc;
use trustification_api::search::SearchResult;
use url::Url;
use yew::prelude::*;
use yew_more_hooks::prelude::UseAsyncState;
use yew_nested_router::components::Link;

#[derive(PartialEq, Properties, Clone)]
pub struct AdvisoryEntry {
    summary: AdvisorySummary,
    url: Option<Url>,
}

#[derive(PartialEq, Properties)]
pub struct AdvisoryResultProperties {
    pub state: UseAsyncState<SearchResult<Rc<Vec<AdvisorySummary>>>, String>,
    pub onsort: Callback<(String, Order)>,
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum Column {
    Id,
    Title,
    Severity,
    Revision,
    Download,
    Vulnerabilities,
}

impl TableEntryRenderer<Column> for AdvisoryEntry {
    fn render_cell(&self, context: CellContext<'_, Column>) -> Cell {
        match context.column {
            Column::Id => html!(
                <Link<AppRoute>
                    to={AppRoute::Advisory(View::Content {id: self.summary.id.clone()})}
                >{ self.summary.id.clone() }</Link<AppRoute>>
            ),
            Column::Title => html!(&self.summary.title),
            Column::Severity => html!(
                if let Some(severity) = self.summary.severity.clone() {
                    <Severity {severity} />
                }
            ),
            Column::Revision => date(self.summary.date),
            Column::Download => html!(if let Some(url) = &self.url {
                <Download href={url.clone()} r#type="csaf"/>
            }),
            Column::Vulnerabilities => {
                let l = self.summary.cves.len();
                if l == 0 {
                    "N/A".to_string().into()
                } else if self.summary.cve_severity_count.is_empty() {
                    self.summary.cves.len().to_string().into()
                } else {
                    html!(<CvssMap map={self.summary.cve_severity_count.clone()} />)
                }
            }
        }
        .into()
    }

    fn is_full_width_details(&self) -> Option<bool> {
        Some(true)
    }

    fn render_details(&self) -> Vec<Span> {
        let html = html!( <AdvisoryDetails advisory={Rc::new(self.summary.clone())} />);
        vec![Span::max(html)]
    }
}

#[function_component(AdvisoryResult)]
pub fn advisory_result(props: &AdvisoryResultProperties) -> Html {
    let backend = use_backend();

    let data = use_state_eq(|| None);

    if let UseAsyncState::Ready(Ok(val)) = &props.state {
        let response: Vec<_> = val
            .result
            .iter()
            .map(|summary| {
                let url = backend.join(Endpoint::Api, &summary.href).ok();
                AdvisoryEntry {
                    summary: summary.clone(),
                    url,
                }
            })
            .collect();
        data.set(Some(response));
    }

    let sortby: UseStateHandle<Option<TableHeaderSortBy<Column>>> = use_state_eq(|| None);
    let onsort = use_callback(
        (sortby.clone(), props.onsort.clone()),
        |val: TableHeaderSortBy<Column>, (sortby, onsort)| {
            sortby.set(Some(val));
            if val.index == Column::Severity {
                onsort.emit(("severity".to_string(), val.order));
            };
        },
    );

    let (entries, onexpand) = use_table_data(MemoizedTableModel::new(Rc::new((*data).clone().unwrap_or_default())));

    let header = vec![
        yew::props!(TableColumnProperties<Column> {
            index: Column::Id,
            label: "ID",
            width: ColumnWidth::Percent(10)
        }),
        yew::props!(TableColumnProperties<Column> {
            index: Column::Title,
            label: "Title",
            width: ColumnWidth::Percent(50)
        }),
        yew::props!(TableColumnProperties<Column> {
            index: Column::Severity,
            label: "Aggregated Severity",
            width: ColumnWidth::Percent(10),
            text_modifier: Some(TextModifier::Wrap),
            sortby: *sortby,
            onsort: onsort.clone()
        }),
        yew::props!(TableColumnProperties<Column> {
            index: Column::Revision,
            label: "Revision",
            width: ColumnWidth::Percent(10)
        }),
        yew::props!(TableColumnProperties<Column> {
            index: Column::Vulnerabilities,
            label: "Vulnerabilities",
            width: ColumnWidth::Percent(20)
        }),
        yew::props!(TableColumnProperties<Column> {
            index: Column::Download,
            label: "Download",
            width: ColumnWidth::FitContent
        }),
    ];

    html!(
        <TableWrapper<Column, UseTableData<Column, MemoizedTableModel<AdvisoryEntry>>>
            loading={&props.state.is_processing()}
            error={props.state.error().cloned()}
            empty={entries.is_empty()}
            {header}
        >
            <Table<Column, UseTableData<Column, MemoizedTableModel<AdvisoryEntry>>>
                {entries}
                mode={TableMode::Expandable}
                {onexpand}
            />
        </TableWrapper<Column, UseTableData<Column, MemoizedTableModel<AdvisoryEntry>>>>
    )
}

pub fn cat_label(cat: &PublisherCategory) -> &'static str {
    match cat {
        PublisherCategory::Other => "Other",
        PublisherCategory::Coordinator => "Coordinator",
        PublisherCategory::Discoverer => "Discoverer",
        PublisherCategory::Translator => "Translator",
        PublisherCategory::User => "User",
        PublisherCategory::Vendor => "Vendor",
    }
}

pub fn tracking_status_str(status: &Status) -> &'static str {
    match status {
        Status::Draft => "Draft",
        Status::Interim => "Interim",
        Status::Final => "Final",
    }
}

#[derive(PartialEq, Properties)]
pub struct CsafReferencesProperties {
    pub references: Option<Vec<Reference>>,

    #[prop_or_default]
    pub plain: bool,
}

#[function_component(CsafReferences)]
pub fn csaf_references(props: &CsafReferencesProperties) -> Html {
    html!(
        <CardWrapper plain={props.plain} title="References">
            if let Some(references) = &props.references {
                <List>
                    { for references.iter().map(|reference| {
                        html_nested! ( <ListItem>
                            <a class="pf-v5-c-button pf-m-link" href={reference.url.to_string()} target="_blank">
                                { &reference.summary }
                                <span class="pf-v5-c-button__icon pf-m-end">
                                    { Icon::ExternalLinkAlt }
                                </span>
                            </a>
                            if let Some(category) = &reference.category {
                                <Label compact=true label={ref_cat_str(category)} color={Color::Blue} />
                            }
                        </ListItem>)
                    }) }
                </List>
            }
        </CardWrapper>
    )
}

pub fn ref_cat_str(category: &ReferenceCategory) -> &'static str {
    match category {
        ReferenceCategory::External => "external",
        ReferenceCategory::RefSelf => "self",
    }
}

#[derive(PartialEq, Properties)]
pub struct CsafNotesProperties {
    pub notes: Option<Vec<Note>>,

    #[prop_or_default]
    pub plain: bool,
}

#[function_component(CsafNotes)]
pub fn csaf_notes(props: &CsafNotesProperties) -> Html {
    html!(
        <CardWrapper plain={props.plain} title="Notes">
            <DescriptionList>
                { for props.notes.iter().flatten().map(|note| {
                    html!(
                        <DescriptionGroup
                            term={note_term(note).to_string()}
                        >
                            <Content>
                                <Markdown content={Rc::new(note.text.clone())}/>
                            </Content>
                        </DescriptionGroup>
                    )
                })}
            </DescriptionList>
        </CardWrapper>
    )
}

fn note_term(note: &Note) -> Cow<str> {
    match &note.title {
        Some(title) => format!("{title} ({})", note_cat_str(&note.category)).into(),
        None => note_cat_str(&note.category).into(),
    }
}

fn note_cat_str(category: &NoteCategory) -> &'static str {
    match category {
        NoteCategory::Description => "Description",
        NoteCategory::Details => "Details",
        NoteCategory::Faq => "FAQ",
        NoteCategory::General => "General",
        NoteCategory::LegalDisclaimer => "Legal Disclaimer",
        NoteCategory::Other => "Other",
        NoteCategory::Summary => "Summary",
    }
}

#[derive(PartialEq, Properties)]
pub struct CsafProductStatusProperties {
    pub status: Option<ProductStatus>,
    pub csaf: Rc<Csaf>,
    pub overview: bool,
}

#[function_component(CsafProductStatus)]
pub fn csaf_product_status(props: &CsafProductStatusProperties) -> Html {
    html!(
        if let Some(status) = &props.status {
            <DescriptionList>
                <CsafProductStatusSection title="First Affected" entries={status.first_affected.clone()} csaf={props.csaf.clone()} overview={props.overview} />
                <CsafProductStatusSection title="First Fixed" entries={status.first_fixed.clone()} csaf={props.csaf.clone()} overview={props.overview} />
                <CsafProductStatusSection title="Fixed" entries={status.fixed.clone()} csaf={props.csaf.clone()} overview={props.overview} />
                <CsafProductStatusSection title="Known Affected" entries={status.known_affected.clone()} csaf={props.csaf.clone()} overview={props.overview} />
                <CsafProductStatusSection title="Known Not Affected" entries={status.known_not_affected.clone()} csaf={props.csaf.clone()} overview={props.overview} />
                <CsafProductStatusSection title="Last Affected" entries={status.last_affected.clone()} csaf={props.csaf.clone()} overview={props.overview} />
                <CsafProductStatusSection title="Recommended" entries={status.recommended.clone()} csaf={props.csaf.clone()} overview={props.overview} />
                <CsafProductStatusSection title="Under Investigation" entries={status.under_investigation.clone()} csaf={props.csaf.clone()} overview={props.overview} />
            </DescriptionList>
        }
    )
}

#[derive(PartialEq, Properties)]
pub struct CsafProductStatusSectionProperties {
    pub title: AttrValue,
    pub entries: Option<Vec<ProductIdT>>,
    pub csaf: Rc<Csaf>,
    pub overview: bool,
}

#[function_component(CsafProductStatusSection)]
fn csaf_product_status(props: &CsafProductStatusSectionProperties) -> Html {
    let entries = use_memo(props.entries.clone(), |entries| {
        entries.as_ref().map(|entries| {
            let entries = match props.overview {
                false => {
                    let products = ProductsCache::new(&props.csaf);
                    let relationships = RelationshipsCache::new(&props.csaf);

                    entries
                        .iter()
                        .map(|entry| csaf_product_status_entry_details(&props.csaf, &products, &relationships, entry))
                        .collect::<Vec<_>>()
                }
                true => csaf_product_status_entry_overview(&props.csaf, entries),
            };

            entries
                .into_iter()
                .map(|i| html_nested!(<ListItem> {i} </ListItem>))
                .collect::<Vec<_>>()
        })
    });

    html!(
        if let Some(entries) = &*entries {
            <DescriptionGroup term={&props.title}>
                { entries }
            </DescriptionGroup>
        }
    )
}

#[derive(Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
enum Product<'a> {
    Known(&'a str),
    Invalid(&'a str),
}

fn csaf_resolve_aggregated_products<'a>(
    csaf: &'a Csaf,
    products_cache: &ProductsCache,
    entries: &'a [ProductIdT],
) -> HashSet<Product<'a>> {
    // gather unique set of products
    let products = entries.iter().map(|id| id.0.as_str()).collect::<HashSet<_>>();

    let cache = RelationshipsCache::new(csaf);

    // gather unique set of products they relate to
    products
        .into_iter()
        .flat_map(|id| {
            let mut products = cache
                .relations(id)
                .map(|rel| rel.relates_to_product_reference.0.as_str())
                .collect::<Vec<_>>();

            if products_cache.has_product(id) {
                products.push(id);
            }

            if products.is_empty() {
                vec![Product::Invalid(id)]
            } else {
                products.into_iter().map(Product::Known).collect::<Vec<_>>()
            }
        })
        .collect::<HashSet<_>>()
}

fn csaf_product_status_entry_overview(csaf: &Csaf, entries: &[ProductIdT]) -> Vec<Html> {
    let products_cache = ProductsCache::new(csaf);

    let products = csaf_resolve_aggregated_products(csaf, &products_cache, entries);

    // aggregate by name
    let mut products = products
        .into_iter()
        .flat_map(|product| match product {
            Product::Known(id) => products_cache.get(id).map(Product::Known),
            Product::Invalid(id) => Some(Product::Invalid(id)),
        })
        .collect::<Vec<_>>();

    products.sort_unstable();
    products.dedup();

    // render out first segment of those products
    products
        .into_iter()
        .map(|product| match product {
            // we resolved the id into a name
            Product::Known(name) => {
                html!({ name })
            }
            Product::Invalid(id) => render_invalid_product(id),
        })
        .collect()
}

fn csaf_product_status_entry_details(
    csaf: &Csaf,
    products: &ProductsCache,
    relationships: &RelationshipsCache,
    id: &ProductIdT,
) -> Html {
    // for details, we show the actual component plus where it comes from
    let actual = products.has_product(&id.0).then_some(id.0.as_str());
    let content = relationships
        .relations(&id.0)
        .map(|r| {
            // add product references
            let product = product_html(trace_product(csaf, &r.relates_to_product_reference.0));
            let relationship = html!(<Label label={rela_cat_str(&r.category)} compact=true />);
            let component = product_html(trace_product(csaf, &r.product_reference.0));

            html!(<>
                { component } {" "} { relationship } {" "} { product }
            </>)
        })
        .chain(actual.map(|product| {
            // add the direct product
            product_html(trace_product(csaf, product))
        }))
        .collect::<Vec<_>>();

    if content.is_empty() {
        render_invalid_product(&id.0)
    } else {
        Html::from_iter(content)
    }
}

fn render_invalid_product(id: &str) -> Html {
    let title = format!(r#"Invalid product ID: "{}""#, id);
    html!(<Alert {title} r#type={AlertType::Warning} plain=true inline=true />)
}

fn product_html(mut branches: Vec<&Branch>) -> Html {
    if let Some(first) = branches.pop() {
        branches.reverse();
        let text = branches
            .into_iter()
            .map(|b| b.name.clone())
            .collect::<Vec<_>>()
            .join(" » ");
        html! (
            <Tooltip {text}>
                { first.name.clone() }
            </Tooltip>
        )
    } else {
        html!()
    }
}

fn rela_cat_str(category: &RelationshipCategory) -> &'static str {
    match category {
        RelationshipCategory::DefaultComponentOf => "default component of",
        RelationshipCategory::ExternalComponentOf => "external component of",
        RelationshipCategory::InstalledOn => "installed on",
        RelationshipCategory::InstalledWith => "installed with",
        RelationshipCategory::OptionalComponentOf => "optional component of",
    }
}

fn rem_cat_str(remediation: &RemediationCategory) -> &'static str {
    match remediation {
        RemediationCategory::Mitigation => "mitigation",
        RemediationCategory::NoFixPlanned => "no fix planned",
        RemediationCategory::NoneAvailable => "none available",
        RemediationCategory::VendorFix => "vendor fix",
        RemediationCategory::Workaround => "workaround",
    }
}

#[allow(unused)]
fn branch_html(branches: Vec<&Branch>) -> Html {
    branches
        .iter()
        .rev()
        .enumerate()
        .map(|(n, branch)| {
            html!(<>
                if n > 0 {
                    { " » "}
                }
                {&branch.name} {" "}
            </>)
        })
        .collect()
}
