use dns_relay::conf::{Conf, Relay, RelayTransport};
use tempfile::tempdir;

use crate::{
    commands::{
        CommandError, ServiceAction, ServiceState, bundled_paths_from_exe,
        config_change_requires_restart, materialize_for_apply, migrate_legacy_secrets,
        parse_config, read_bounded_lines, validate_draft, version_outputs_match,
    },
    secrets::{FallbackVault, SecretId, SecretManager, SecretStore},
    state::draft_for_install_files,
};

#[test]
fn command_error_has_stable_shape() {
    let value = serde_json::to_value(CommandError::field(
        "invalid_subnet",
        "Client subnet must be a public IPv4 /24",
        "clientSubnet",
    ))
    .unwrap();

    assert_eq!(value["code"], "invalid_subnet");
    assert_eq!(value["message"], "Client subnet must be a public IPv4 /24");
    assert_eq!(value["field"], "clientSubnet");
}

#[test]
fn command_error_omits_absent_field() {
    let value =
        serde_json::to_value(CommandError::new("unavailable", "Metrics unavailable")).unwrap();

    assert!(value.get("field").is_none());
}

#[test]
fn rule_only_changes_do_not_require_restart_when_hot_reload_is_enabled() {
    let saved = crate::state::starter_draft();
    let mut draft = saved.clone();
    draft.drop_list.push("ads.example".into());

    assert!(!config_change_requires_restart(Some(&saved), &draft));

    draft.dns_target = "127.0.0.1:5353".into();
    assert!(config_change_requires_restart(Some(&saved), &draft));
    assert!(config_change_requires_restart(None, &draft));
}

#[test]
fn service_contract_is_closed_and_serializes_as_strings() {
    assert_eq!(
        serde_json::to_string(&ServiceState::Running).unwrap(),
        r#""running""#
    );
    assert_eq!(
        serde_json::from_str::<ServiceAction>(r#""restart""#).unwrap(),
        ServiceAction::Restart
    );
    assert!(serde_json::from_str::<ServiceAction>(r#""shell""#).is_err());
}

#[test]
fn installed_service_does_not_get_a_default_editable_draft() {
    let (installed, recovery_required) = draft_for_install_files(true, true, true);
    assert!(installed.is_none());
    assert!(!recovery_required);
    let (draft, recovery_required) = draft_for_install_files(false, false, false);
    let draft = draft.unwrap();
    assert!(!recovery_required);
    assert_eq!(draft.dns_target, "127.0.0.1:53");
    assert!(draft.secure_only);
    assert_eq!(draft.resolvers, ["https://1.1.1.1/dns-query"]);
    assert!(draft.metric_conf.enable);
    assert!(draft.validate().is_ok());
}

#[test]
fn partial_install_gets_a_safe_repair_draft() {
    let (draft, recovery_required) = draft_for_install_files(false, false, false);
    assert!(draft.is_some());
    assert!(!recovery_required);

    let (draft, recovery_required) = draft_for_install_files(true, false, false);

    assert!(recovery_required);
    assert!(draft.unwrap().validate().is_ok());
    let (draft, recovery_required) = draft_for_install_files(true, true, true);
    assert!(draft.is_none());
    assert!(!recovery_required);

    let (draft, recovery_required) = draft_for_install_files(true, false, true);
    assert!(draft.is_none());
    assert!(recovery_required);
}

#[test]
fn development_sidecars_are_fixed_siblings_of_the_gui() {
    let (helper, resolver) = bundled_paths_from_exe(std::path::Path::new("/tmp/dns_relay_gui"));

    assert_eq!(helper, std::path::Path::new("/tmp/dns_relay_admin"));
    assert_eq!(resolver, std::path::Path::new("/tmp/dns_relay"));
}

#[test]
fn apply_version_guard_requires_exact_nonempty_output() {
    assert!(version_outputs_match(
        "dns-relay 1.6.10\n",
        "dns-relay 1.6.10"
    ));
    assert!(!version_outputs_match(
        "dns-relay 1.6.11",
        "dns-relay 1.6.10"
    ));
    assert!(!version_outputs_match("", ""));
}

#[test]
fn gui_validation_rejects_invalid_network_and_rule_fields() {
    let mut draft = crate::state::starter_draft();
    draft.dns_target = "not-a-listener".into();
    draft.resolvers = vec!["http://insecure.example/dns-query".into()];
    draft.drop_list = vec!["not a domain".into()];
    draft.redirect_list = vec![("ok.example".into(), "999.1.1.1".into())];
    draft.relay_conf.enable = true;
    draft.relay_conf.relay_instances.push(Relay {
        relay_key: String::new(),
        relay_url: "https://".into(),
        transport: RelayTransport::Direct,
    });

    let result = validate_draft(draft);

    assert!(!result.valid);
    assert!(
        result
            .errors
            .iter()
            .any(|error| error.field.as_deref() == Some("dnsTarget"))
    );
    assert!(
        result
            .errors
            .iter()
            .any(|error| error.code == "invalid_resolver")
    );
    assert!(
        result
            .errors
            .iter()
            .any(|error| error.code == "invalid_drop_rule")
    );
    assert!(
        result
            .errors
            .iter()
            .any(|error| error.code == "invalid_redirect_rule")
    );
    assert!(
        result
            .errors
            .iter()
            .any(|error| error.code == "invalid_relay_url")
    );
    assert!(
        result
            .errors
            .iter()
            .any(|error| error.code == "secret_reference_required")
    );
}

#[test]
fn redirect_rules_accept_only_ipv4_addresses() {
    let mut draft = crate::state::starter_draft();
    draft.redirect_list = vec![("router.example".into(), "::1".into())];

    let result = validate_draft(draft);

    assert!(
        result
            .errors
            .iter()
            .any(|error| error.code == "invalid_redirect_rule")
    );
}

#[test]
fn config_import_requires_vault_references_for_secrets() {
    let mut draft = crate::state::starter_draft();
    draft.relay_conf.relay_instances.push(Relay {
        relay_key: "plaintext".into(),
        relay_url: "https://relay.example".into(),
        transport: RelayTransport::Direct,
    });
    let error = match parse_config(draft.to_toml().unwrap()) {
        Ok(_) => panic!("plaintext secret import must fail"),
        Err(error) => error,
    };
    assert_eq!(error.code, "secret_reference_required");

    draft.relay_conf.relay_instances[0].relay_key = "vault://relay.primary".into();
    assert!(parse_config(draft.to_toml().unwrap()).is_ok());
}

#[test]
fn config_import_rejects_unknown_fields_instead_of_dropping_them() {
    let config = format!(
        "mystery_option = true\n{}",
        crate::state::starter_draft().to_toml().unwrap()
    );
    let error = match parse_config(config) {
        Ok(_) => panic!("unknown config fields must fail"),
        Err(error) => error,
    };

    assert_eq!(error.code, "unknown_config_field");
    assert_eq!(error.field.as_deref(), Some("mystery_option"));
}

#[test]
fn activity_reader_returns_only_the_latest_bounded_lines() {
    let root = tempdir().unwrap();
    let first = root.path().join("out.log");
    let second = root.path().join("err.log");
    std::fs::write(&first, "one\ntwo\n").unwrap();
    std::fs::write(&second, "three\nfour\n").unwrap();

    assert_eq!(
        read_bounded_lines(&[first, second], 3).unwrap(),
        ["two", "three", "four"]
    );
}

#[test]
fn generated_relay_keys_have_the_runtime_format() {
    let key = dns_relay::generate_relay_key();
    assert!(dns_relay::relay::load_key_from_str(&key).is_ok());
}

#[test]
fn gui_field_registry_covers_every_serialized_config_leaf() {
    let mut draft = crate::state::starter_draft();
    draft.drop_list.push("ads.example".into());
    draft
        .redirect_list
        .push(("router.example".into(), "10.0.0.1".into()));
    draft.client_subnet = Some([1, 1, 1]);
    draft
        .resolver_searching
        .resolver_source
        .push("https://example.test/list".into());
    draft.resolver_searching.resfresh_interval = Some(60);
    draft.relay_conf.relay_instances.push(Relay {
        relay_key: "vault://relay.primary".into(),
        relay_url: "https://relay.example".into(),
        transport: RelayTransport::Direct,
    });
    draft.record_history_conf = Some(dns_relay::conf::RecordHisotryConf {
        matched_list: vec!["*.example".into()],
        lines: 100,
    });
    draft.obfs_conf.keys.push("vault://obfs.primary".into());
    let mut serialized = Vec::new();
    collect_leaf_paths(&serde_json::to_value(draft).unwrap(), "", &mut serialized);
    serialized.sort();
    let registry: Vec<String> =
        serde_json::from_str(include_str!("../../src/config-fields.json")).unwrap();

    assert_eq!(serialized, registry);
}

fn collect_leaf_paths(value: &serde_json::Value, prefix: &str, output: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(entries) => {
            for (key, value) in entries {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                collect_leaf_paths(value, &path, output);
            }
        }
        serde_json::Value::Array(entries) => {
            let path = format!("{prefix}[]");
            if entries.first().is_some_and(serde_json::Value::is_object) {
                collect_leaf_paths(&entries[0], &path, output);
            } else {
                output.push(path);
            }
        }
        _ => output.push(prefix.into()),
    }
}

#[test]
fn apply_materializes_a_copy_without_exposing_secrets_in_the_draft() {
    let root = tempdir().unwrap();
    let vault = FallbackVault::new(root.path().join("vault.json"), "passphrase").unwrap();
    let store = SecretManager::<crate::secrets_tests::MemoryBackend>::encrypted_fallback(vault);
    let id = SecretId::new("relay.primary").unwrap();
    store.put(&id, b"rk_private").unwrap();
    let mut draft = Conf::default();
    draft.relay_conf.relay_instances.push(Relay {
        relay_key: "vault://relay.primary".into(),
        relay_url: "https://relay.example".into(),
        transport: RelayTransport::Direct,
    });

    let materialized = materialize_for_apply(&draft, &store).unwrap();

    assert_eq!(
        draft.relay_conf.relay_instances[0].relay_key,
        "vault://relay.primary"
    );
    assert_eq!(
        materialized.relay_conf.relay_instances[0].relay_key,
        "rk_private"
    );
}

#[test]
fn adoption_moves_legacy_plaintext_secrets_into_the_vault() {
    let root = tempdir().unwrap();
    let vault = FallbackVault::new(root.path().join("vault.json"), "passphrase").unwrap();
    let store = SecretManager::<crate::secrets_tests::MemoryBackend>::encrypted_fallback(vault);
    let mut draft = Conf::default();
    draft.relay_conf.relay_instances.push(Relay {
        relay_key: "legacy-relay-key".into(),
        relay_url: "https://relay.example".into(),
        transport: RelayTransport::Direct,
    });
    draft.obfs_conf.keys.push("legacy-obfs-key".into());

    migrate_legacy_secrets(&mut draft, &store).unwrap();

    assert!(
        draft.relay_conf.relay_instances[0]
            .relay_key
            .starts_with("vault://adopted.relay.")
    );
    assert!(draft.obfs_conf.keys[0].starts_with("vault://adopted.obfs."));
    let materialized = materialize_for_apply(&draft, &store).unwrap();
    assert_eq!(
        materialized.relay_conf.relay_instances[0].relay_key,
        "legacy-relay-key"
    );
    assert_eq!(materialized.obfs_conf.keys[0], "legacy-obfs-key");
}
