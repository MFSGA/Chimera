use serde_yaml::{Mapping, Value};

use super::*;

#[test]
fn ipc_payload_converts_to_persistent_overrides() {
    let overrides = ClashConfigOverrides::from(PatchRuntimeConfig {
        allow_lan: Some(true),
        ipv6: Some(false),
        log_level: Some("warning".to_string()),
        mode: Some("global".to_string()),
    });

    assert_eq!(
        overrides,
        ClashConfigOverrides {
            allow_lan: Some(true),
            ipv6: Some(false),
            log_level: Some("warning".to_string()),
            mode: Some("global".to_string()),
        }
    );
}

#[test]
fn mapping_conversion_ignores_core_only_fields() {
    let mapping = Mapping::from_iter([
        ("allow-lan".into(), Value::Bool(true)),
        ("mixed-port".into(), Value::Number(7890.into())),
        (
            "external-controller".into(),
            Value::String("127.0.0.1:9090".to_string()),
        ),
    ]);

    let overrides = ClashConfigOverrides::from_mapping(&mapping)
        .expect("core-only fields should not invalidate persistent overrides");

    assert_eq!(overrides.allow_lan, Some(true));
    assert_eq!(overrides.ipv6, None);
    assert_eq!(overrides.log_level, None);
    assert_eq!(overrides.mode, None);
}

#[test]
fn mapping_conversion_rejects_invalid_override_values() {
    let mapping = Mapping::from_iter([(
        "allow-lan".into(),
        Value::String("not-a-boolean".to_string()),
    )]);

    let error = ClashConfigOverrides::from_mapping(&mapping)
        .expect_err("invalid override values must not be silently discarded");

    assert!(error.to_string().contains("invalid Clash config overrides"));
}

#[test]
fn generated_runtime_patch_updates_only_override_fields() {
    let mut runtime = IRuntime {
        config: Some(Mapping::from_iter([
            ("allow-lan".into(), Value::Bool(false)),
            ("mode".into(), Value::String("rule".to_string())),
            ("mixed-port".into(), Value::Number(7890.into())),
        ])),
    };
    let overrides = ClashConfigOverrides {
        allow_lan: Some(true),
        mode: Some("global".to_string()),
        ..ClashConfigOverrides::default()
    };

    runtime.patch_config(&overrides);

    let config = runtime
        .config
        .expect("runtime config should remain present");
    assert_eq!(config.get("allow-lan"), Some(&Value::Bool(true)));
    assert_eq!(
        config.get("mode"),
        Some(&Value::String("global".to_string()))
    );
    assert_eq!(config.get("mixed-port"), Some(&Value::Number(7890.into())));
}

#[test]
fn override_mapping_contains_only_explicit_fields() {
    let overrides = ClashConfigOverrides {
        allow_lan: Some(false),
        log_level: Some("info".to_string()),
        ..ClashConfigOverrides::default()
    };

    assert_eq!(
        overrides.to_mapping(),
        Mapping::from_iter([
            ("allow-lan".into(), Value::Bool(false)),
            ("log-level".into(), Value::String("info".to_string())),
        ])
    );
}
