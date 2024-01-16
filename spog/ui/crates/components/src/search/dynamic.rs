use crate::analytics::SearchContext;
use crate::search::{
    DefaultEntry, Search, SearchCategory, SearchDefaults, SearchOption, SearchOptionCheck, SearchOptionSelect,
    SearchOptionSelectItem,
};
use analytics_next::tracking;
use gloo_utils::format::JsValueSerdeExt;
use patternfly_yew::core::Order;
use spog_model::prelude::*;
use spog_ui_common::components::SafeHtml;
use spog_ui_common::utils::search::{escape_terms, or_group, SimpleProperties, ToFilterExpression};
use spog_ui_utils::analytics::UseAnalytics;
use std::collections::HashMap;
use std::rc::Rc;
use yew::prelude::*;

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DynamicSearchParameters {
    pub terms: Vec<String>,
    pub state: HashMap<String, Rc<String>>,
    /// Column name and whether or not it's sorted ascending
    pub sort: Option<(String, bool)>,
}

impl DynamicSearchParameters {
    fn id(cat: Rc<String>, id: Rc<String>) -> String {
        format!("{cat}/{id}")
    }

    pub fn get(&self, cat: Rc<String>, id: Rc<String>) -> Option<Rc<String>> {
        self.state.get(&Self::id(cat, id)).cloned()
    }

    pub fn set(&mut self, cat: Rc<String>, id: Rc<String>, value: Option<Rc<String>>) {
        if let Some(value) = value {
            self.state.insert(Self::id(cat, id), value);
        } else {
            self.state.remove(&Self::id(cat, id));
        }
    }

    pub fn set_sort_by(&mut self, (index, order): (String, Order)) {
        self.sort = Some((
            index,
            match order {
                Order::Ascending => true,
                Order::Descending => false,
            },
        ));
    }
}

impl SimpleProperties for DynamicSearchParameters {
    type Defaults = SearchDefaults;

    fn terms(&self) -> &[String] {
        &self.terms
    }

    fn terms_mut(&mut self) -> &mut Vec<String> {
        &mut self.terms
    }

    fn apply_defaults(&mut self, defaults: Self::Defaults) {
        for DefaultEntry { category, id, value } in defaults.0 {
            self.set(category, id, Some(value));
        }
    }
}

fn extend_terms(cat_terms: &mut Vec<String>, terms: &Terms) {
    // extend specific terms
    cat_terms.extend(terms.terms.clone());

    // extend evaluated terms
    if !terms.script.is_empty() {
        log::debug!("Eval terms: {}", terms.script);
        match js_sys::eval(&terms.script) {
            Ok(result) => match result.into_serde::<Vec<String>>() {
                Ok(terms) => {
                    log::debug!("Result: {terms:?}");
                    cat_terms.extend(terms);
                }
                Err(err) => {
                    log::warn!("Failed to deserialize result: {err}");
                }
            },
            Err(err) => {
                log::warn!("Failed to eval terms: {:?}", err.as_string());
            }
        }
    }
}

impl ToFilterExpression for DynamicSearchParameters {
    type Context = Filters;

    fn to_filter_expression(&self, context: &Self::Context) -> String {
        let mut terms = escape_terms(self.terms.clone()).collect::<Vec<_>>();

        for cat in &context.categories {
            let mut cat_terms = vec![];
            for opt in &cat.options {
                match opt {
                    FilterOption::Divider => {
                        // skip over dividers
                    }
                    FilterOption::Check(opt) => {
                        if self.get(Rc::new(cat.label.clone()), Rc::new(opt.id.clone())).is_some() {
                            extend_terms(&mut cat_terms, &opt.terms);
                        }
                    }
                    FilterOption::Select(opt) => {
                        if let Some(id) = self.get(Rc::new(cat.label.clone()), Rc::new(opt.group.clone())) {
                            for o in &opt.options {
                                if o.id == *id {
                                    extend_terms(&mut cat_terms, &o.terms);
                                }
                            }
                        }
                    }
                }
            }
            terms.extend(or_group(cat_terms));
        }

        if let Some(sort) = self.sort.clone() {
            let sort_prefix = if sort.1 { "" } else { "-" };
            terms.push(format!("{sort_prefix}sort:{}", sort.0));
        };

        terms.join(" ")
    }
}

pub fn convert_search(filters: &Filters, analytics: &UseAnalytics, context: SearchContext) -> (Search, SearchDefaults) {
    let mut defaults = vec![];

    let categories = filters
        .categories
        .iter()
        .map(|cat| SearchCategory {
            title: cat.label.clone(),
            options: cat
                .options
                .iter()
                .map(|opt| convert_option(&cat.label, opt, &mut defaults, analytics, context))
                .collect(),
        })
        .collect();

    (Search { categories, context }, SearchDefaults(defaults))
}

fn convert_option(
    cat_id: &str,
    opt: &FilterOption,
    defaults: &mut Vec<DefaultEntry>,
    analytics: &UseAnalytics,
    search: SearchContext,
) -> SearchOption {
    let cat_id = Rc::new(cat_id.to_string());

    #[derive(serde::Serialize)]
    #[tracking("Filter Check Change")]
    struct CheckEvent<'a> {
        category: &'a str,
        id: &'a str,
        value: bool,
        search: SearchContext,
    }

    #[derive(serde::Serialize)]
    #[tracking("Filter Radio Change")]
    struct RadioEvent<'a> {
        category: &'a str,
        id: &'a str,
        value: &'a str,
        search: SearchContext,
    }

    match opt {
        FilterOption::Divider => SearchOption::Divider,
        FilterOption::Check(opt) => {
            let id = Rc::new(opt.id.clone());
            let analytics = analytics.clone();

            SearchOption::Check(SearchOptionCheck {
                label: html!(<SafeHtml html={opt.label.clone()} />).into(),
                getter: {
                    let cat_id = cat_id.clone();
                    let id = id.clone();
                    Rc::new(move |state| state.get(cat_id.clone(), id.clone()).is_some())
                },
                setter: {
                    Rc::new(move |state, value| {
                        analytics.track(CheckEvent {
                            category: &cat_id,
                            id: &id,
                            value,
                            search,
                        });
                        state.set(cat_id.clone(), id.clone(), value.then(|| Rc::new(String::new())))
                    })
                },
            })
        }
        FilterOption::Select(select) => {
            let analytics = analytics.clone();
            let group = Rc::new(select.group.clone());

            if let Some(default) = &select.default {
                defaults.push(DefaultEntry {
                    category: cat_id.clone(),
                    id: group.clone(),
                    value: Rc::new(default.clone()),
                });
            }

            SearchOption::Select(SearchOptionSelect {
                options: select
                    .options
                    .iter()
                    .map(|option| {
                        let cat_id = cat_id.clone();
                        let group = group.clone();
                        let id = Rc::new(option.id.clone());
                        let analytics = analytics.clone();
                        SearchOptionSelectItem {
                            label: html!(<SafeHtml html={option.label.clone()} />).into(),
                            getter: {
                                let cat_id = cat_id.clone();
                                let group = group.clone();
                                let id = id.clone();
                                Rc::new(move |state: &DynamicSearchParameters| {
                                    state.get(cat_id.clone(), group.clone()).as_deref() == Some(&*id)
                                })
                            },
                            setter: {
                                Rc::new(move |state, event_value| {
                                    if event_value {
                                        analytics.track(RadioEvent {
                                            category: &cat_id,
                                            id: &group,
                                            value: &id,
                                            search,
                                        });
                                        // we only set the radio button which got set to true, which is the only even we get anyway
                                        state.set(cat_id.clone(), group.clone(), Some(id.clone()));
                                    }
                                })
                            },
                        }
                    })
                    .collect(),
            })
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_serialize() {
        let mut v = DynamicSearchParameters::default();
        v.set(
            Rc::new("cat".to_string()),
            Rc::new("id".to_string()),
            Some(Rc::new("value".to_string())),
        );
        v.set_sort_by(("field".into(), Order::Ascending));

        // must serialize to JSON
        serde_json::to_string(&v).unwrap();
    }
}
