use spog_ui_backend::Endpoints;
use spog_ui_common::error::components::ApiError as ApiErrorComponent;
use spog_ui_common::error::{ApiError, ApiErrorForStatus};
use std::rc::Rc;
use web_sys::RequestCache;
use yew::prelude::*;
use yew_more_hooks::hooks::r#async::*;

#[derive(Clone, Debug, PartialEq, Properties)]
pub struct BackendProperties {
    #[prop_or_default]
    pub children: Children,
    pub bootstrap_url: String,
}

#[function_component(Backend)]
pub fn backend(props: &BackendProperties) -> Html {
    let backend = use_async_with_options(
        async {
            log::info!("Discovering backend...");
            // we cannot use reqwest here, as we might need to do a relative lookup, based on the
            // current web page. Which is something that Url (which is used by reqwest) doesn't
            // support. But gloo_net does.
            let response = gloo_net::http::Request::get("/endpoints/backend.json")
                .cache(RequestCache::NoStore)
                .send()
                .await?
                .api_error_for_status()
                .await?;

            let endpoints: Endpoints = response.json().await?;

            log::info!("Found: {endpoints:?}");

            Ok::<_, ApiError>(spog_ui_backend::Backend { endpoints })
        },
        UseAsyncOptions::enable_auto(),
    );

    match &*backend {
        UseAsyncState::Pending | UseAsyncState::Processing => html!(),
        UseAsyncState::Ready(Err(err)) => html!(
            <ApiErrorComponent error={err.clone()} message="Failed to discover backend" />
        ),
        UseAsyncState::Ready(Ok(backend)) => html!(
            <ContextProvider<Rc<spog_ui_backend::Backend>> context={Rc::new(backend.clone())}>
                { for props.children.iter() }
            </ContextProvider<Rc<spog_ui_backend::Backend>>>
        ),
    }
}
