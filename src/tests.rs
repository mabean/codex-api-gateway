use std::fs;

use tempfile::NamedTempFile;

use crate::ProxyServer;

#[tokio::test]
async fn parses_legacy_auth_file() {
    let file = NamedTempFile::new().unwrap();
    fs::write(
        file.path(),
        r#"{
            "OPENAI_API_KEY": null,
            "tokens": {
                "access_token": "legacy-token",
                "account_id": "acc_123"
            }
        }"#,
    )
    .unwrap();

    let proxy = ProxyServer::new(file.path().to_str().unwrap())
        .await
        .unwrap();
    assert_eq!(
        proxy.auth_data.access_token.as_deref(),
        Some("legacy-token")
    );
    assert_eq!(proxy.auth_data.account_id.as_deref(), Some("acc_123"));
}

#[tokio::test]
async fn parses_openclaw_auth_profiles_file() {
    let file = NamedTempFile::new().unwrap();
    let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF9hY2NvdW50X2lkIjoiYWNjX29wZW5jbGF3In19.signature";
    fs::write(
        file.path(),
        format!(
            r#"{{
                "profiles": {{
                    "profile-1": {{
                        "type": "oauth",
                        "access": "{}"
                    }}
                }},
                "lastGood": {{
                    "openai-codex": "profile-1"
                }}
            }}"#,
            jwt
        ),
    )
    .unwrap();

    let proxy = ProxyServer::new(file.path().to_str().unwrap())
        .await
        .unwrap();
    assert_eq!(proxy.auth_data.access_token.as_deref(), Some(jwt));
    assert_eq!(proxy.auth_data.account_id.as_deref(), Some("acc_openclaw"));
}

#[tokio::test]
async fn rejects_file_without_usable_credentials() {
    let file = NamedTempFile::new().unwrap();
    fs::write(file.path(), r#"{"tokens": {}}"#).unwrap();

    let result = ProxyServer::new(file.path().to_str().unwrap(), "https://example.test").await;
    assert!(result.is_err());
    let err = result.err().unwrap();
    let msg = err.to_string();
    assert!(
        msg.contains("parse")
            || msg.contains("supported auth file format")
            || msg.contains("usable")
    );
}
