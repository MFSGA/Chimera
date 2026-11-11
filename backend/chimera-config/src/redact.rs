//! Credential redaction shared by runtime inspection and diagnostic archives.
//! Adapted from ref/backend/nyanpasu-config/src/redact.rs.

fn normalized_key(key: &str) -> String {
    key.to_ascii_lowercase().replace('_', "-")
}

fn is_sensitive_key(key: &str) -> bool {
    matches!(
        normalized_key(key).as_str(),
        "secret"
            | "password"
            | "token"
            | "private-key"
            | "auth-str"
            | "authentication"
            | "auth"
            | "authorization"
            | "uuid"
            | "username"
            | "user"
            | "psk"
            | "api-key"
            | "access-token"
            | "refresh-token"
            | "client-secret"
            | "credential"
            | "credentials"
    )
}

fn is_url_key(key: &str) -> bool {
    let key = normalized_key(key);
    key == "url" || key.ends_with("-url")
}

pub fn redact_config(mut value: serde_json::Value) -> serde_json::Value {
    match &mut value {
        serde_json::Value::Object(fields) => {
            for (key, value) in fields {
                if is_sensitive_key(key) {
                    *value = serde_json::Value::String("<redacted>".into());
                } else if matches!(
                    key.as_str(),
                    "external-controller" | "external-controller-tls"
                ) {
                    if let Some(address) = value.as_str()
                        && let Some((_, authority)) = address.rsplit_once('@')
                    {
                        *value = serde_json::Value::String(format!("<redacted>@{authority}"));
                    }
                } else if is_url_key(key) && value.is_string() {
                    // Subscription credentials can be embedded in either the
                    // query or path, so never expose provider URL text.
                    *value = serde_json::Value::String("<redacted>".into());
                } else {
                    *value = redact_config(std::mem::take(value));
                }
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                *value = redact_config(std::mem::take(value));
            }
        }
        _ => {}
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_nested_credentials_and_controller_authority() {
        assert_eq!(
            redact_config(serde_json::json!({
                "nested": [{"token": "abc", "password": "private"}],
                "external-controller": "u:p@host",
                "mmdb": "Country.mmdb"
            })),
            serde_json::json!({
                "nested": [{"token": "<redacted>", "password": "<redacted>"}],
                "external-controller": "<redacted>@host",
                "mmdb": "Country.mmdb"
            })
        );
    }

    #[test]
    fn redacts_proxy_credentials_and_subscription_urls() {
        let input = serde_json::json!({
            "proxies": [
                {"type": "vless", "uuid": "uuid-secret"},
                {"type": "hysteria", "auth": "auth-secret"},
                {"type": "socks5", "username": "proxy-user", "password": "proxy-pass"}
            ],
            "proxy-providers": {
                "private": {
                    "url": "https://user:pass@example.invalid/subscription/token-value?key=secret"
                }
            },
            "rules": [{"Authorization": "Bearer api-secret"}]
        });

        let redacted = redact_config(input);
        assert_eq!(redacted["proxies"][0]["uuid"], "<redacted>");
        assert_eq!(redacted["proxies"][1]["auth"], "<redacted>");
        assert_eq!(redacted["proxies"][2]["username"], "<redacted>");
        assert_eq!(redacted["proxy-providers"]["private"]["url"], "<redacted>");
        assert_eq!(redacted["rules"][0]["Authorization"], "<redacted>");

        let text = serde_json::to_string(&redacted).unwrap();
        for secret in [
            "uuid-secret",
            "auth-secret",
            "proxy-user",
            "proxy-pass",
            "token-value",
            "key=secret",
            "api-secret",
        ] {
            assert!(!text.contains(secret), "found {secret} in redacted config");
        }
    }
}
