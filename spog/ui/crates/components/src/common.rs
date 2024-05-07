use patternfly_yew::prelude::*;
use yew::prelude::*;

#[derive(Clone, PartialEq, Properties)]
pub struct PageHeadingProperties {
    pub children: Children,
    #[prop_or_default]
    pub subtitle: Option<String>,
    #[prop_or(true)]
    pub sticky: bool,
    #[prop_or_default]
    pub action: Html,
}

#[function_component(PageHeading)]
pub fn page_heading(props: &PageHeadingProperties) -> Html {
    let sticky = match props.sticky {
        true => vec![PageSectionSticky::Top],
        false => vec![],
    };
    html!(
        <PageSection {sticky} variant={PageSectionVariant::Light} >
            <Flex>
                <FlexItem>
                    <Content>
                        <Title>{ for props.children.iter() }</Title>
                        if let Some(subtitle) = &props.subtitle {
                            <p>{ &subtitle }</p>
                        }
                    </Content>
                </FlexItem>
                <FlexItem modifiers={[FlexModifier::Align(Alignment::Right), FlexModifier::Align(Alignment::End)]}>
                    {props.action.clone()}
                </FlexItem>
            </Flex>
        </PageSection>
    )
}

#[function_component(NotFound)]
pub fn not_found() -> Html {
    html!(
        <EmptyState
            title="Not found"
            icon={Icon::Search}
            size={Size::Small}
        >
            { "The content requested could not be found." }
        </EmptyState>
    )
}

#[derive(PartialEq, Properties)]
pub struct CardWrapperProperties {
    pub title: AttrValue,

    #[prop_or_default]
    pub children: Children,

    #[prop_or_default]
    pub plain: bool,
}

#[function_component(CardWrapper)]
pub fn card_wrapper(props: &CardWrapperProperties) -> Html {
    html!(
        <Card plain={props.plain} full_height=true>
            <CardTitle><Title size={Size::XLarge}>{ props.title.clone() }</Title></CardTitle>
            <CardBody>
                { for props.children.iter() }
            </CardBody>
        </Card>
    )
}

#[derive(PartialEq, Properties)]
pub struct ExternalNavLinkProperties {
    pub href: AttrValue,
    pub children: Children,
}

#[function_component(ExternalNavLink)]
pub fn ext_nav_link(props: &ExternalNavLinkProperties) -> Html {
    html!(
        <NavLink target="_blank" href={&props.href}>
            { for props.children.iter() }
            {" "}
            <ExternalLinkMarker/>
        </NavLink>
    )
}

#[function_component(ExternalLinkMarker)]
pub fn ext_link_marker() -> Html {
    html!({ Icon::ExternalLinkAlt.with_classes(classes!("pf-v5-u-ml-sm", "pf-v5-u-color-200")) })
}
