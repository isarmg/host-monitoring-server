use std::process::Command;

use host_monitoring_server::release_contract::{BinaryIdentity, SOURCE_REVISION, embedded};

#[test]
fn identity_command_reports_the_exact_embedded_current_contract() {
    let output = Command::new(env!("CARGO_BIN_EXE_host-monitoring-server"))
        .arg("identity")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());

    let reported: BinaryIdentity = serde_json::from_slice(&output.stdout).unwrap();
    let embedded = embedded().unwrap();
    assert_eq!(reported.manifest_format, embedded.manifest_format);
    assert_eq!(reported.application, embedded.application);
    assert_eq!(reported.version, embedded.version);
    assert_eq!(
        reported.schema_application_version,
        embedded.schema_application_version
    );
    assert_eq!(reported.api_prefix, embedded.api_prefix);
    assert_eq!(reported.schema_revision, embedded.schema_revision);
    assert_eq!(reported.schema_sha256, embedded.schema_sha256);
    assert_eq!(reported.target, embedded.target);
    assert_eq!(reported.source_revision, SOURCE_REVISION);
}
