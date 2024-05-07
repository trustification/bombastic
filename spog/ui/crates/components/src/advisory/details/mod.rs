mod product;
mod remediation;

pub use product::*;
pub use remediation::*;

use std::ops::Deref;
use std::rc::Rc;

use crate::{
    advisory::{CsafNotes, CsafProductStatus, CsafReferences},
    common::CardWrapper,
    cvss::Cvss3,
};
use csaf::{vulnerability::Vulnerability, Csaf};
use patternfly_yew::prelude::*;
use spog_model::prelude::*;
use spog_ui_backend::{use_backend, VexService};
use spog_ui_common::{
    components::SafeHtml,
    utils::{time::chrono_date, OrNone},
};
use spog_ui_navigation::{AppRoute, View};

use yew::prelude::*;
use yew_more_hooks::hooks::use_async_with_cloned_deps;
use yew_nested_router::components::Link;
use yew_oauth2::prelude::use_latest_access_token;

#[derive(Clone, Properties)]
pub struct AdvisoryDetailsProps {
    pub advisory: Rc<AdvisorySummary>,
}

impl PartialEq for AdvisoryDetailsProps {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.advisory, &other.advisory)
    }
}

#[function_component(AdvisoryDetails)]
pub fn csaf_details(props: &AdvisoryDetailsProps) -> Html {
    let backend = use_backend();
    let access_token = use_latest_access_token();

    let summary = props.advisory.clone();

    let fetch = {
        use_async_with_cloned_deps(
            move |summary| async move {
                let service = VexService::new(backend.clone(), access_token);
                service
                    .lookup(&summary)
                    .await
                    .map(|result| result.map(Rc::new))
                    .map_err(|err| err.to_string())
            },
            (*summary).clone(),
        )
    };

    if let Some(Some(csaf)) = fetch.data() {
        let snippet = summary.desc.clone();
        html!(
            <Panel>
                <PanelMain>
                <PanelMainBody>
                <SafeHtml html={snippet} />
                <CsafVulnTable csaf={csaf.clone()}/>
                </PanelMainBody>
                </PanelMain>
            </Panel>
        )
    } else {
        html!(<></>)
    }
}

// vulns

#[derive(Clone, Copy, PartialEq, Eq)]
enum Column {
    Cve,
    Title,
    Cwe,
    Score,
    Discovery,
    Release,
    Products,
}

#[derive(PartialEq, Properties)]
pub struct CsafProperties {
    pub csaf: Rc<Csaf>,
}

#[derive(Clone)]
pub struct VulnerabilityWrapper {
    vuln: Vulnerability,
    csaf: Rc<Csaf>,
}

impl Deref for VulnerabilityWrapper {
    type Target = Vulnerability;

    fn deref(&self) -> &Self::Target {
        &self.vuln
    }
}

impl TableEntryRenderer<Column> for VulnerabilityWrapper {
    fn render_cell(&self, context: CellContext<'_, Column>) -> Cell {
        match context.column {
            Column::Cve => match &self.cve {
                Some(cve) => html!(
                    <Link<AppRoute>
                        to={AppRoute::Cve(View::Content { id: cve.clone() })}
                    >
                        {cve.clone()}
                    </Link<AppRoute>>
                ),
                None => html!(OrNone::<()>::DEFAULT_NA),
            },
            Column::Title => self.title.clone().map(Html::from).unwrap_or_default(),
            Column::Score => self
                .scores
                .clone()
                .unwrap_or_default()
                .into_iter()
                .filter_map(|s| s.cvss_v3)
                .map(|cvss| html!(<Cvss3 {cvss}/>))
                .collect::<Html>(),
            Column::Cwe => OrNone(self.cwe.clone().map(|cwe| {
                html!(
                    <Tooltip text={cwe.name}>
                        {cwe.id}
                    </Tooltip>
                )
            }))
            .into(),
            Column::Discovery => html!({ OrNone(self.discovery_date).map(chrono_date) }),
            Column::Release => html!({ OrNone(self.release_date).map(chrono_date) }),
            Column::Products => html!(
                <CsafProductStatus status={self.product_status.clone()} csaf={self.csaf.clone()} overview=true />
            ),
        }
        .into()
    }

    fn render_details(&self) -> Vec<Span> {
        let content = html!(
            <Grid gutter=true>

                <GridItem cols={[7]}>
                    <CardWrapper plain=true title="Product Status">
                        <CsafProductStatus status={self.product_status.clone()} csaf={self.csaf.clone()} overview=true />
                    </CardWrapper>
                </GridItem>

                <GridItem cols={[5]}>
                    <CardWrapper plain=true title="Remediations">
                        <CsafRemediationTable csaf={self.csaf.clone()} remediations={self.remediations.clone()} />
                    </CardWrapper>
                </GridItem>

                <GridItem cols={[6]}>
                    <CsafNotes plain=true notes={self.notes.clone()} />
                </GridItem>

                <GridItem cols={[4]}>
                    <CardWrapper plain=true title="IDs">
                        if let Some(ids) = &self.ids {
                            <List>
                                { for ids.iter().map(|id| {
                                    html_nested!(<ListItem>{&id.text}  {" ("} { &id.system_name } {")"}</ListItem>)
                                })}
                            </List>
                        }
                    </CardWrapper>
                </GridItem>

                <GridItem cols={[6]}>
                    <CsafReferences plain=true references={self.references.clone()} />
                </GridItem>

            </Grid>

        );

        vec![Span::max(content)]
    }
}

#[derive(PartialEq, Properties)]
pub struct CsafVulnTableProperties {
    pub csaf: Rc<Csaf>,
    #[prop_or_default]
    pub expandable: bool,
}

#[function_component(CsafVulnTable)]
pub fn vulnerability_table(props: &CsafVulnTableProperties) -> Html {
    let vulns = use_memo(props.csaf.clone(), |csaf| {
        csaf.vulnerabilities
            .clone()
            .into_iter()
            .flatten()
            .map(|vuln| VulnerabilityWrapper {
                vuln,
                csaf: csaf.clone(),
            })
            .collect::<Vec<_>>()
    });

    let (entries, onexpand) = use_table_data(MemoizedTableModel::new(vulns));

    let header = html_nested! {
        <TableHeader<Column>>
            <TableColumn<Column> label="CVE ID" index={Column::Cve} width={ColumnWidth::Percent(10)}/>
            <TableColumn<Column> label="Title" index={Column::Title} width={ColumnWidth::Percent(20)}/>
            <TableColumn<Column> label="Discovery" index={Column::Discovery} width={ColumnWidth::Percent(10)}/>
            <TableColumn<Column> label="Release" index={Column::Release} width={ColumnWidth::Percent(10)}/>
            <TableColumn<Column> label="Score" index={Column::Score} width={ColumnWidth::Percent(10)}/>
            <TableColumn<Column> label="CWE" index={Column::Cwe} width={ColumnWidth::Percent(10)}/>
            { for (!props.expandable).then(|| html_nested!(<TableColumn<Column> label="Products" index={Column::Products} width={ColumnWidth::Percent(30)} />))}
        </TableHeader<Column>>
    };

    let mode = match props.expandable {
        true => TableMode::CompactExpandable,
        false => TableMode::Compact,
    };

    html!(
        <Table<Column, UseTableData<Column, MemoizedTableModel<VulnerabilityWrapper>>>
            {mode}
            {header}
            {entries}
            {onexpand}
        />
    )
}
