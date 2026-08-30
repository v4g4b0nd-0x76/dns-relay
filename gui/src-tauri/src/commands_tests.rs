use dns_relay::conf::{Conf, Relay, RelayTransport};
use tempfile::tempdir;

use crate::{
    commands::{CommandError, ServiceAction, ServiceState, materialize_for_apply},
    secrets::{FallbackVault, SecretId, SecretManager, SecretStore},
    state::draft_for_install_state,
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
    assert!(draft_for_install_state(true).is_none());
    assert!(draft_for_install_state(false).is_some());
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
