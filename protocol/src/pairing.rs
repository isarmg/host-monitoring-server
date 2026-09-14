//! Pairing and report acknowledgement wire types.
//!
//! These DTOs intentionally contain no trust-boundary validation beyond strict JSON shape and
//! canonical UUID decoding. The Server still owns policy checks such as hash format, supported
//! activation-code limits, and pairing state transitions. Release versions are
//! diagnostics only; wire compatibility is negotiated explicitly below.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};

use crate::{HostIdentity, report::deserialize_canonical_uuid};

/// Exact HTTP surface shared by Host Monitoring 0.7 Server and Client.
/// There are deliberately no aliases for the former module-prefixed routes.
pub const API_PREFIX: &str = "/api/v2";
pub const CLIENT_REPORT_PATH: &str = "/api/v2/host-monitor/report";
pub const CLIENT_CREDENTIAL_STATUS_PATH: &str = "/api/v2/host-monitor/credential-status";
pub const CLIENT_PAIRING_REQUESTS_PATH: &str = "/api/v2/host-monitor/pairing-requests";
pub const CLIENT_PAIRING_REQUEST_PATH: &str = "/api/v2/host-monitor/pairing-requests/{request_id}";
pub const CLIENT_PAIRING_STATUS_PATH: &str =
    "/api/v2/host-monitor/pairing-requests/{request_id}/status";
pub const CLIENT_ACTIVATE_PATH: &str = "/api/v2/host-monitor/activate";
pub const CLIENT_ADMIN_ACTIVATE_PATH: &str = "/api/v2/host-monitor/activate-admin";
pub const BROWSER_ACTIVATION_PATH_PREFIX: &str = "/activate/";

/// Pairing wire contract implemented by this release.
///
/// A missing field decodes as version 1 so a Server can be rolled out before
/// all existing Clients have started sending the explicit discriminator.
pub const HOST_PAIRING_PROTOCOL_VERSION: u16 = 1;

const fn default_host_pairing_protocol_version() -> u16 {
    HOST_PAIRING_PROTOCOL_VERSION
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientPairingMode {
    #[default]
    Fresh,
    RecoverIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientPairingRequest {
    #[serde(default = "default_host_pairing_protocol_version")]
    pub protocol_version: u16,
    #[serde(default)]
    pub mode: ClientPairingMode,
    pub host: HostIdentity,
    pub token_hash: String,
    pub polling_secret_hash: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialStatus {
    Authorized,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialStatusResponse {
    pub status: CredentialStatus,
    #[serde(deserialize_with = "deserialize_canonical_uuid")]
    pub host_id: String,
    #[serde(deserialize_with = "deserialize_canonical_uuid")]
    pub instance_id: String,
    pub protocol_version: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientPairingResponse {
    #[serde(deserialize_with = "deserialize_canonical_uuid")]
    pub request_id: String,
    pub activation_url: String,
    pub expires_in: u64,
    pub poll_interval: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientPairingStatusResponse {
    pub status: PairingStatus,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_canonical_uuid"
    )]
    pub instance_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PairingStatus {
    Waiting,
    Active,
    Denied,
    Expired,
}

impl PairingStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Waiting => "waiting",
            Self::Active => "active",
            Self::Denied => "denied",
            Self::Expired => "expired",
        }
    }
}

impl TryFrom<&str> for PairingStatus {
    type Error = ();

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "waiting" => Ok(Self::Waiting),
            "active" => Ok(Self::Active),
            "denied" => Ok(Self::Denied),
            "expired" => Ok(Self::Expired),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActivateClientRequest {
    #[serde(deserialize_with = "deserialize_canonical_uuid")]
    pub request_id: String,
    pub activation_code: String,
}

/// Borrowed serialization view used by Clients so the instance authorization code
/// is not copied into an additional heap allocation before transmission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ActivateClientRequestRef<'a> {
    pub request_id: &'a str,
    pub activation_code: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActivateClientResponse {
    #[serde(deserialize_with = "deserialize_canonical_uuid")]
    pub instance_id: String,
    pub status: ActivatePairingStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivatePairingStatus {
    Active,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientReportAck {
    #[serde(deserialize_with = "deserialize_canonical_uuid")]
    pub host_id: String,
    #[serde(deserialize_with = "deserialize_canonical_uuid")]
    pub report_id: String,
    pub accepted: bool,
    pub received_at: DateTime<Utc>,
}

fn deserialize_optional_canonical_uuid<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer)?
        .map(|value| {
            let deserializer = serde::de::value::StringDeserializer::<D::Error>::new(value);
            deserialize_canonical_uuid(deserializer)
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::de::DeserializeOwned;

    fn host() -> HostIdentity {
        HostIdentity {
            id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb".into(),
            os: "linux".into(),
            os_version: None,
            kernel_version: None,
            arch: "x86_64".into(),
            client_version: "0.3.6".into(),
        }
    }

    #[test]
    fn pairing_request_round_trips_without_loss() {
        let request = ClientPairingRequest {
            protocol_version: HOST_PAIRING_PROTOCOL_VERSION,
            mode: ClientPairingMode::Fresh,
            host: host(),
            token_hash: "a".repeat(64),
            polling_secret_hash: "b".repeat(64),
        };
        let encoded = serde_json::to_vec(&request).unwrap();
        assert_eq!(
            serde_json::from_slice::<ClientPairingRequest>(&encoded).unwrap(),
            request
        );
    }

    #[test]
    fn legacy_pairing_request_defaults_to_protocol_one() {
        let request: ClientPairingRequest = serde_json::from_value(serde_json::json!({
            "host": host(),
            "token_hash": "a".repeat(64),
            "polling_secret_hash": "b".repeat(64)
        }))
        .unwrap();
        assert_eq!(request.protocol_version, HOST_PAIRING_PROTOCOL_VERSION);
        assert_eq!(request.mode, ClientPairingMode::Fresh);
    }

    #[test]
    fn compatibility_manifest_matches_wire_constants() {
        let manifest: serde_json::Value =
            serde_json::from_str(include_str!("../../compatibility.json")).unwrap();
        assert_eq!(
            manifest["host_pairing_protocol"],
            HOST_PAIRING_PROTOCOL_VERSION
        );
        assert_eq!(
            manifest["host_report_schema"],
            crate::CLIENT_REPORT_SCHEMA_VERSION
        );
    }

    #[test]
    fn pairing_responses_reject_unknown_statuses_and_noncanonical_uuids() {
        for value in [
            serde_json::json!({
                "status": "future",
                "instance_id": null
            }),
            serde_json::json!({
                "status": "active",
                "instance_id": "BBBBBBBB-BBBB-4BBB-8BBB-BBBBBBBBBBBB"
            }),
        ] {
            assert!(serde_json::from_value::<ClientPairingStatusResponse>(value).is_err());
        }
    }

    #[test]
    fn report_acknowledgement_rejects_unknown_fields() {
        let value = serde_json::json!({
            "host_id": "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
            "report_id": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
            "accepted": true,
            "received_at": "2026-01-01T00:00:00Z",
            "unknown_status_detail": "ok"
        });
        assert!(serde_json::from_value::<ClientReportAck>(value).is_err());
    }

    fn assert_rejects_server_control_fields<T: DeserializeOwned>(value: serde_json::Value) {
        for field in ["command", "configuration", "script"] {
            let mut candidate = value.clone();
            candidate
                .as_object_mut()
                .expect("response fixture must be an object")
                .insert(field.into(), serde_json::json!("forbidden"));
            assert!(
                serde_json::from_value::<T>(candidate).is_err(),
                "server-to-Client response unexpectedly accepted {field}"
            );
        }
    }

    #[test]
    fn server_to_client_contract_has_no_control_payload() {
        assert_rejects_server_control_fields::<ClientPairingResponse>(serde_json::json!({
            "request_id": "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
            "activation_url": "/activate/bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
            "expires_in": 900,
            "poll_interval": 2
        }));
        assert_rejects_server_control_fields::<ClientPairingStatusResponse>(serde_json::json!({
            "status": "waiting"
        }));
        assert_rejects_server_control_fields::<ActivateClientResponse>(serde_json::json!({
            "instance_id": "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
            "status": "active"
        }));
        assert_rejects_server_control_fields::<ClientReportAck>(serde_json::json!({
            "host_id": "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
            "report_id": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
            "accepted": true,
            "received_at": "2026-01-01T00:00:00Z"
        }));
    }

    #[test]
    fn borrowed_activation_request_matches_owned_wire_shape() {
        let request_id = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
        let activation_code = "uci_example";
        let owned = ActivateClientRequest {
            request_id: request_id.into(),
            activation_code: activation_code.into(),
        };
        let borrowed = ActivateClientRequestRef {
            request_id,
            activation_code,
        };

        assert_eq!(
            serde_json::to_value(owned).unwrap(),
            serde_json::to_value(borrowed).unwrap()
        );
    }
}
