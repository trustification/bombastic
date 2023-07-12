use integration_tests::{assert_within_timeout, with_vexination};
use reqwest::StatusCode;
use serde_json::{Map, Value};
use std::time::Duration;

#[test]
fn test_vexination() {
    with_vexination(Duration::from_secs(60), |port| async move {
        let client = reqwest::Client::new();
        let input =
            serde_json::from_str::<Map<String, Value>>(include_str!("../../vexination/testdata/rhsa-2023_1441.json"))
                .unwrap();
        let response = client
            .post(format!("http://localhost:{port}/api/v1/vex"))
            .json(&input)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);

        assert_within_timeout(Duration::from_secs(30), async move {
            loop {
                // 1. Check that we can get the VEX back
                let response = client
                    .get(format!("http://localhost:{port}/api/v1/vex?advisory=RHSA-2023%3A1441"))
                    .header("Accept", "application/json")
                    .send()
                    .await
                    .unwrap();

                if response.status() == StatusCode::OK {
                    let body: Map<String, Value> = response.json().await.unwrap();
                    assert_eq!(body, input);

                    // 2. Make sure we can search for the VEX (takes some time)
                    let response = client
                        .get(format!("http://localhost:{port}/api/v1/vex/search?q="))
                        .send()
                        .await
                        .unwrap();

                    assert_eq!(response.status(), StatusCode::OK);
                    let payload: Value = response.json().await.unwrap();

                    if payload["total"].as_u64().unwrap() >= 1 {
                        break;
                    }
                }
                tokio::time::sleep(Duration::from_secs(4)).await;
            }
        })
        .await;
    })
}
