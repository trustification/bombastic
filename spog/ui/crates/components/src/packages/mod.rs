mod search;

use crate::cvss::CvssMap;
use crate::table_wrapper::TableWrapper;
use packageurl::PackageUrl;
use patternfly_yew::prelude::*;
pub use search::*;
use spog_model::package_info::PackageInfo;
use spog_ui_navigation::AppRoute;
use std::collections::HashMap;
use std::rc::Rc;
use std::str::FromStr;
use trustification_api::search::SearchResult;
use yew::prelude::*;
use yew_more_hooks::prelude::*;
use yew_nested_router::components::Link;

#[derive(PartialEq, Properties, Clone)]
pub struct PackagesEntry {
    package: PackageInfo,
    purl: PackageUrl<'static>,
    summary: HashMap<String, u64>,
}

#[derive(PartialEq, Properties)]
pub struct PackagesResultProperties {
    pub state: UseAsyncState<SearchResult<Rc<Vec<PackageInfo>>>, String>,
    pub onsort: Callback<(String, Order)>,
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum Column {
    Name,
    Namespace,
    Version,
    PackageType,
    Qualifiers,
    Path,
    Vulnerabilities,
}

impl TableEntryRenderer<Column> for PackagesEntry {
    fn render_cell(&self, context: CellContext<'_, Column>) -> Cell {
        match context.column {
            Column::Name => html!(
                <Link<AppRoute>
                    to={AppRoute::Package{id: self.package.purl.clone()}}
                >{ self.purl.name() }</Link<AppRoute>>
            )
            .into(),
            Column::Namespace => html!({ for self.purl.namespace() }).into(),
            Column::Version => Cell::from(html!({ for self.purl.version() })).text_modifier(TextModifier::Truncate),
            Column::PackageType => html!({ self.purl.ty() }).into(),
            Column::Path => html!({ for self.purl.subpath() }).into(),
            Column::Qualifiers => {
                html!({ for self.purl.qualifiers().iter().map(|(k,v)| html!(<Label label={format!("{k}={v}")} />)) })
                    .into()
            }
            Column::Vulnerabilities => {
                let l = self.summary.len();
                if l == 0 {
                    html!({ "N/A" }).into()
                } else {
                    html!(<CvssMap map={self.summary.clone()} />).into()
                }
            }
        }
    }

    fn is_full_width_details(&self) -> Option<bool> {
        Some(true)
    }

    fn render_details(&self) -> Vec<Span> {
        let html = html!();
        vec![Span::max(html)]
    }
}

fn get_package_definitions(package: PackageInfo) -> Option<PackagesEntry> {
    let mut summary = HashMap::new();
    for vuln in &package.vulnerabilities {
        *summary.entry(vuln.severity.clone()).or_default() += 1;
    }

    match PackageUrl::from_str(&package.purl) {
        Ok(purl) => Some(PackagesEntry { package, purl, summary }),
        Err(_) => None,
    }
}

#[function_component(PackagesResult)]
pub fn package_result(props: &PackagesResultProperties) -> Html {
    let data = match &props.state {
        UseAsyncState::Ready(Ok(val)) => {
            let data: Vec<_> = (*val.result)
                .clone()
                .into_iter()
                .filter_map(get_package_definitions)
                .collect();
            Some(data)
        }
        _ => None,
    };
    let sortby: UseStateHandle<Option<TableHeaderSortBy<Column>>> = use_state_eq(|| None);
    let _onsort = use_callback(
        (sortby.clone(), props.onsort.clone()),
        |val: TableHeaderSortBy<Column>, (sortby, onsort)| {
            sortby.set(Some(val));
            match &val.index {
                Column::Name => {
                    onsort.emit(("name".to_string(), val.order));
                }
                Column::Version => {
                    onsort.emit(("version".to_string(), val.order));
                }
                Column::PackageType => {
                    onsort.emit(("package_type".to_string(), val.order));
                }
                _ => {}
            }
        },
    );

    let (entries, onexpand) = use_table_data(MemoizedTableModel::new(Rc::new(data.unwrap_or_default())));

    let header = vec![
        yew::props!(TableColumnProperties<Column> {
            index: Column::Name,
            label: "Name",
            width: ColumnWidth::Percent(20),
        }),
        yew::props!(TableColumnProperties<Column> {
            index: Column::Namespace,
            label: "Namespace",
            width: ColumnWidth::Percent(20),
        }),
        yew::props!(TableColumnProperties<Column> {
            index: Column::Version,
            label: "Version",
            width: ColumnWidth::Percent(10),
        }),
        yew::props!(TableColumnProperties<Column> {
            index: Column::PackageType,
            label: "Type",
            width: ColumnWidth::Percent(10),
        }),
        yew::props!(TableColumnProperties<Column> {
            index: Column::Path,
            label: "Path",
            width: ColumnWidth::Percent(10),
        }),
        yew::props!(TableColumnProperties<Column> {
            index: Column::Qualifiers,
            label: "Qualifiers",
            width: ColumnWidth::Percent(10),
        }),
        yew::props!(TableColumnProperties<Column> {
            index: Column::Vulnerabilities,
            label: "Vulnerabilities",
            width: ColumnWidth::Percent(20),
        }),
    ];

    html!(
        <TableWrapper<Column, UseTableData<Column, MemoizedTableModel<PackagesEntry>>>
            loading={&props.state.is_processing()}
            error={props.state.error().cloned()}
            empty={entries.is_empty()}
            {header}
        >
            <Table<Column, UseTableData<Column, MemoizedTableModel<PackagesEntry>>>
                {entries}
                mode={TableMode::Default}
                {onexpand}
            />
        </TableWrapper<Column, UseTableData<Column, MemoizedTableModel<PackagesEntry>>>>
    )
}
