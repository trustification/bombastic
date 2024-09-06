mod inspect;

use crate::pages::scanner::{parse, validate};
use inspect::Inspect;
use patternfly_yew::prelude::*;
use spog_ui_components::upload_file::UploadFile;
use spog_ui_utils::config::*;
use std::rc::Rc;
use yew::prelude::*;
use yew_more_hooks::prelude::*;

const EMPTY_BODY_CONTENT: &str = r#"
<div>
    <p>Start by <strong>dragging and dropping a file here</strong> or clicking the <strong>Load an SBOM</strong> button.</p>
</div>
"#;

#[function_component(SbomUploader)]
pub fn sbom_uploader() -> Html {
    let content = use_state_eq(|| None::<Rc<String>>);
    let onsubmit = use_callback(content.clone(), |data, content| content.set(Some(data)));

    let sbom = use_memo(content.clone(), |content| {
        content
            .as_ref()
            .and_then(|data| parse(data.as_bytes()).ok().map(|sbom| (data.clone(), Rc::new(sbom))))
    });

    let onvalidate = use_callback((), |data: Rc<String>, ()| {
        let result = parse(data.as_bytes());
        match result {
            Ok(_sbom) => Ok(data),
            Err(err) => Err(format!("Failed to parse SBOM: {err}")),
        }
    });

    let onvalidate_warnings = use_callback((), |data: Rc<String>, ()| {
        let result = validate(data.as_bytes());
        match result {
            Ok(_sbom) => Ok(data),
            Err(err) => Err(format!("Warning: {err}")),
        }
    });

    // allow resetting the form
    let onreset = use_callback(content.clone(), move |_, content| {
        content.set(None);
    });

    match &*sbom {
        Some((raw, _bom)) => {
            html!(<Inspect {onreset} raw={(*raw).clone()} />)
        }
        None => {
            html!(
                <>
                    <CommonHeader />

                    <PageSection variant={PageSectionVariant::Light} fill=true>
                        <UploadFile
                            state_title="Get started by uploading your SBOM file"
                            state_content={Html::from_html_unchecked(AttrValue::from(EMPTY_BODY_CONTENT))}
                            primary_action_text="Load an SBOM"
                            submit_btn_text="Upload SBOM"
                            {onsubmit}
                            {onvalidate}
                            {onvalidate_warnings}
                        />
                    </PageSection>
                </>
            )
        }
    }
}

#[derive(PartialEq, Properties)]
pub struct CommonHeaderProperties {
    #[prop_or_default]
    pub onreset: Option<Callback<()>>,
}

#[function_component(CommonHeader)]
fn common_header(props: &CommonHeaderProperties) -> Html {
    let config = use_config_private();

    let onreset = use_map(props.onreset.clone(), move |callback| callback.reform(|_| ()));

    html!(
        <PageSection sticky={[PageSectionSticky::Top]} variant={PageSectionVariant::Light}>
            <Flex>
                <FlexItem>
                    <Content>
                        <Title>{"Upload an SBOM"}</Title>
                        <p>
                            {"Load an existing CycloneDX (1.3, 1.4, 1.5) or SPDX 2.2 file"}
                            if let Some(url) = &config.scanner.documentation_url {
                                {" or "}
                                <a
                                    href={url.to_string()} target="_blank"
                                    class="pf-v5-c-button pf-m-link pf-m-inline"
                                >
                                    {"learn about creating an SBOM"}
                                </a>
                            }
                            { "." }
                        </p>
                    </Content>
                </FlexItem>
                <FlexItem modifiers={[FlexModifier::Align(Alignment::Right), FlexModifier::Align(Alignment::End)]}>
                    if let Some(onreset) = onreset {
                        <Button
                            label={"Upload another"}
                            icon={Icon::Redo}
                            variant={ButtonVariant::Secondary}
                            onclick={onreset}
                        />
                    }
                </FlexItem>
            </Flex>
        </PageSection>
    )
}
