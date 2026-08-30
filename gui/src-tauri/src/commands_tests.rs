use dns_relay::conf::{Conf, Relay, RelayTransport};
use tempfile::tempdir;

use crate::{
    commands::{
        CommandError, ServiceAction, ServiceState, bundled_paths_from_exe, materialize_for_apply,
        migrate_legacy_secrets, version_outputs_match,
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
