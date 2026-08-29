use std::fs;

#[cfg(unix)]
use std::os::unix::{fs::PermissionsExt, fs::symlink};
use tempfile::tempdir;
use uuid::Uuid;

use crate::{AdminAction, AdminRequest, parse_request_id, paths::read_request_at};

const REQUEST_ID: &str = "f47ac10b-58cc-4372-a567-0e02b2c3d479";

#[test]
fn action_rejects_unknown_variants() {
    let error = serde_json::from_str::<AdminRequest>(&format!(
        r#"{{"id":"{REQUEST_ID}","action":"shell"}}"#
    ))
    .unwrap_err();

    assert!(error.to_string().contains("unknown variant"));
}

#[test]
fn status_request_parses() {
    let request = serde_json::from_str::<AdminRequest>(&format!(
        r#"{{"id":"{REQUEST_ID}","action":"status"}}"#
    ))
    .unwrap();

    assert_eq!(request.id, Uuid::parse_str(REQUEST_ID).unwrap());
    assert_eq!(request.action, AdminAction::Status);
}

#[test]
fn malformed_request_id_is_rejected() {
    assert!(parse_request_id("../request").is_err());
}

#[cfg(unix)]
#[test]
fn request_reader_rejects_symlinks_and_wrong_parent() {
    let root = tempdir().unwrap();
    let requests = root.path().join("requests");
    let outside = root.path().join("outside");
    fs::create_dir_all(&requests).unwrap();
    fs::create_dir_all(&outside).unwrap();

    let id = Uuid::parse_str(REQUEST_ID).unwrap();
    let target = outside.join(format!("{id}.json"));
    fs::write(&target, format!(r#"{{"id":"{id}","action":"status"}}"#)).unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();

    assert!(read_request_at(&target, &requests, id).is_err());

    let link = requests.join(format!("{id}.json"));
    symlink(&target, &link).unwrap();
    assert!(read_request_at(&link, &requests, id).is_err());
}

#[cfg(unix)]
#[test]
fn request_reader_requires_mode_0600() {
    let root = tempdir().unwrap();
    let requests = root.path().join("requests");
    fs::create_dir_all(&requests).unwrap();

    let id = Uuid::parse_str(REQUEST_ID).unwrap();
    let path = requests.join(format!("{id}.json"));
    fs::write(&path, format!(r#"{{"id":"{id}","action":"status"}}"#)).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

    assert!(read_request_at(&path, &requests, id).is_err());
}

#[cfg(unix)]
#[test]
fn request_reader_accepts_an_owned_0600_status_request() {
    let root = tempdir().unwrap();
    let requests = root.path().join("requests");
    fs::create_dir_all(&requests).unwrap();

    let id = Uuid::parse_str(REQUEST_ID).unwrap();
    let path = requests.join(format!("{id}.json"));
    fs::write(&path, format!(r#"{{"id":"{id}","action":"status"}}"#)).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();

    assert_eq!(
        read_request_at(&path, &requests, id).unwrap().action,
        AdminAction::Status
    );
}
