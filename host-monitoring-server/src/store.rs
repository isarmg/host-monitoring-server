use chrono::{DateTime, Utc};
use host_protocol::{Capability, ClientPairingRequest, ClientReport, PairingStatus};
use sarmg_admin_core::AdministratorStore;
use sqlx::{Acquire, FromRow, Row, Sqlite, SqlitePool, Transaction, types::Json};
use uuid::Uuid;

pub use crate::database_schema::{initialize_empty, open_existing, open_or_initialize};
use crate::model::{
    ClientInstanceSummary, ClientPairingPublicSummary, HistoryPoint, HostSummary, MetricSummary,
    host_status,
};

pub async fn ready(pool: &SqlitePool) -> bool {
    crate::database_schema::is_current(pool).await
}

pub async fn retention_ready(pool: &SqlitePool) -> bool {
    sqlx::query_scalar::<_, i64>(
        "SELECT \
           EXISTS(SELECT 1 FROM pragma_table_info('client_metric_reports') \
                   WHERE name='aggregated_at') \
           AND EXISTS(SELECT 1 FROM sqlite_master \
                      WHERE type='table' AND name='client_metric_hourly_aggregates') \
           AND (SELECT COUNT(*) FROM pragma_table_info('client_metric_hourly_aggregates'))=42",
    )
    .fetch_one(pool)
    .await
    .is_ok_and(|ready| ready == 1)
}

pub fn normalize_username(username: &str) -> anyhow::Result<String> {
    sarmg_admin_auth::normalize_administrator_username(username).map_err(anyhow::Error::from)
}

pub async fn ensure_admin_user(
    pool: &SqlitePool,
    username: &str,
    password: Option<&str>,
) -> anyhow::Result<()> {
    let store = sarmg_admin_sqlite::SqliteAdministratorStore::new(pool.clone());
    store.validate_all_administrators().await?;
    let service = sarmg_admin_core::AdministratorService::new(store);
    if service.store().administrator_count().await? == 0 {
        let password = password.ok_or_else(|| {
            anyhow::anyhow!(
                "HOST_MONITORING_BOOTSTRAP_ADMIN_PASSWORD is required while no administrators exist"
            )
        })?;
        service
            .bootstrap_administrator(username, password, now_micros()?)
            .await?;
    }
    Ok(())
}

pub async fn reset_admin_password(
    pool: &SqlitePool,
    username: &str,
    password: &str,
) -> anyhow::Result<()> {
    let store = sarmg_admin_sqlite::SqliteAdministratorStore::new(pool.clone());
    store.validate_all_administrators().await?;
    sarmg_admin_core::AdministratorService::new(store)
        .change_administrator_password(username, password, now_micros()?)
        .await
        .map_err(anyhow::Error::from)
}

fn now_micros() -> anyhow::Result<u64> {
    Ok(u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_micros(),
    )?)
}

async fn audit(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    action: &str,
    target: &str,
    detail: Option<&str>,
    actor: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO audit_events(action,target,detail,actor,created_at) VALUES(?,?,?,?,?)",
    )
    .bind(action)
    .bind(target)
    .bind(detail)
    .bind(actor)
    .bind(Utc::now())
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub enum CreateInviteResult {
    Created(ClientInstanceSummary),
    Conflict,
}

pub async fn create_invite(
    pool: &SqlitePool,
    secrets: &crate::crypto::SecretBox,
    display_name: &str,
    actor: &str,
) -> anyhow::Result<(CreateInviteResult, Option<String>)> {
    let invite_id = Uuid::new_v4();
    let instance_id = Uuid::new_v4();
    let activation_code = format!("uci_{}", Uuid::new_v4().simple());
    let activation_hash = crate::token_hash(&activation_code);
    let authorization_code_enc = secrets.encrypt(instance_id, &activation_code)?;
    let created_at = Utc::now();
    let mut tx = pool.begin().await?;
    let row = sqlx::query(
        r#"INSERT INTO client_instance_invites(
               invite_id,instance_id,activation_code_hash,authorization_code_enc,display_name,created_at
           ) VALUES(?,?,?,?,?,?)
           ON CONFLICT (instance_id) WHERE status='pending' DO NOTHING
           RETURNING invite_id,instance_id,display_name,status,created_at,authorization_code_enc"#,
    )
    .bind(invite_id)
    .bind(instance_id)
    .bind(&activation_hash)
    .bind(authorization_code_enc)
    .bind(display_name)
    .bind(created_at)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(row) = row else {
        tx.rollback().await?;
        return Ok((CreateInviteResult::Conflict, None));
    };
    audit(
        &mut tx,
        "monitoring.client_instance.invite.create",
        &instance_id.to_string(),
        Some(&format!("invite_id={invite_id}")),
        actor,
    )
    .await?;
    tx.commit().await?;
    Ok((
        CreateInviteResult::Created(client_instance(&row, secrets)?),
        Some(activation_code),
    ))
}

pub async fn list_invites(
    pool: &SqlitePool,
    secrets: &crate::crypto::SecretBox,
) -> anyhow::Result<Vec<ClientInstanceSummary>> {
    let rows = sqlx::query(
        r#"SELECT invite_id,instance_id,display_name,created_at,status,authorization_code_enc
           FROM client_instance_invites
           ORDER BY created_at DESC LIMIT 200"#,
    )
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|row| client_instance(row, secrets))
        .collect()
}

pub async fn validate_invite_authorizations(
    pool: &SqlitePool,
    secrets: &crate::crypto::SecretBox,
) -> anyhow::Result<()> {
    for row in sqlx::query(
        "SELECT instance_id,activation_code_hash,authorization_code_enc \
         FROM client_instance_invites ORDER BY instance_id",
    )
    .fetch_all(pool)
    .await?
    {
        let instance_id = row.try_get::<Uuid, _>("instance_id")?;
        let code = secrets.decrypt(
            instance_id,
            &row.try_get::<Vec<u8>, _>("authorization_code_enc")?,
        )?;
        crate::model::validate_activation_code(&code)
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        anyhow::ensure!(
            row.try_get::<String, _>("activation_code_hash")? == crate::token_hash(&code),
            "stored client authorization digest does not match its encrypted value"
        );
    }
    Ok(())
}

pub async fn rotate_invite_authorization(
    pool: &SqlitePool,
    secrets: &crate::crypto::SecretBox,
    invite_id: Uuid,
    authorization_code: &str,
    actor: &str,
) -> anyhow::Result<Option<ClientInstanceSummary>> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let row = sqlx::query(
        "SELECT instance_id FROM client_instance_invites WHERE invite_id = ? AND status != 'cancelled'",
    )
    .bind(invite_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(row) = row else { return Ok(None) };
    let instance_id: Uuid = row.try_get("instance_id")?;
    let encoded = secrets.encrypt(instance_id, authorization_code)?;
    let now = Utc::now();
    sqlx::query(
        "UPDATE client_instance_invites SET activation_code_hash = ?, authorization_code_enc = ?, \
         status = 'pending', activated_at = NULL WHERE invite_id = ?",
    )
    .bind(crate::token_hash(authorization_code))
    .bind(encoded)
    .bind(invite_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE client_credentials SET revoked_at = ? WHERE host_id = ? AND revoked_at IS NULL",
    )
    .bind(now)
    .bind(instance_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE monitored_hosts SET lifecycle_status = 'revoked', revoked_at = ? WHERE host_id = ?",
    )
    .bind(now)
    .bind(instance_id)
    .execute(&mut *tx)
    .await?;
    audit(
        &mut tx,
        "monitoring.client_instance.authorization.rotate",
        &instance_id.to_string(),
        None,
        actor,
    )
    .await?;
    tx.commit().await?;
    let row = sqlx::query(
        "SELECT invite_id,instance_id,display_name,created_at,status,authorization_code_enc \
         FROM client_instance_invites WHERE invite_id = ?",
    )
    .bind(invite_id)
    .fetch_one(pool)
    .await?;
    Ok(Some(client_instance(&row, secrets)?))
}

pub enum CancelInviteResult {
    Cancelled,
    Deleted,
    NotFound,
    NotPending,
}

pub async fn cancel_invite(
    pool: &SqlitePool,
    invite_id: Uuid,
    actor: &str,
) -> anyhow::Result<CancelInviteResult> {
    let mut tx = pool.begin().await?;
    let row =
        sqlx::query("SELECT status,instance_id FROM client_instance_invites WHERE invite_id=?")
            .bind(invite_id)
            .fetch_optional(&mut *tx)
            .await?;
    let Some(row) = row else {
        tx.rollback().await?;
        return Ok(CancelInviteResult::NotFound);
    };
    let status = row.try_get::<String, _>("status")?;
    let instance_id: Uuid = row.try_get("instance_id")?;
    if status == "cancelled" {
        sqlx::query("DELETE FROM client_pairing_requests WHERE invite_id=? OR (requested_host_id=? AND status IN ('pending','denied'))")
            .bind(invite_id).bind(instance_id).execute(&mut *tx).await?;
        audit(
            &mut tx,
            "monitoring.client_instance.invite.delete",
            &instance_id.to_string(),
            Some(&format!("invite_id={invite_id}")),
            actor,
        )
        .await?;
        sqlx::query("DELETE FROM client_instance_invites WHERE invite_id=?")
            .bind(invite_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        return Ok(CancelInviteResult::Deleted);
    }
    if status != "pending" {
        tx.rollback().await?;
        return Ok(CancelInviteResult::NotPending);
    }
    sqlx::query(
        "UPDATE client_instance_invites SET status='cancelled',cancelled_at=? WHERE invite_id=?",
    )
    .bind(Utc::now())
    .bind(invite_id)
    .execute(&mut *tx)
    .await?;
    audit(
        &mut tx,
        "monitoring.client_instance.invite.cancel",
        &instance_id.to_string(),
        Some(&format!("invite_id={invite_id}")),
        actor,
    )
    .await?;
    tx.commit().await?;
    Ok(CancelInviteResult::Cancelled)
}

fn client_instance(
    row: &sqlx::sqlite::SqliteRow,
    secrets: &crate::crypto::SecretBox,
) -> anyhow::Result<ClientInstanceSummary> {
    let instance_id = row.try_get::<Uuid, _>("instance_id")?;
    Ok(ClientInstanceSummary {
        request_id: row.try_get::<Uuid, _>("invite_id")?.to_string(),
        instance_id: instance_id.to_string(),
        display_name: row.try_get("display_name")?,
        status: row.try_get("status")?,
        created_at: row.try_get("created_at")?,
        authorization_code: secrets.decrypt(
            instance_id,
            &row.try_get::<Vec<u8>, _>("authorization_code_enc")?,
        )?,
    })
}

pub enum CreatePairingResult {
    Ready {
        request_id: Uuid,
        expires_at: DateTime<Utc>,
        created: bool,
    },
    Expired,
    Conflict,
    AtCapacity,
    DeviceAtCapacity,
}

pub async fn create_pairing(
    pool: &SqlitePool,
    request: &ClientPairingRequest,
) -> anyhow::Result<CreatePairingResult> {
    const MAX_PENDING: i64 = 4096;
    let mut tx = pool.begin().await?;
    // Serialize identical polling secrets without locking the whole table.
    // SQLite serializes writes with its database lock; no advisory lock is needed.
    let existing = sqlx::query(
        "SELECT request_id,requested_host_id,os,os_version,kernel_version,arch,client_version,token_hash,status,expires_at \
         FROM client_pairing_requests WHERE polling_secret_hash=?",
    ).bind(&request.polling_secret_hash).fetch_optional(&mut *tx).await?;
    if let Some(row) = existing {
        let matches = row.try_get::<Uuid, _>("requested_host_id")?.to_string() == request.host.id
            && row.try_get::<String, _>("os")? == request.host.os.trim()
            && row.try_get::<Option<String>, _>("os_version")? == request.host.os_version
            && row.try_get::<Option<String>, _>("kernel_version")? == request.host.kernel_version
            && row.try_get::<String, _>("arch")? == request.host.arch.trim()
            && row.try_get::<String, _>("client_version")? == request.host.client_version.trim()
            && row.try_get::<String, _>("token_hash")? == request.token_hash;
        let expires_at: DateTime<Utc> = row.try_get("expires_at")?;
        let status: String = row.try_get("status")?;
        let request_id: Uuid = row.try_get("request_id")?;
        tx.rollback().await?;
        if !matches || status == "denied" {
            return Ok(CreatePairingResult::Conflict);
        }
        if expires_at <= Utc::now() {
            return Ok(CreatePairingResult::Expired);
        }
        return Ok(CreatePairingResult::Ready {
            request_id,
            expires_at,
            created: false,
        });
    }
    let now = Utc::now();
    let denied_cutoff = now - chrono::Duration::days(30);
    sqlx::query(
        "DELETE FROM client_pairing_requests WHERE request_id IN (\
           SELECT request_id FROM client_pairing_requests \
           WHERE (status='pending' AND expires_at <= ?) OR (status='denied' AND created_at < ?) \
           ORDER BY created_at LIMIT 512)",
    )
    .bind(now)
    .bind(denied_cutoff)
    .execute(&mut *tx)
    .await?;
    let pending: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM client_pairing_requests WHERE status='pending' AND expires_at>?",
    )
    .bind(now)
    .fetch_one(&mut *tx)
    .await?;
    if pending >= MAX_PENDING {
        tx.commit().await?;
        return Ok(CreatePairingResult::AtCapacity);
    }
    let requested_host_id = Uuid::parse_str(&request.host.id)?;
    let pending_for_device: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM client_pairing_requests \
         WHERE requested_host_id=? AND status='pending' AND expires_at>?",
    )
    .bind(requested_host_id)
    .bind(now)
    .fetch_one(&mut *tx)
    .await?;
    if pending_for_device >= 4 {
        tx.commit().await?;
        return Ok(CreatePairingResult::DeviceAtCapacity);
    }
    let request_id = Uuid::new_v4();
    let created_at = Utc::now();
    let expires_at = created_at + chrono::Duration::minutes(15);
    let result = sqlx::query(
        r#"INSERT INTO client_pairing_requests(
               request_id,requested_host_id,os,os_version,kernel_version,arch,client_version,
               token_hash,polling_secret_hash,expires_at,created_at)
           VALUES(?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT DO NOTHING"#,
    )
    .bind(request_id)
    .bind(requested_host_id)
    .bind(request.host.os.trim())
    .bind(&request.host.os_version)
    .bind(&request.host.kernel_version)
    .bind(request.host.arch.trim())
    .bind(request.host.client_version.trim())
    .bind(&request.token_hash)
    .bind(&request.polling_secret_hash)
    .bind(expires_at)
    .bind(created_at)
    .execute(&mut *tx)
    .await?;
    if result.rows_affected() != 1 {
        tx.rollback().await?;
        return Ok(CreatePairingResult::Conflict);
    }
    tx.commit().await?;
    Ok(CreatePairingResult::Ready {
        request_id,
        expires_at,
        created: true,
    })
}

pub async fn pairing_public(
    pool: &SqlitePool,
    request_id: Uuid,
) -> anyhow::Result<Option<ClientPairingPublicSummary>> {
    let row = sqlx::query(
        "SELECT request_id,os,arch,client_version,expires_at,CASE WHEN status='pending' AND expires_at<=? THEN 'expired' ELSE CASE WHEN status='pending' THEN 'waiting' ELSE status END END AS status \
         FROM client_pairing_requests WHERE request_id=?",
    ).bind(Utc::now()).bind(request_id).fetch_optional(pool).await?;
    row.map(|row| {
        Ok(ClientPairingPublicSummary {
            request_id: row.try_get::<Uuid, _>("request_id")?.to_string(),
            os: row.try_get("os")?,
            arch: row.try_get("arch")?,
            client_version: row.try_get("client_version")?,
            status: row.try_get("status")?,
            expires_at: row.try_get("expires_at")?,
        })
    })
    .transpose()
}

pub async fn pairing_status(
    pool: &SqlitePool,
    request_id: Uuid,
    secret_hash: &str,
) -> anyhow::Result<Option<(PairingStatus, Option<String>)>> {
    let row = sqlx::query(
        "SELECT instance_id,CASE WHEN status='pending' AND expires_at<=? THEN 'expired' WHEN status='pending' THEN 'waiting' ELSE status END AS status \
         FROM client_pairing_requests WHERE request_id=? AND polling_secret_hash=?",
    ).bind(Utc::now()).bind(request_id).bind(secret_hash).fetch_optional(pool).await?;
    row.map(|row| {
        let raw: String = row.try_get("status")?;
        let status = PairingStatus::try_from(raw.as_str())
            .map_err(|_| anyhow::anyhow!("invalid pairing status in database"))?;
        let instance = if status == PairingStatus::Active {
            row.try_get::<Option<Uuid>, _>("instance_id")?
                .map(|v| v.to_string())
        } else {
            None
        };
        Ok((status, instance))
    })
    .transpose()
}

pub enum ActivateResult {
    Active(Uuid),
    NotFound,
    InvalidCode,
    Expired,
    Conflict,
}

pub async fn activate(
    pool: &SqlitePool,
    request_id: Uuid,
    activation_hash: &str,
    actor: &str,
) -> anyhow::Result<ActivateResult> {
    let mut tx = pool.begin().await?;
    let pairing = sqlx::query(
        "SELECT request_id,os,os_version,kernel_version,arch,client_version,token_hash,status,invite_id,instance_id,expires_at \
         FROM client_pairing_requests WHERE request_id=?",
    ).bind(request_id).fetch_optional(&mut *tx).await?;
    let Some(pairing) = pairing else {
        tx.rollback().await?;
        return Ok(ActivateResult::NotFound);
    };
    let invite = sqlx::query(
        "SELECT invite_id,instance_id,display_name,status FROM client_instance_invites \
         WHERE activation_code_hash=?",
    )
    .bind(activation_hash)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(invite) = invite else {
        tx.rollback().await?;
        return Ok(ActivateResult::InvalidCode);
    };
    let invite_id: Uuid = invite.try_get("invite_id")?;
    let instance_id: Uuid = invite.try_get("instance_id")?;
    let pairing_status: String = pairing.try_get("status")?;
    if pairing_status == "active" {
        let same = pairing.try_get::<Option<Uuid>, _>("invite_id")? == Some(invite_id)
            && pairing.try_get::<Option<Uuid>, _>("instance_id")? == Some(instance_id);
        let token_hash: String = pairing.try_get("token_hash")?;
        let active: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM client_credentials WHERE host_id=? AND token_hash=? AND revoked_at IS NULL)",
        ).bind(instance_id).bind(token_hash).fetch_one(&mut *tx).await?;
        tx.rollback().await?;
        return Ok(if same && active {
            ActivateResult::Active(instance_id)
        } else {
            ActivateResult::Conflict
        });
    }
    if pairing_status != "pending" || invite.try_get::<String, _>("status")? != "pending" {
        tx.rollback().await?;
        return Ok(ActivateResult::Conflict);
    }
    let now = Utc::now();
    if pairing.try_get::<DateTime<Utc>, _>("expires_at")? <= now {
        tx.rollback().await?;
        return Ok(ActivateResult::Expired);
    }
    let token_hash: String = pairing.try_get("token_hash")?;
    sqlx::query(
        "INSERT INTO monitored_hosts(host_id,name,os,os_version,kernel_version,arch,client_version,registered_at,last_seen_at) \
         VALUES(?,?,?,?,?,?,?,?,?) ON CONFLICT(host_id) DO UPDATE SET name=excluded.name,os=excluded.os, \
         os_version=excluded.os_version,kernel_version=excluded.kernel_version,arch=excluded.arch, \
         client_version=excluded.client_version,last_seen_at=excluded.last_seen_at,lifecycle_status='active',revoked_at=NULL",
    ).bind(instance_id).bind(invite.try_get::<String,_>("display_name")?)
      .bind(pairing.try_get::<String,_>("os")?).bind(pairing.try_get::<Option<String>,_>("os_version")?)
      .bind(pairing.try_get::<Option<String>,_>("kernel_version")?).bind(pairing.try_get::<String,_>("arch")?)
      .bind(pairing.try_get::<String,_>("client_version")?).bind(now).bind(now).execute(&mut *tx).await?;
    sqlx::query(
        "INSERT INTO client_credentials(credential_id,host_id,token_hash,issued_at) VALUES(?,?,?,?)",
    )
    .bind(Uuid::new_v4())
    .bind(instance_id)
    .bind(&token_hash)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE client_pairing_requests SET status='active',invite_id=?,instance_id=?,activated_at=? WHERE request_id=?")
        .bind(invite_id).bind(instance_id).bind(now).bind(request_id).execute(&mut *tx).await?;
    sqlx::query(
        "UPDATE client_instance_invites SET status='active',activated_at=? WHERE invite_id=?",
    )
    .bind(now)
    .bind(invite_id)
    .execute(&mut *tx)
    .await?;
    audit(
        &mut tx,
        "monitoring.client_instance.activate",
        &instance_id.to_string(),
        Some(&format!("request_id={request_id}; invite_id={invite_id}")),
        actor,
    )
    .await?;
    tx.commit().await?;
    Ok(ActivateResult::Active(instance_id))
}

pub async fn host_for_token(pool: &SqlitePool, token_hash: &str) -> anyhow::Result<Option<Uuid>> {
    Ok(sqlx::query_scalar(
        "SELECT c.host_id FROM client_credentials c JOIN monitored_hosts h ON h.host_id=c.host_id WHERE c.token_hash=? AND c.revoked_at IS NULL AND h.lifecycle_status='active'",
    )
    .bind(token_hash)
    .fetch_optional(pool)
    .await?)
}

#[derive(Debug, thiserror::Error)]
pub enum ReportStoreError {
    #[error("monitoring host or credential no longer exists")]
    Unauthorized,
    #[error("report_id already belongs to another host")]
    ReportIdConflict,
}

/// A report that has already passed the HTTP trust-boundary checks. The token
/// hash is intentionally private and this type does not implement `Debug`, so
/// telemetry credentials cannot be included accidentally in writer logs.
pub struct ReportWrite {
    report: ClientReport,
    token_hash: String,
    metrics: MetricSummary,
}

impl ReportWrite {
    pub fn new(report: ClientReport, token_hash: String, metrics: MetricSummary) -> Self {
        Self {
            report,
            token_hash,
            metrics,
        }
    }
}

pub async fn store_report(
    pool: &SqlitePool,
    report: &ClientReport,
    token_hash: &str,
    metrics: &MetricSummary,
) -> anyhow::Result<(bool, DateTime<Utc>)> {
    let mut tx = pool.begin().await?;
    let result = store_report_in_transaction(&mut tx, report, token_hash, metrics).await;
    match result {
        Ok(result) => {
            tx.commit().await?;
            Ok(result)
        }
        Err(error) => {
            tx.rollback().await?;
            Err(error)
        }
    }
}

/// Persist a bounded writer batch in one SQLite transaction. Every report is
/// wrapped in a savepoint so an authentication race, report-id conflict, or a
/// single malformed/corrupt row rolls back only that report. Results are
/// returned only after the outer transaction commits; an uncertain batch
/// commit therefore never produces a false acknowledgement.
pub async fn store_report_batch(
    pool: &SqlitePool,
    reports: &[ReportWrite],
) -> anyhow::Result<Vec<anyhow::Result<(bool, DateTime<Utc>)>>> {
    // Acquire SQLite's single-writer slot before taking any read snapshot.
    // A deferred transaction can otherwise fail with SQLITE_BUSY_SNAPSHOT
    // when another product mutation commits between this batch's read/write.
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let mut results = Vec::with_capacity(reports.len());
    for write in reports {
        let mut savepoint = tx.begin().await?;
        match store_report_in_transaction(
            &mut savepoint,
            &write.report,
            &write.token_hash,
            &write.metrics,
        )
        .await
        {
            Ok(result) => {
                savepoint.commit().await?;
                results.push(Ok(result));
            }
            Err(error) => {
                savepoint.rollback().await?;
                results.push(Err(error));
            }
        }
    }
    tx.commit().await?;
    Ok(results)
}

async fn store_report_in_transaction(
    tx: &mut Transaction<'_, Sqlite>,
    report: &ClientReport,
    token_hash: &str,
    metrics: &MetricSummary,
) -> anyhow::Result<(bool, DateTime<Utc>)> {
    let host_id = Uuid::parse_str(&report.host.id)?;
    let report_id = Uuid::parse_str(&report.report_id)?;
    let current = sqlx::query(
        "SELECT latest_report_id,latest_collected_at FROM monitored_hosts h \
         WHERE host_id=? AND h.lifecycle_status='active' AND EXISTS(SELECT 1 FROM client_credentials c WHERE c.host_id=h.host_id AND c.token_hash=? AND c.revoked_at IS NULL)",
    ).bind(host_id).bind(token_hash).fetch_optional(&mut **tx).await?;
    let Some(current) = current else {
        return Err(ReportStoreError::Unauthorized.into());
    };
    let previous_report: Option<Uuid> = current.try_get("latest_report_id")?;
    let previous_collected: Option<DateTime<Utc>> = current.try_get("latest_collected_at")?;
    let becomes_latest = previous_collected.is_none_or(|previous| {
        report.collected_at > previous
            || (report.collected_at == previous
                && previous_report.is_none_or(|previous| report_id > previous))
    });
    let payload = becomes_latest.then(|| Json(report.clone()));
    let received_at = Utc::now();
    let inserted = sqlx::query(
        r#"INSERT INTO client_metric_reports(
             report_id,host_id,schema_version,collected_at,received_at,interval_seconds,payload,
             cpu_usage_percent,memory_usage_percent,network_received_bytes_per_second,
             network_transmitted_bytes_per_second,disk_read_bytes_per_second,disk_written_bytes_per_second,
             max_temperature_celsius,gpu_utilization_percent,gpu_memory_usage_percent)
           VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)
           ON CONFLICT(report_id) DO NOTHING RETURNING received_at"#,
    ).bind(report_id).bind(host_id).bind(i32::from(report.schema_version)).bind(report.collected_at)
      .bind(received_at).bind(report.interval_seconds).bind(payload)
      .bind(metrics.cpu_usage_percent).bind(metrics.memory_usage_percent)
      .bind(metrics.network_received_bytes_per_second).bind(metrics.network_transmitted_bytes_per_second)
      .bind(metrics.disk_read_bytes_per_second).bind(metrics.disk_written_bytes_per_second)
      .bind(metrics.max_temperature_celsius).bind(metrics.gpu_utilization_percent).bind(metrics.gpu_memory_usage_percent)
      .fetch_optional(&mut **tx).await?;
    let Some(row) = inserted else {
        let existing: Option<(Uuid, DateTime<Utc>)> = sqlx::query_as(
            "SELECT host_id,received_at FROM client_metric_reports WHERE report_id=?",
        )
        .bind(report_id)
        .fetch_optional(&mut **tx)
        .await?;
        return match existing {
            Some((owner, timestamp)) if owner == host_id => Ok((false, timestamp)),
            _ => Err(ReportStoreError::ReportIdConflict.into()),
        };
    };
    let stored_received: DateTime<Utc> = row.try_get("received_at")?;
    if becomes_latest {
        sqlx::query(
            r#"UPDATE monitored_hosts SET
                 os=?,os_version=?,kernel_version=?,arch=?,client_version=?,capabilities=?,
                 last_seen_at=CASE WHEN last_seen_at > ? THEN last_seen_at ELSE ? END,latest_report_id=?,
                 latest_collected_at=?,latest_interval_seconds=? WHERE host_id=?"#,
        )
        .bind(report.host.os.trim())
        .bind(&report.host.os_version)
        .bind(&report.host.kernel_version)
        .bind(report.host.arch.trim())
        .bind(report.host.client_version.trim())
        .bind(Json(&report.capabilities))
        .bind(stored_received)
        .bind(stored_received)
        .bind(report_id)
        .bind(report.collected_at)
        .bind(report.interval_seconds)
        .bind(host_id)
        .execute(&mut **tx)
        .await?;
        if let Some(previous) = previous_report.filter(|previous| *previous != report_id) {
            sqlx::query("UPDATE client_metric_reports SET payload=NULL WHERE report_id=?")
                .bind(previous)
                .execute(&mut **tx)
                .await?;
        }
    }
    sqlx::query("UPDATE client_credentials SET last_used_at=? WHERE token_hash=?")
        .bind(stored_received)
        .bind(token_hash)
        .execute(&mut **tx)
        .await?;
    Ok((true, stored_received))
}

#[derive(FromRow)]
struct HostRow {
    host_id: Uuid,
    name: String,
    os: String,
    os_version: Option<String>,
    kernel_version: Option<String>,
    arch: String,
    client_version: String,
    capabilities: Json<Vec<Capability>>,
    registered_at: DateTime<Utc>,
    last_seen_at: DateTime<Utc>,
    latest_collected_at: Option<DateTime<Utc>>,
    latest_interval_seconds: Option<f64>,
    cpu_usage_percent: Option<f64>,
    memory_usage_percent: Option<f64>,
    network_received_bytes_per_second: Option<f64>,
    network_transmitted_bytes_per_second: Option<f64>,
    disk_read_bytes_per_second: Option<f64>,
    disk_written_bytes_per_second: Option<f64>,
    max_temperature_celsius: Option<f64>,
    gpu_utilization_percent: Option<f64>,
    gpu_memory_usage_percent: Option<f64>,
}

const HOST_SELECT: &str = r#"SELECT h.host_id,h.name,h.os,h.os_version,h.kernel_version,h.arch,h.client_version,
 h.capabilities,h.registered_at,h.last_seen_at,h.latest_collected_at,h.latest_interval_seconds,
 r.cpu_usage_percent,r.memory_usage_percent,r.network_received_bytes_per_second,
 r.network_transmitted_bytes_per_second,r.disk_read_bytes_per_second,r.disk_written_bytes_per_second,
 r.max_temperature_celsius,r.gpu_utilization_percent,r.gpu_memory_usage_percent
 FROM monitored_hosts h LEFT JOIN client_metric_reports r ON r.report_id=h.latest_report_id"#;

fn summarize(row: HostRow) -> HostSummary {
    HostSummary {
        id: row.host_id.to_string(),
        name: row.name,
        os: row.os,
        os_version: row.os_version,
        kernel_version: row.kernel_version,
        arch: row.arch,
        client_version: row.client_version,
        registered_at: row.registered_at,
        last_seen_at: row.last_seen_at,
        latest_collected_at: row.latest_collected_at,
        status: host_status(row.last_seen_at, row.latest_interval_seconds),
        capabilities: row.capabilities.0,
        metrics: MetricSummary {
            cpu_usage_percent: row.cpu_usage_percent,
            memory_usage_percent: row.memory_usage_percent,
            network_received_bytes_per_second: row.network_received_bytes_per_second,
            network_transmitted_bytes_per_second: row.network_transmitted_bytes_per_second,
            disk_read_bytes_per_second: row.disk_read_bytes_per_second,
            disk_written_bytes_per_second: row.disk_written_bytes_per_second,
            max_temperature_celsius: row.max_temperature_celsius,
            gpu_utilization_percent: row.gpu_utilization_percent,
            gpu_memory_usage_percent: row.gpu_memory_usage_percent,
        },
    }
}

pub async fn list_hosts(
    pool: &SqlitePool,
    limit: i64,
    offset: i64,
) -> anyhow::Result<(Vec<HostSummary>, i64)> {
    let total: i64 =
        sqlx::query_scalar("SELECT count(*) FROM monitored_hosts WHERE lifecycle_status='active'")
            .fetch_one(pool)
            .await?;
    let sql = format!(
        "{HOST_SELECT} WHERE h.lifecycle_status='active' ORDER BY h.registered_at,h.host_id LIMIT ? OFFSET ?"
    );
    let rows: Vec<HostRow> = sqlx::query_as(&sql)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?;
    Ok((rows.into_iter().map(summarize).collect(), total))
}

pub async fn get_host(
    pool: &SqlitePool,
    host_id: Uuid,
) -> anyhow::Result<Option<(HostSummary, Option<ClientReport>)>> {
    let sql = format!("{HOST_SELECT} WHERE h.host_id=? AND h.lifecycle_status='active'");
    let row: Option<HostRow> = sqlx::query_as(&sql)
        .bind(host_id)
        .fetch_optional(pool)
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let payload: Option<Json<ClientReport>> = sqlx::query_scalar(
        "SELECT r.payload FROM monitored_hosts h LEFT JOIN client_metric_reports r ON r.report_id=h.latest_report_id WHERE h.host_id=?",
    ).bind(host_id).fetch_one(pool).await?;
    Ok(Some((summarize(row), payload.map(|json| json.0))))
}

#[derive(FromRow)]
struct HistoryRow {
    report_id: Uuid,
    collected_at: DateTime<Utc>,
    received_at: DateTime<Utc>,
    cpu_usage_percent: Option<f64>,
    memory_usage_percent: Option<f64>,
    network_received_bytes_per_second: Option<f64>,
    network_transmitted_bytes_per_second: Option<f64>,
    disk_read_bytes_per_second: Option<f64>,
    disk_written_bytes_per_second: Option<f64>,
    max_temperature_celsius: Option<f64>,
    gpu_utilization_percent: Option<f64>,
    gpu_memory_usage_percent: Option<f64>,
}

pub async fn history(
    pool: &SqlitePool,
    host_id: Uuid,
    from: Option<DateTime<Utc>>,
    to: Option<DateTime<Utc>>,
    limit: i64,
) -> anyhow::Result<Option<Vec<HistoryPoint>>> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM monitored_hosts WHERE host_id=? AND lifecycle_status='active')",
    )
    .bind(host_id)
    .fetch_one(pool)
    .await?;
    if !exists {
        return Ok(None);
    }
    let rows: Vec<HistoryRow> = sqlx::query_as(
        r#"SELECT report_id,collected_at,received_at,cpu_usage_percent,memory_usage_percent,
         network_received_bytes_per_second,network_transmitted_bytes_per_second,disk_read_bytes_per_second,
         disk_written_bytes_per_second,max_temperature_celsius,gpu_utilization_percent,gpu_memory_usage_percent
         FROM client_metric_reports WHERE host_id=?1
           AND (?2 IS NULL OR collected_at >= ?2)
           AND (?3 IS NULL OR collected_at <= ?3)
         ORDER BY collected_at DESC,report_id DESC LIMIT ?4"#,
    ).bind(host_id).bind(from).bind(to).bind(limit).fetch_all(pool).await?;
    let mut points: Vec<_> = rows
        .into_iter()
        .map(|row| HistoryPoint {
            report_id: row.report_id.to_string(),
            collected_at: row.collected_at,
            received_at: row.received_at,
            metrics: MetricSummary {
                cpu_usage_percent: row.cpu_usage_percent,
                memory_usage_percent: row.memory_usage_percent,
                network_received_bytes_per_second: row.network_received_bytes_per_second,
                network_transmitted_bytes_per_second: row.network_transmitted_bytes_per_second,
                disk_read_bytes_per_second: row.disk_read_bytes_per_second,
                disk_written_bytes_per_second: row.disk_written_bytes_per_second,
                max_temperature_celsius: row.max_temperature_celsius,
                gpu_utilization_percent: row.gpu_utilization_percent,
                gpu_memory_usage_percent: row.gpu_memory_usage_percent,
            },
        })
        .collect();
    points.reverse();
    Ok(Some(points))
}

pub async fn update_remark(
    pool: &SqlitePool,
    host_id: Uuid,
    remark: &str,
    actor: &str,
) -> anyhow::Result<bool> {
    let mut tx = pool.begin().await?;
    let changed = sqlx::query("UPDATE monitored_hosts SET name=? WHERE host_id=?")
        .bind(remark)
        .bind(host_id)
        .execute(&mut *tx)
        .await?
        .rows_affected()
        == 1;
    if changed {
        audit(
            &mut tx,
            "monitoring.instance.remark.update",
            &host_id.to_string(),
            None,
            actor,
        )
        .await?;
        tx.commit().await?;
    } else {
        tx.rollback().await?;
    }
    Ok(changed)
}

pub async fn delete_host(pool: &SqlitePool, host_id: Uuid, actor: &str) -> anyhow::Result<bool> {
    let mut tx = pool.begin().await?;
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM monitored_hosts WHERE host_id=?)")
            .bind(host_id)
            .fetch_one(&mut *tx)
            .await?;
    if !exists {
        tx.rollback().await?;
        return Ok(false);
    }
    audit(
        &mut tx,
        "monitoring.instance.delete",
        &host_id.to_string(),
        Some("host, reports, credentials, pairings and invites permanently deleted"),
        actor,
    )
    .await?;
    sqlx::query("DELETE FROM client_pairing_requests WHERE instance_id=? OR (requested_host_id=? AND status IN ('pending','denied'))")
        .bind(host_id).bind(host_id).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM client_instance_invites WHERE instance_id=?")
        .bind(host_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM monitored_hosts WHERE host_id=?")
        .bind(host_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(true)
}
