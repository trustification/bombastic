mod component;

pub use component::*;
use std::ops::Deref;

use analytics_next::{AnalyticsBrowser, Settings, TrackingEvent, User};
use openidconnect::LocalizedClaim;
use serde_json::{json, Value};
use spog_ui_backend::use_backend;
use spog_ui_common::utils::auth::claims;
use yew::prelude::*;
use yew_consent::prelude::*;
use yew_nested_router::history::History;
use yew_oauth2::prelude::use_auth_state;

#[derive(Clone, PartialEq)]
pub struct UseAnalytics {
    context: AnalyticsContext,
}

impl Deref for UseAnalytics {
    type Target = AnalyticsContext;

    fn deref(&self) -> &Self::Target {
        &self.context
    }
}

#[derive(Clone, Default, PartialEq)]
pub struct AnalyticsContext {
    analytics: Option<AnalyticsBrowser>,
}

impl AnalyticsContext {
    pub fn is_active(&self) -> bool {
        self.analytics.is_some()
    }

    pub fn identify(&self, user: impl Into<User>) {
        if let Some(analytics) = &self.analytics {
            analytics.identify(user);
        }
    }

    pub fn track<'a>(&self, event: impl Into<TrackingEvent<'a>>) {
        if let Some(analytics) = &self.analytics {
            analytics.track(event);
        }
    }

    pub fn page(&self) {
        if let Some(analytics) = &self.analytics {
            analytics.page();
        }
    }
}

/// Fetch the analytics context.
///
/// Possibly a "no-op" context if the user didn't consent to tracking or the call is done outside
/// a component wrapped by [`Segment`].
#[hook]
pub fn use_analytics() -> UseAnalytics {
    UseAnalytics {
        context: use_context::<AnalyticsContext>().unwrap_or_default(),
    }
}

#[derive(PartialEq, Properties)]
pub struct SegmentProperties {
    /// The segment.io "write key"
    #[prop_or_default]
    pub write_key: Option<String>,

    #[prop_or_default]
    pub children: Children,
}

/// Inject the segment tracking context, if permitted
#[function_component(Segment)]
pub fn segment(props: &SegmentProperties) -> Html {
    let consent = use_consent();
    let backend = use_backend();

    match (consent, backend.endpoints.external_consent) {
        // if we have consent, or consent is managed externally
        (_, true) | (ConsentState::Yes(()), _) => {
            let analytics = build(props.write_key.as_deref());
            let context = AnalyticsContext { analytics };

            html!(
                <ContextProvider<AnalyticsContext> {context}>
                    <SegmentPageTracker/>
                    { for props.children.iter() }
                </ContextProvider<AnalyticsContext>>
            )
        }
        // otherwise
        (ConsentState::No, false) => props.children.iter().collect(),
    }
}

#[function_component(SegmentPageTracker)]
pub fn segment_page_tracker() -> Html {
    let analytics = use_analytics();

    // trigger whenever it changes from here on
    use_effect_with(analytics, |analytics| {
        log::info!("Creating page tracker");
        let analytics = analytics.clone();

        // trigger once
        analytics.page();

        // and whenver it changes
        let listener = yew_nested_router::History::new().clone().listen(move || {
            analytics.page();
        });

        move || drop(listener)
    });

    html!()
}

pub trait BestLanguage {
    type Target;

    fn get(&self) -> Option<&Self::Target>;
}

impl<T> BestLanguage for Option<&T>
where
    T: BestLanguage,
{
    type Target = T::Target;

    fn get(&self) -> Option<&Self::Target> {
        self.and_then(|value| value.get())
    }
}

impl<T> BestLanguage for LocalizedClaim<T> {
    type Target = T;

    fn get(&self) -> Option<&Self::Target> {
        self.get(None)
    }
}

#[function_component(SegmentIdentify)]
pub fn segment_identify() -> Html {
    let analytics = use_analytics();
    let state = use_auth_state();

    let user = use_state_eq(User::default);

    use_effect_with((state, user.clone()), |(state, user)| {
        let claims = claims(state);
        let current = match claims {
            Some(claims) => User {
                id: Some(format!("{}#{}", **claims.issuer(), **claims.subject())),
                traits: json!({
                    "preferred_username": claims.preferred_username(),
                    "name": claims.name().get(),
                    "given_name": claims.given_name().get(),
                    "family_name": claims.family_name().get(),
                    "middle_name": claims.middle_name().get(),
                    "nickname": claims.nickname().get(),
                    "email": claims.email(),
                    "email_verified": claims.email_verified(),
                    "locale": claims.locale(),
                }),
                options: Value::Null,
            },
            None => User::default(),
        };
        user.set(current);
    });

    use_effect_with((analytics, (*user).clone()), |(analytics, user)| {
        log::info!("User changed: {user:?}");
        analytics.identify(user.clone());
    });

    html!()
}

fn build(write_key: Option<&str>) -> Option<AnalyticsBrowser> {
    write_key.map(|write_key| {
        AnalyticsBrowser::load(Settings {
            write_key: write_key.to_string(),
        })
    })
}

/// Wrap a callback with a tracking call
#[hook]
pub fn use_wrap_tracking<'a, IN, OUT, F, FO, D>(cb: Callback<IN, OUT>, f: F, deps: D) -> Callback<IN, OUT>
where
    IN: 'static,
    OUT: 'static,
    F: Fn(&IN, &D) -> FO + 'static,
    FO: Into<TrackingEvent<'static>> + 'static,
    D: Clone + PartialEq + 'static,
{
    let analytics = use_analytics();

    (*use_memo((cb, (analytics, deps)), |(cb, (analytics, deps))| {
        let cb = cb.clone();
        let analytics = analytics.clone();
        let deps = deps.clone();
        Callback::from(move |value| {
            analytics.track(f(&value, &deps).into());
            cb.emit(value)
        })
    }))
    .clone()
}

/// Create a tracking callback
#[hook]
pub fn use_tracking<'a, IN, F, FO, D>(f: F, deps: D) -> Callback<IN>
where
    IN: 'static,
    F: Fn(IN, &D) -> FO + 'static,
    FO: Into<TrackingEvent<'static>> + 'static,
    D: PartialEq + 'static,
{
    let analytics = use_analytics();

    use_callback((analytics, deps), move |values, (analytics, deps)| {
        if analytics.is_active() {
            analytics.track(f(values, deps));
        }
    })
}
