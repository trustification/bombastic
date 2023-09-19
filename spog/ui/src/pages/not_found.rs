use patternfly_yew::prelude::*;
use spog_ui_navigation::AppRoute;
use yew::prelude::*;
use yew_nested_router::prelude::use_router;

#[function_component(NotFound)]
pub fn not_found() -> Html {
    let router = use_router();

    let onhome = use_callback(
        |_, router| {
            if let Some(router) = &router {
                router.push(AppRoute::Index);
            }
        },
        router,
    );

    html!(
        <Bullseye>
            <EmptyState
                title="404 – Page not found"
                size={Size::XXXLarge}
                primary={Action::new("Take me home", onhome)}
            >
            </EmptyState>
        </Bullseye>
    )
}
