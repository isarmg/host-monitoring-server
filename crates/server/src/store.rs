use chrono::{DateTime, Utc};
use rand::TryRng;
use sqlx::{Acquire, Connection, FromRow, Row, Sqlite, SqlitePool, Transaction, types::Json};
use uuid::Uuid;
use xcss::admin_core::AdministratorStore;
use xsos_protocol::{
    Capability, ClientPairingMode, ClientPairingRequest, ClientReport, PairingStatus,
};

pub use crate::database_schema::{initialize_empty, open_existing, open_or_initialize};
use crate::model::{
    ClientInstanceSummary, ClientPairingPublicSummary, HistoryBucket, HistoryPoint,
    HistorySeriesResponse, HostCount, HostStatistics, HostSummary, MetricAggregate, MetricSummary,
    host_status,
};

const HISTORY_METRICS: [&str; 15] = [
    "cpu_usage_percent",
    "memory_usage_percent",
    "network_received_bytes_per_second",
    "network_transmitted_bytes_per_second",
    "disk_read_bytes_per_second",
    "disk_written_bytes_per_second",
    "max_temperature_celsius",
    "gpu_utilization_percent",
    "gpu_memory_usage_percent",
    "cpu_frequency_mhz",
    "gpu_power_watts",
    "gpu_core_clock_mhz",
    "max_fan_rpm",
    "max_disk_temperature_celsius",
    "max_disk_percentage_used",
];

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
           AND (SELECT COUNT(*) FROM pragma_table_info('client_metric_hourly_aggregates'))=66",
    )
    .fetch_one(pool)
    .await
    .is_ok_and(|ready| ready == 1)
}

pub fn normalize_username(username: &str) -> anyhow::Result<String> {
    xcss::admin_auth::normalize_administrator_username(username).map_err(anyhow::Error::from)
}

pub async fn ensure_admin_user(
    pool: &SqlitePool,
    username: &str,
    password: Option<&str>,
) -> anyhow::Result<()> {
    let store = xcss::admin_sqlite::SqliteAdministratorStore::new(pool.clone());
    let service = xcss::admin_core::AdministratorService::new(store);
    if service.store().administrator_count().await? == 0 {
        let password = password.ok_or_else(|| {
            anyhow::anyhow!(
                "XSOS_BOOTSTRAP_ADMIN_PASSWORD is required while no administrators exist"
            )
        })?;
        service
            .bootstrap_administrator(username, password, now_micros()?)
            .await?;
    }
    service.store().validate_all_administrators().await?;
    Ok(())
}

pub async fn reset_admin_password(
    pool: &SqlitePool,
    username: &str,
    password: &str,
) -> anyhow::Result<()> {
    let store = xcss::admin_sqlite::SqliteAdministratorStore::new(pool.clone());
    store.validate_all_administrators().await?;
    xcss::admin_core::AdministratorService::new(store)
        .change_administrator_password(username, password, now_micros()?)
        .await
        .map_err(anyhow::Error::from)
}

/// Startup validates the current administrator state and never bootstraps it.
pub async fn require_administrator(pool: &SqlitePool) -> anyhow::Result<()> {
    let store = xcss::admin_sqlite::SqliteAdministratorStore::new(pool.clone());
    anyhow::ensure!(
        store.administrator_count().await? > 0,
        "service is not initialized; run init to create the first administrator"
    );
    store.validate_all_administrators().await?;
    Ok(())
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
    let activation_code = random_authorization_code();
    let activation_hash = crate::token_hash(&activation_code);
    let authorization_code_enc = secrets.encrypt(instance_id, &activation_code)?;
    let created_at = Utc::now();
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    crate::capacity::instance(&mut tx, crate::capacity::Limits::PRODUCTION).await?;
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
) -> anyhow::Result<(Vec<ClientInstanceSummary>, Vec<HostSummary>)> {
    let (page, hosts) = list_invites_page(pool, secrets, None, None, "internal").await?;
    Ok((page.rows, hosts))
}

pub async fn list_invites_page(
    pool: &SqlitePool,
    secrets: &crate::crypto::SecretBox,
    cursor: Option<crate::pagination::Cursor>,
    focused: Option<Uuid>,
    scope: &str,
) -> anyhow::Result<(
    crate::pagination::Page<ClientInstanceSummary>,
    Vec<HostSummary>,
)> {
    let secrets = secrets.clone();
    let scope = scope.to_owned();
    crate::pagination::query(pool, move |conn| Box::pin(async move {
        let mut query = sqlx::QueryBuilder::<Sqlite>::new(
            "SELECT invite_id,instance_id,CASE WHEN length(CAST(display_name AS BLOB))>128 THEN '[invalid]' ELSE display_name END AS display_name,created_at,status,substr(authorization_code_enc,1,4097) AS authorization_code_enc FROM client_instance_invites WHERE 1=1");
        if let Some(id) = focused { query.push(" AND (invite_id=").push_bind(id).push(" OR instance_id=").push_bind(id).push(")"); }
        if let Some(key) = &cursor {
            query.push(" AND (CASE WHEN length(CAST(display_name AS BLOB))>128 THEN '[invalid]' ELSE display_name END COLLATE NOCASE,CASE WHEN length(CAST(display_name AS BLOB))>128 THEN '[invalid]' ELSE display_name END,invite_id)")
                .push(if key.newer { " < (" } else { " > (" })
                .push_bind(&key.sort).push(" COLLATE NOCASE,").push_bind(&key.sort).push(",").push_bind(key.id).push(")");
        }
        query.push(if cursor.as_ref().is_some_and(|c|c.newer) { " ORDER BY display_name COLLATE NOCASE DESC,display_name DESC,invite_id DESC" } else { " ORDER BY display_name COLLATE NOCASE,display_name,invite_id" }).push(" LIMIT ").push_bind(if focused.is_some() { 1i64 } else { 51 });
        let rows = query.build().fetch_all(&mut *conn).await?;
        let instances = rows.iter().map(|row|client_instance(row,&secrets)).collect::<anyhow::Result<Vec<_>>>()?;
        let page = crate::pagination::page(instances,cursor.as_ref(),&scope,|row|(row.display_name.clone(),Uuid::parse_str(&row.request_id).expect("stored UUID")))?;
        let mut hosts = Vec::new();
        if !page.rows.is_empty() {
            let mut query = sqlx::QueryBuilder::<Sqlite>::new(HOST_SELECT);
            query.push(" WHERE h.lifecycle_status='active' AND h.host_id IN (");
            let mut ids = query.separated(",");
            for instance in &page.rows { ids.push_bind(Uuid::parse_str(&instance.instance_id)?); }
            ids.push_unseparated(") ORDER BY name COLLATE NOCASE,name,h.host_id");
            hosts = query.build_query_as::<HostRow>().fetch_all(&mut *conn).await?.into_iter().map(summarize).collect();
        }
        Ok((page,hosts))
    })).await
}

fn random_authorization_code() -> String {
    const ALPHABET: &[u8; 36] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut value = String::with_capacity(36);
    let mut bytes = [0_u8; 64];
    while value.len() < 36 {
        rand::rngs::SysRng
            .try_fill_bytes(&mut bytes)
            .expect("operating system entropy is available");
        for byte in bytes {
            if byte < 252 {
                value.push(ALPHABET[usize::from(byte % 36)] as char);
                if value.len() == 36 {
                    break;
                }
            }
        }
    }
    value
}

#[cfg(test)]
mod authorization_code_tests {
    use super::*;

    #[test]
    fn generated_authorization_codes_have_the_shared_format() {
        for _ in 0..64 {
            let value = random_authorization_code();
            assert_eq!(value.len(), 36);
            assert!(
                value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || byte.is_ascii_lowercase())
            );
        }
    }
}

pub async fn validate_invite_authorizations(
    pool: &SqlitePool,
    secrets: &crate::crypto::SecretBox,
) -> anyhow::Result<()> {
    use futures_util::TryStreamExt;
    let mut rows=sqlx::query("SELECT instance_id,substr(activation_code_hash,1,65) AS activation_code_hash,substr(authorization_code_enc,1,4097) AS authorization_code_enc FROM client_instance_invites ORDER BY instance_id").fetch(pool);
    while let Some(row) = rows.try_next().await? {
        let instance_id = row.try_get::<Uuid, _>("instance_id")?;
        let code = secrets.decrypt(
            instance_id,
            &row.try_get::<Vec<u8>, _>("authorization_code_enc")?,
        )?;
        crate::model::validate_stored_activation_code(&code)
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
    // A client may not have polled its successful activation yet. Once its
    // credential is revoked, that request must no longer advertise "active".
    sqlx::query(
        "UPDATE client_pairing_requests SET status='denied' WHERE instance_id=? AND status='active'",
    )
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
    let row = sqlx::query(
        "SELECT invite_id,instance_id,display_name,created_at,status,authorization_code_enc \
         FROM client_instance_invites WHERE invite_id = ?",
    )
    .bind(invite_id)
    .fetch_one(&mut *tx)
    .await?;
    let instance = client_instance(&row, secrets)?;
    tx.commit().await?;
    Ok(Some(instance))
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
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
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
        audit(
            &mut tx,
            "monitoring.client_instance.invite.delete",
            &instance_id.to_string(),
            Some(&format!("invite_id={invite_id}")),
            actor,
        )
        .await?;
        delete_host_records(&mut tx, instance_id).await?;
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

pub async fn delete_invite(
    pool: &SqlitePool,
    invite_id: Uuid,
    actor: &str,
) -> anyhow::Result<bool> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let instance_id: Option<Uuid> =
        sqlx::query_scalar("SELECT instance_id FROM client_instance_invites WHERE invite_id=?")
            .bind(invite_id)
            .fetch_optional(&mut *tx)
            .await?;
    let Some(instance_id) = instance_id else {
        tx.rollback().await?;
        return Ok(false);
    };
    audit(
        &mut tx,
        "monitoring.client_instance.delete",
        &instance_id.to_string(),
        Some(&format!("invite_id={invite_id}")),
        actor,
    )
    .await?;
    delete_host_records(&mut tx, instance_id).await?;
    tx.commit().await?;
    Ok(true)
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
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    // Pairing rows can be cleaned up while their issued credential row remains.
    // Reusing that token would otherwise create a request that cannot activate
    // because client_credentials.token_hash is unique, including revoked rows.
    let issued_token: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM client_credentials WHERE token_hash=?)")
            .bind(&request.token_hash)
            .fetch_one(&mut *tx)
            .await?;
    if issued_token {
        tx.rollback().await?;
        return Ok(CreatePairingResult::Conflict);
    }
    // Serialize identical polling secrets without locking the whole table.
    // SQLite serializes writes with its database lock; no advisory lock is needed.
    let existing = sqlx::query(
        "SELECT request_id,requested_host_id,pairing_mode,os,os_version,kernel_version,arch,client_version,token_hash,status,expires_at \
         FROM client_pairing_requests WHERE polling_secret_hash=?",
    ).bind(&request.polling_secret_hash).fetch_optional(&mut *tx).await?;
    if let Some(row) = existing {
        let matches = row.try_get::<Uuid, _>("requested_host_id")?.to_string() == request.host.id
            && row.try_get::<String, _>("pairing_mode")? == pairing_mode(request.mode)
            && row.try_get::<String, _>("os")? == request.host.os.trim()
            && row.try_get::<Option<String>, _>("os_version")? == request.host.os_version
            && row.try_get::<Option<String>, _>("kernel_version")? == request.host.kernel_version
            && row.try_get::<String, _>("arch")? == request.host.arch.trim()
            && row.try_get::<String, _>("client_version")? == request.host.client_version.trim()
            && row.try_get::<String, _>("token_hash")? == request.token_hash;
        let expires_at: DateTime<Utc> = row.try_get("expires_at")?;
        let status: String = row.try_get("status")?;
        let request_id: Uuid = row.try_get("request_id")?;
        if !matches || status == "denied" {
            tx.rollback().await?;
            return Ok(CreatePairingResult::Conflict);
        }
        if status == "expired" {
            tx.rollback().await?;
            return Ok(CreatePairingResult::Expired);
        }
        if status == "pending" && expires_at <= Utc::now() {
            sqlx::query("UPDATE client_pairing_requests SET status='expired' WHERE request_id=? AND status='pending'")
                .bind(request_id)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            return Ok(CreatePairingResult::Expired);
        }
        if status != "pending" {
            tx.rollback().await?;
            return Ok(CreatePairingResult::Conflict);
        }
        tx.rollback().await?;
        return Ok(CreatePairingResult::Ready {
            request_id,
            expires_at,
            created: false,
        });
    }
    let now = Utc::now();
    let denied_cutoff = now - chrono::Duration::days(30);
    let expired_cutoff = now - chrono::Duration::hours(24);
    sqlx::query(
        "UPDATE client_pairing_requests SET status='expired' \
         WHERE status='pending' AND expires_at<=?",
    )
    .bind(now)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "DELETE FROM client_pairing_requests WHERE request_id IN (\
           SELECT request_id FROM client_pairing_requests \
           WHERE (status='expired' AND expires_at <= ?) OR (status='denied' AND created_at < ?) \
           ORDER BY created_at LIMIT 512)",
    )
    .bind(expired_cutoff)
    .bind(denied_cutoff)
    .execute(&mut *tx)
    .await?;
    let retained: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM client_pairing_requests")
        .fetch_one(&mut *tx)
        .await?;
    if retained >= 8192 {
        tx.commit().await?;
        return Ok(CreatePairingResult::AtCapacity);
    }
    if let Err(error) = crate::capacity::pairing(&mut tx, crate::capacity::Limits::PRODUCTION).await
    {
        if error.is::<crate::capacity::Exhausted>() {
            tx.commit().await?;
            return Ok(CreatePairingResult::AtCapacity);
        }
        return Err(error);
    }
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
               request_id,requested_host_id,pairing_mode,os,os_version,kernel_version,arch,client_version,
               token_hash,polling_secret_hash,expires_at,created_at)
           VALUES(?,?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT DO NOTHING"#,
    )
    .bind(request_id)
    .bind(requested_host_id)
    .bind(pairing_mode(request.mode))
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
    persist_pairing_expiration(pool, request_id).await?;
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
    persist_pairing_expiration(pool, request_id).await?;
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

async fn persist_pairing_expiration(pool: &SqlitePool, request_id: Uuid) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE client_pairing_requests SET status='expired' \
         WHERE request_id=? AND status='pending' AND expires_at<=?",
    )
    .bind(request_id)
    .bind(Utc::now())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn pairing_request_exists(pool: &SqlitePool, request_id: Uuid) -> anyhow::Result<bool> {
    let exists: i64 = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM client_pairing_requests WHERE request_id=?)",
    )
    .bind(request_id)
    .fetch_one(pool)
    .await?;
    Ok(exists != 0)
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
    secrets: &crate::crypto::SecretBox,
    request_id: Uuid,
    activation_hash: &str,
    actor: &str,
) -> anyhow::Result<ActivateResult> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let pairing = sqlx::query(
        "SELECT request_id,requested_host_id,pairing_mode,os,os_version,kernel_version,arch,client_version,token_hash,status,invite_id,instance_id,expires_at \
         FROM client_pairing_requests WHERE request_id=?",
    ).bind(request_id).fetch_optional(&mut *tx).await?;
    let Some(pairing) = pairing else {
        tx.rollback().await?;
        return Ok(ActivateResult::NotFound);
    };
    let invite = sqlx::query(
        "SELECT invite_id,instance_id,display_name,status,authorization_code_enc FROM client_instance_invites \
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
    let invite_instance_id: Uuid = invite.try_get("instance_id")?;
    let mode = parse_pairing_mode(&pairing.try_get::<String, _>("pairing_mode")?)?;
    let requested_host_id: Uuid = pairing.try_get("requested_host_id")?;
    let instance_id = match mode {
        ClientPairingMode::Fresh => invite_instance_id,
        ClientPairingMode::RecoverIdentity => requested_host_id,
    };
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
    if mode == ClientPairingMode::RecoverIdentity {
        let host_status: Option<String> =
            sqlx::query_scalar("SELECT lifecycle_status FROM monitored_hosts WHERE host_id=?")
                .bind(instance_id)
                .fetch_optional(&mut *tx)
                .await?;
        let invite_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM client_instance_invites WHERE instance_id=? AND invite_id<>?)",
        )
        .bind(instance_id)
        .bind(invite_id)
        .fetch_one(&mut *tx)
        .await?;
        let live_credential: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM client_credentials WHERE host_id=? AND revoked_at IS NULL)",
        )
        .bind(instance_id)
        .fetch_one(&mut *tx)
        .await?;
        // A rotated instance keeps its Host UUID and spool identity. The
        // matching pending invite may reactivate that revoked Host, but a new
        // invite must never claim an existing Host or a live credential.
        if invite_exists
            || live_credential
            || host_status
                .as_deref()
                .is_some_and(|status| status != "revoked" || invite_instance_id != instance_id)
        {
            tx.rollback().await?;
            return Ok(ActivateResult::Conflict);
        }
        if invite_instance_id != instance_id {
            let code = secrets.decrypt(
                invite_instance_id,
                &invite.try_get::<Vec<u8>, _>("authorization_code_enc")?,
            )?;
            let rebound = secrets.encrypt(instance_id, &code)?;
            sqlx::query(
                "UPDATE client_instance_invites SET instance_id=?,authorization_code_enc=? WHERE invite_id=? AND instance_id=?",
            )
            .bind(instance_id)
            .bind(rebound)
            .bind(invite_id)
            .bind(invite_instance_id)
            .execute(&mut *tx)
            .await?;
        }
    }
    let token_hash: String = pairing.try_get("token_hash")?;
    // A renewed authorization must not expose telemetry from the revoked
    // credential as the new client's current report. Keep its scalar history,
    // but release the full payload before replacing the latest pointer.
    sqlx::query(
        "UPDATE client_metric_reports SET payload=NULL WHERE report_id=(\
         SELECT latest_report_id FROM monitored_hosts WHERE host_id=?)",
    )
    .bind(instance_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO monitored_hosts(host_id,name,os,os_version,kernel_version,arch,client_version,registered_at,last_seen_at) \
         VALUES(?,?,?,?,?,?,?,?,?) ON CONFLICT(host_id) DO UPDATE SET name=excluded.name,os=excluded.os, \
         os_version=excluded.os_version,kernel_version=excluded.kernel_version,arch=excluded.arch, \
         client_version=excluded.client_version,last_seen_at=excluded.last_seen_at,capabilities='[]', \
         latest_report_id=NULL,latest_collected_at=NULL,latest_interval_seconds=NULL, \
         lifecycle_status='active',revoked_at=NULL",
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

fn pairing_mode(mode: ClientPairingMode) -> &'static str {
    match mode {
        ClientPairingMode::Fresh => "fresh",
        ClientPairingMode::RecoverIdentity => "recover_identity",
    }
}

fn parse_pairing_mode(value: &str) -> anyhow::Result<ClientPairingMode> {
    match value {
        "fresh" => Ok(ClientPairingMode::Fresh),
        "recover_identity" => Ok(ClientPairingMode::RecoverIdentity),
        _ => anyhow::bail!("invalid pairing mode in database"),
    }
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
#[derive(Clone)]
pub struct ReportWrite {
    report: ClientReport,
    token_hash: String,
    metrics: MetricSummary,
}

impl ReportWrite {
    pub(crate) fn host_id(&self) -> &str {
        &self.report.host.id
    }
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
    store_report_batch(
        pool,
        &[ReportWrite::new(
            report.clone(),
            token_hash.to_owned(),
            metrics.clone(),
        )],
    )
    .await?
    .pop()
    .expect("one submitted report")
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
    let reports = reports.to_vec();
    crate::pagination::query(pool, move |conn| {
        Box::pin(async move {
            let mut tx = conn.begin_with("BEGIN IMMEDIATE").await?;
            let mut results = Vec::with_capacity(reports.len());
            for write in &reports {
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
        })
    })
    .await
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
        "SELECT h.latest_report_id,h.latest_collected_at,r.received_at AS latest_received_at \
         FROM monitored_hosts h LEFT JOIN client_metric_reports r ON r.report_id=h.latest_report_id \
         WHERE h.host_id=? AND h.lifecycle_status='active' AND EXISTS(SELECT 1 FROM client_credentials c WHERE c.host_id=h.host_id AND c.token_hash=? AND c.revoked_at IS NULL)",
    ).bind(host_id).bind(token_hash).fetch_optional(&mut **tx).await?;
    let Some(current) = current else {
        return Err(ReportStoreError::Unauthorized.into());
    };
    let previous_report: Option<Uuid> = current.try_get("latest_report_id")?;
    let previous_collected: Option<DateTime<Utc>> = current.try_get("latest_collected_at")?;
    let previous_received: Option<DateTime<Utc>> = current.try_get("latest_received_at")?;
    let received_at = Utc::now();
    // Client time may be a few minutes ahead and then be corrected. Cap the
    // ordering key at server receipt time so such a sample cannot pin latest
    // after newer reports arrive. Older delayed samples remain historical.
    let incoming_key = (report.collected_at.min(received_at), received_at, report_id);
    let previous_key = previous_report
        .zip(previous_collected)
        .zip(previous_received)
        .map(|((id, collected), received)| (collected.min(received), received, id));
    let becomes_latest = previous_key.is_none_or(|previous| incoming_key > previous);
    crate::capacity::report(
        tx,
        host_id,
        report_id,
        if becomes_latest {
            serde_json::to_vec(report)?.len() as u64
        } else {
            0
        },
        crate::capacity::Limits::PRODUCTION,
    )
    .await?;
    let payload = becomes_latest.then(|| Json(report.clone()));
    let inserted = sqlx::query(
        r#"INSERT INTO client_metric_reports(
             report_id,host_id,schema_version,collected_at,received_at,interval_seconds,payload,
             cpu_usage_percent,memory_usage_percent,network_received_bytes_per_second,
             network_transmitted_bytes_per_second,disk_read_bytes_per_second,disk_written_bytes_per_second,
             max_temperature_celsius,gpu_utilization_percent,gpu_memory_usage_percent,cpu_frequency_mhz,gpu_power_watts,gpu_core_clock_mhz,max_fan_rpm,max_disk_temperature_celsius,max_disk_percentage_used)
           VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)
           ON CONFLICT(report_id) DO NOTHING RETURNING received_at"#,
    ).bind(report_id).bind(host_id).bind(i32::from(report.schema_version)).bind(report.collected_at)
      .bind(received_at).bind(report.interval_seconds).bind(payload)
      .bind(metrics.cpu_usage_percent).bind(metrics.memory_usage_percent)
      .bind(metrics.network_received_bytes_per_second).bind(metrics.network_transmitted_bytes_per_second)
      .bind(metrics.disk_read_bytes_per_second).bind(metrics.disk_written_bytes_per_second)
      .bind(metrics.max_temperature_celsius).bind(metrics.gpu_utilization_percent).bind(metrics.gpu_memory_usage_percent).bind(metrics.cpu_frequency_mhz).bind(metrics.gpu_power_watts).bind(metrics.gpu_core_clock_mhz).bind(metrics.max_fan_rpm).bind(metrics.max_disk_temperature_celsius).bind(metrics.max_disk_percentage_used)
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
    // Transport liveness is based on the server receipt time of every newly
    // accepted report. A client's sampling clock may move backwards; that must
    // not overwrite newer metrics, but it still proves that the client is
    // communicating successfully.
    sqlx::query(
        "UPDATE monitored_hosts SET last_seen_at = CASE WHEN last_seen_at > ? THEN last_seen_at ELSE ? END WHERE host_id = ?",
    )
    .bind(stored_received)
    .bind(stored_received)
    .bind(host_id)
    .execute(&mut **tx)
    .await?;
    if becomes_latest {
        sqlx::query(
            r#"UPDATE monitored_hosts SET
                 os=?,os_version=?,kernel_version=?,arch=?,client_version=?,capabilities=?,
                 latest_report_id=?,
                 latest_collected_at=?,latest_interval_seconds=? WHERE host_id=?"#,
        )
        .bind(report.host.os.trim())
        .bind(&report.host.os_version)
        .bind(&report.host.kernel_version)
        .bind(report.host.arch.trim())
        .bind(report.host.client_version.trim())
        .bind(Json(&report.capabilities))
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
    capabilities: Vec<u8>,
    invalid_metadata: bool,
    registered_at: DateTime<Utc>,
    last_seen_at: DateTime<Utc>,
    latest_report_id: Option<Uuid>,
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
    cpu_frequency_mhz: Option<f64>,
    gpu_power_watts: Option<f64>,
    gpu_core_clock_mhz: Option<f64>,
    max_fan_rpm: Option<f64>,
    max_disk_temperature_celsius: Option<f64>,
    max_disk_percentage_used: Option<f64>,
}

const HOST_SELECT: &str = r#"SELECT h.host_id,
 CASE WHEN COALESCE(length(CAST(h.name AS BLOB)),0)>128 THEN '[invalid]' ELSE h.name END AS name,
 CASE WHEN COALESCE(length(CAST(h.os AS BLOB)),0)>64 THEN '[invalid]' ELSE h.os END AS os,
 CASE WHEN COALESCE(length(CAST(h.os_version AS BLOB)),0)>128 THEN NULL ELSE h.os_version END AS os_version,
 CASE WHEN COALESCE(length(CAST(h.kernel_version AS BLOB)),0)>128 THEN NULL ELSE h.kernel_version END AS kernel_version,
 CASE WHEN COALESCE(length(CAST(h.arch AS BLOB)),0)>64 THEN '[invalid]' ELSE h.arch END AS arch,
 CASE WHEN COALESCE(length(CAST(h.client_version AS BLOB)),0)>128 THEN '[invalid]' ELSE h.client_version END AS client_version,
 CASE WHEN CASE WHEN h.registered_at IS NULL THEN 0 WHEN length(CAST(h.registered_at AS BLOB))>64 THEN 1 ELSE julianday(h.registered_at) IS NULL END THEN '1970-01-01T00:00:00+00:00' ELSE h.registered_at END AS registered_at,
 CASE WHEN CASE WHEN h.last_seen_at IS NULL THEN 0 WHEN length(CAST(h.last_seen_at AS BLOB))>64 THEN 1 ELSE julianday(h.last_seen_at) IS NULL END THEN '1970-01-01T00:00:00+00:00' ELSE h.last_seen_at END AS last_seen_at,
 CASE WHEN CASE WHEN h.latest_collected_at IS NULL THEN 0 WHEN length(CAST(h.latest_collected_at AS BLOB))>64 THEN 1 ELSE julianday(h.latest_collected_at) IS NULL END THEN NULL ELSE h.latest_collected_at END AS latest_collected_at,
 CASE WHEN length(CAST(h.capabilities AS BLOB))>524288 THEN CAST('[]' AS BLOB) ELSE CAST(h.capabilities AS BLOB) END AS capabilities,
 h.latest_report_id,
 h.latest_interval_seconds,
 (COALESCE(length(CAST(h.name AS BLOB)),0)>128 OR COALESCE(length(CAST(h.os AS BLOB)),0)>64 OR COALESCE(length(CAST(h.os_version AS BLOB)),0)>128 OR COALESCE(length(CAST(h.kernel_version AS BLOB)),0)>128 OR COALESCE(length(CAST(h.arch AS BLOB)),0)>64 OR COALESCE(length(CAST(h.client_version AS BLOB)),0)>128 OR (CASE WHEN h.registered_at IS NULL THEN 0 WHEN length(CAST(h.registered_at AS BLOB))>64 THEN 1 ELSE julianday(h.registered_at) IS NULL END) OR (CASE WHEN h.last_seen_at IS NULL THEN 0 WHEN length(CAST(h.last_seen_at AS BLOB))>64 THEN 1 ELSE julianday(h.last_seen_at) IS NULL END) OR (CASE WHEN h.latest_collected_at IS NULL THEN 0 WHEN length(CAST(h.latest_collected_at AS BLOB))>64 THEN 1 ELSE julianday(h.latest_collected_at) IS NULL END) OR length(CAST(h.capabilities AS BLOB))>524288) AS invalid_metadata,
 r.cpu_usage_percent,
 r.memory_usage_percent,
 r.network_received_bytes_per_second,
 r.network_transmitted_bytes_per_second,
 r.disk_read_bytes_per_second,
 r.disk_written_bytes_per_second,
 r.max_temperature_celsius,
 r.gpu_utilization_percent,
 r.gpu_memory_usage_percent,
 r.cpu_frequency_mhz,
 r.gpu_power_watts,
 r.gpu_core_clock_mhz,
 r.max_fan_rpm,
 r.max_disk_temperature_celsius,
 r.max_disk_percentage_used
 FROM monitored_hosts h LEFT JOIN client_metric_reports r ON r.report_id=h.latest_report_id"#;

fn summarize(row: HostRow) -> HostSummary {
    let capabilities = serde_json::from_slice::<Vec<Capability>>(&row.capabilities).ok();
    let invalid_capabilities = capabilities.as_ref().is_none_or(|values| {
        values.len() > xsos_protocol::CLIENT_REPORT_MAX_CAPABILITIES
            || values.iter().any(|value| {
                value.name.len() > xsos_protocol::CLIENT_REPORT_MAX_CAPABILITY_NAME_BYTES
                    || value.source.len() > xsos_protocol::CLIENT_REPORT_MAX_CAPABILITY_SOURCE_BYTES
                    || value.message.as_ref().is_some_and(|message| {
                        message.len() > xsos_protocol::CLIENT_REPORT_MAX_CAPABILITY_MESSAGE_BYTES
                    })
            })
    });
    let invalid_metadata = row.invalid_metadata || invalid_capabilities;
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
        status: if invalid_metadata {
            "unavailable".into()
        } else if row.latest_report_id.is_some() {
            host_status(row.last_seen_at, row.latest_interval_seconds)
        } else {
            "offline".into()
        },
        capabilities: if invalid_capabilities {
            Vec::new()
        } else {
            capabilities.unwrap_or_default()
        },
        data_error: invalid_metadata.then_some("stored_host_data_invalid"),
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
            cpu_frequency_mhz: row.cpu_frequency_mhz,
            gpu_power_watts: row.gpu_power_watts,
            gpu_core_clock_mhz: row.gpu_core_clock_mhz,
            max_fan_rpm: row.max_fan_rpm,
            max_disk_temperature_celsius: row.max_disk_temperature_celsius,
            max_disk_percentage_used: row.max_disk_percentage_used,
        },
    }
}

pub async fn list_hosts(pool: &SqlitePool) -> anyhow::Result<Vec<HostSummary>> {
    Ok(list_hosts_page(pool, None, None, "internal").await?.0.rows)
}

pub async fn list_hosts_page(
    pool: &SqlitePool,
    cursor: Option<crate::pagination::Cursor>,
    focused: Option<Uuid>,
    scope: &str,
) -> anyhow::Result<(crate::pagination::Page<HostSummary>, HostStatistics)> {
    let scope = scope.to_owned();
    crate::pagination::query(pool,move |conn|Box::pin(async move {
        // Compare the same bounded projection that is serialized into the cursor.
        let mut query=sqlx::QueryBuilder::<Sqlite>::new(format!("SELECT * FROM ({HOST_SELECT}) WHERE 1=1"));
        query.push(" AND host_id IN (SELECT host_id FROM monitored_hosts WHERE lifecycle_status='active')");
        if let Some(id)=focused {query.push(" AND host_id=").push_bind(id);}
        if let Some(key)=&cursor {
            query.push(" AND (name COLLATE NOCASE,name,host_id)").push(if key.newer {" < ("}else{" > ("}).push_bind(&key.sort).push(" COLLATE NOCASE,").push_bind(&key.sort).push(",").push_bind(key.id).push(")");
        }
        query.push(if cursor.as_ref().is_some_and(|c|c.newer){" ORDER BY name COLLATE NOCASE DESC,name DESC,host_id DESC"}else{" ORDER BY name COLLATE NOCASE,name,host_id"}).push(" LIMIT ").push_bind(if focused.is_some(){1i64}else{51});
        let rows=query.build_query_as::<HostRow>().fetch_all(&mut *conn).await?.into_iter().map(summarize).collect();
        let page=crate::pagination::page(rows,cursor.as_ref(),&scope,|row:&HostSummary|(row.name.clone(),Uuid::parse_str(&row.id).expect("stored UUID")))?;
        let statistics=host_statistics_connection(conn).await?;
        Ok((page,statistics))
    })).await
}

pub async fn host_statistics(pool: &SqlitePool) -> anyhow::Result<HostStatistics> {
    crate::pagination::query(pool, |conn| Box::pin(host_statistics_connection(conn))).await
}

async fn host_statistics_connection(
    conn: &mut sqlx::SqliteConnection,
) -> anyhow::Result<HostStatistics> {
    // SQLite aggregates only fixed counters; no per-host allocation escapes.
    let row=sqlx::query(r#"WITH classified AS (
      SELECT CASE WHEN length(CAST(os AS BLOB))>64 THEN '' ELSE lower(trim(os)) END AS os,
      latest_report_id IS NOT NULL AND length(CAST(last_seen_at AS BLOB))<=64 AND
      max(0,CAST((julianday(?)-julianday(last_seen_at))*86400 AS INTEGER)) <= max(30,min(3600,max(1,coalesce(latest_interval_seconds,10)))*3) AS online
      FROM monitored_hosts WHERE lifecycle_status='active'
    ), kinds AS (SELECT CASE WHEN instr(os,'windows')>0 THEN 'windows' WHEN instr(os,'linux')>0 THEN 'linux' WHEN instr(os,'macos')>0 OR instr(os,'mac os')>0 OR instr(os,'darwin')>0 THEN 'macos' ELSE '' END AS kind,online FROM classified)
    SELECT count(*) AS total,coalesce(sum(online),0) AS online,
    coalesce(sum(kind='windows'),0) AS windows,coalesce(sum(kind='windows' AND online),0) AS windows_online,
    coalesce(sum(kind='linux'),0) AS linux,coalesce(sum(kind='linux' AND online),0) AS linux_online,
    coalesce(sum(kind='macos'),0) AS macos,coalesce(sum(kind='macos' AND online),0) AS macos_online FROM kinds"#).bind(Utc::now()).fetch_one(conn).await?;
    Ok(HostStatistics {
        total: HostCount {
            total: row.try_get("total")?,
            online: row.try_get("online")?,
        },
        windows: HostCount {
            total: row.try_get("windows")?,
            online: row.try_get("windows_online")?,
        },
        linux: HostCount {
            total: row.try_get("linux")?,
            online: row.try_get("linux_online")?,
        },
        macos: HostCount {
            total: row.try_get("macos")?,
            online: row.try_get("macos_online")?,
        },
    })
}

pub async fn get_host(
    pool: &SqlitePool,
    host_id: Uuid,
) -> anyhow::Result<Option<(HostSummary, Option<ClientReport>)>> {
    crate::pagination::query(pool,move |conn|Box::pin(async move{
        let sql=format!("{HOST_SELECT} WHERE h.host_id=? AND h.lifecycle_status='active'");
        let row:Option<HostRow>=sqlx::query_as(sqlx::AssertSqlSafe(sql)).bind(host_id).fetch_optional(&mut *conn).await?;
        let Some(row)=row else{return Ok(None)};
        let payload:Option<Vec<u8>>=sqlx::query_scalar("SELECT substr(CAST(r.payload AS BLOB),1,524289) FROM monitored_hosts h LEFT JOIN client_metric_reports r ON r.report_id=h.latest_report_id WHERE h.host_id=?").bind(host_id).fetch_one(&mut *conn).await?;
        let mut host=summarize(row);
        let latest=payload.as_ref().and_then(|bytes|if bytes.len()<=524288 {serde_json::from_slice::<ClientReport>(bytes).ok()}else{None});
        let latest=latest.filter(|report|report.host.id==host.id&&crate::model::validate_report(report).is_ok());
        if payload.is_some()&&latest.is_none(){host.data_error=Some("stored_host_data_invalid");host.status="unavailable".into();}
        Ok(Some((host,latest)))
    })).await
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
    cpu_frequency_mhz: Option<f64>,
    gpu_power_watts: Option<f64>,
    gpu_core_clock_mhz: Option<f64>,
    max_fan_rpm: Option<f64>,
    max_disk_temperature_celsius: Option<f64>,
    max_disk_percentage_used: Option<f64>,
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
         disk_written_bytes_per_second,max_temperature_celsius,gpu_utilization_percent,gpu_memory_usage_percent,cpu_frequency_mhz,gpu_power_watts,gpu_core_clock_mhz,max_fan_rpm,max_disk_temperature_celsius,max_disk_percentage_used
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
                cpu_frequency_mhz: row.cpu_frequency_mhz,
                gpu_power_watts: row.gpu_power_watts,
                gpu_core_clock_mhz: row.gpu_core_clock_mhz,
                max_fan_rpm: row.max_fan_rpm,
                max_disk_temperature_celsius: row.max_disk_temperature_celsius,
                max_disk_percentage_used: row.max_disk_percentage_used,
            },
        })
        .collect();
    points.reverse();
    Ok(Some(points))
}

#[derive(FromRow, serde::Serialize)]
pub struct ReportLogRow {
    #[serde(skip)]
    pub sort_key: String,
    pub report_id: Uuid,
    pub collected_at: DateTime<Utc>,
    pub received_at: DateTime<Utc>,
}

/// Reports are grouped by server receipt time. Client collection time may be
/// delayed or clock-skewed, so it cannot define a server calendar day.
pub async fn report_logs(
    pool: &SqlitePool,
    host_id: Uuid,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> anyhow::Result<Option<Vec<ReportLogRow>>> {
    Ok(report_logs_page(pool, host_id, from, to, None, "internal")
        .await?
        .map(|page| page.rows))
}

pub async fn report_logs_page(
    pool: &SqlitePool,
    host_id: Uuid,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    cursor: Option<crate::pagination::Cursor>,
    scope: &str,
) -> anyhow::Result<Option<crate::pagination::Page<ReportLogRow>>> {
    let scope = scope.to_owned();
    crate::pagination::query(pool,move |conn|Box::pin(async move {
        let exists:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM monitored_hosts WHERE host_id=? AND lifecycle_status='active')").bind(host_id).fetch_one(&mut *conn).await?;
        if !exists{return Ok(None)};
        let mut query=sqlx::QueryBuilder::<Sqlite>::new("SELECT report_id,CASE WHEN length(CAST(collected_at AS BLOB))>64 THEN NULL ELSE collected_at END AS collected_at,CASE WHEN length(CAST(received_at AS BLOB))>64 THEN NULL ELSE received_at END AS received_at,CASE WHEN length(CAST(received_at AS BLOB))>64 THEN '' ELSE received_at END AS sort_key FROM client_metric_reports WHERE host_id=");
        query.push_bind(host_id).push(" AND received_at>=").push_bind(from).push(" AND received_at<").push_bind(to);
        if let Some(key)=&cursor {query.push(" AND (received_at,report_id)").push(if key.newer {" > ("}else{" < ("}).push_bind(&key.sort).push(",").push_bind(key.id).push(")");}
        query.push(if cursor.as_ref().is_some_and(|c|c.newer){" ORDER BY received_at,report_id"}else{" ORDER BY received_at DESC,report_id DESC"}).push(" LIMIT 51");
        let rows=query.build_query_as::<ReportLogRow>().fetch_all(&mut *conn).await?;
        Ok(Some(crate::pagination::page(rows,cursor.as_ref(),&scope,|row|(row.sort_key.clone(),row.report_id))?))
    })).await
}

pub async fn history_series(
    pool: &SqlitePool,
    host_id: Uuid,
    requested_from: DateTime<Utc>,
    requested_to: DateTime<Utc>,
    max_points: i64,
) -> anyhow::Result<Option<HistorySeriesResponse>> {
    let mut tx = pool.begin().await?;
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM monitored_hosts WHERE host_id=? AND lifecycle_status='active')",
    )
    .bind(host_id)
    .fetch_one(&mut *tx)
    .await?;
    if !exists {
        tx.commit().await?;
        return Ok(None);
    }
    let span = (requested_to - requested_from).num_seconds().max(1);
    let minimum_step = ((span + max_points - 1) / max_points).max(1);
    let mut step = [1, 2, 5, 10, 30, 60, 120, 300, 900, 3600, 21_600, 86_400]
        .into_iter()
        .find(|candidate| *candidate >= minimum_step)
        .unwrap_or(86_400);
    // Round the complete timestamp up before aligning it to a bucket. Taking
    // timestamp() alone would exclude the fractional part of the last second.
    let to_seconds =
        requested_to.timestamp() + i64::from(requested_to.timestamp_subsec_nanos() != 0);
    let (actual_from, actual_to, has_hourly) = loop {
        let from_epoch = requested_from.timestamp().div_euclid(step) * step;
        let to_epoch = ((to_seconds + step - 1).div_euclid(step)) * step;
        if (to_epoch - from_epoch) / step > max_points {
            step = [2, 5, 10, 30, 60, 120, 300, 900, 3600, 21_600, 86_400]
                .into_iter()
                .find(|candidate| *candidate > step)
                .ok_or_else(|| anyhow::anyhow!("history range exceeds the point budget"))?;
            continue;
        }
        let actual_from = DateTime::from_timestamp(from_epoch, 0)
            .ok_or_else(|| anyhow::anyhow!("history start is outside the supported range"))?;
        let actual_to = DateTime::from_timestamp(to_epoch, 0)
            .ok_or_else(|| anyhow::anyhow!("history end is outside the supported range"))?;
        // A sparse aggregate still represents its complete UTC hour. Include
        // the hour containing the candidate start, then realign if its data
        // requires hourly resolution. At hourly or coarser resolution this
        // predicate is identical to the final aggregate selection below.
        let hourly_from = DateTime::from_timestamp(from_epoch.div_euclid(3600) * 3600, 0)
            .ok_or_else(|| anyhow::anyhow!("history start is outside the supported range"))?;
        let has_hourly: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM client_metric_hourly_aggregates WHERE host_id=? AND bucket_start>=? AND bucket_start<?)",
        )
        .bind(host_id)
        .bind(hourly_from)
        .bind(actual_to)
        .fetch_one(&mut *tx)
        .await?;
        if has_hourly && step < 3600 {
            step = 3600;
            continue;
        }
        break (actual_from, actual_to, has_hourly);
    };
    let has_raw: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM client_metric_reports WHERE host_id=? AND aggregated_at IS NULL AND collected_at>=? AND collected_at<?)",
    )
    .bind(host_id)
    .bind(actual_from)
    .bind(actual_to)
    .fetch_one(&mut *tx)
    .await?;

    let mut raw_metrics = String::new();
    let mut hourly_metrics = String::new();
    let mut grouped_metrics = String::new();
    for metric in HISTORY_METRICS {
        raw_metrics.push_str(&format!(
            ",CASE WHEN {metric} IS NULL THEN 0 ELSE 1 END AS {metric}_count,{metric} AS {metric}_min,{metric} AS {metric}_max,{metric} AS {metric}_avg"
        ));
        hourly_metrics.push_str(&format!(
            ",{metric}_count,{metric}_min,{metric}_max,{metric}_avg"
        ));
        grouped_metrics.push_str(&format!(
            ",SUM({metric}_count) AS {metric}_count,MIN(CASE WHEN {metric}_count>0 THEN {metric}_min END) AS {metric}_min,MAX(CASE WHEN {metric}_count>0 THEN {metric}_max END) AS {metric}_max,CASE WHEN SUM({metric}_count)=0 THEN NULL ELSE SUM({metric}_avg*{metric}_count)/SUM({metric}_count) END AS {metric}_avg"
        ));
    }
    // Offsets from the aligned start are nonnegative, so SQLite's integer
    // division keeps buckets in range even for timestamps before the Unix epoch.
    let sql = format!(
        "WITH source AS (\
           SELECT unixepoch(collected_at) AS start_epoch,unixepoch(collected_at) AS end_epoch,1 AS sample_count{raw_metrics} \
             FROM client_metric_reports WHERE host_id=? AND aggregated_at IS NULL AND collected_at>=? AND collected_at<? \
           UNION ALL \
           SELECT unixepoch(interval_start),unixepoch(interval_end),sample_count{hourly_metrics} \
             FROM client_metric_hourly_aggregates WHERE host_id=? AND bucket_start>=? AND bucket_start<?\
         ) SELECT ((start_epoch-?)/?)*?+? AS bucket_epoch,MIN(start_epoch) AS interval_start_epoch,MAX(end_epoch) AS interval_end_epoch,SUM(sample_count) AS sample_count{grouped_metrics} \
           FROM source GROUP BY bucket_epoch ORDER BY bucket_epoch LIMIT ?"
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(host_id)
        .bind(actual_from)
        .bind(actual_to)
        .bind(host_id)
        .bind(actual_from)
        .bind(actual_to)
        .bind(actual_from.timestamp())
        .bind(step)
        .bind(step)
        .bind(actual_from.timestamp())
        .bind(max_points)
        .fetch_all(&mut *tx)
        .await?;
    let mut points = Vec::with_capacity(rows.len());
    for row in rows {
        let metric = |name: &str| -> anyhow::Result<MetricAggregate> {
            Ok(MetricAggregate {
                count: row.try_get(format!("{name}_count").as_str())?,
                min: row.try_get(format!("{name}_min").as_str())?,
                max: row.try_get(format!("{name}_max").as_str())?,
                avg: row.try_get(format!("{name}_avg").as_str())?,
            })
        };
        let bucket_epoch: i64 = row.try_get("bucket_epoch")?;
        points.push(HistoryBucket {
            start: DateTime::from_timestamp(bucket_epoch, 0)
                .ok_or_else(|| anyhow::anyhow!("invalid history bucket timestamp"))?,
            end: DateTime::from_timestamp(bucket_epoch + step, 0)
                .ok_or_else(|| anyhow::anyhow!("invalid history bucket end timestamp"))?,
            sample_count: row.try_get("sample_count")?,
            cpu_usage_percent: metric("cpu_usage_percent")?,
            memory_usage_percent: metric("memory_usage_percent")?,
            network_received_bytes_per_second: metric("network_received_bytes_per_second")?,
            network_transmitted_bytes_per_second: metric("network_transmitted_bytes_per_second")?,
            disk_read_bytes_per_second: metric("disk_read_bytes_per_second")?,
            disk_written_bytes_per_second: metric("disk_written_bytes_per_second")?,
            max_temperature_celsius: metric("max_temperature_celsius")?,
            gpu_utilization_percent: metric("gpu_utilization_percent")?,
            gpu_memory_usage_percent: metric("gpu_memory_usage_percent")?,
            cpu_frequency_mhz: metric("cpu_frequency_mhz")?,
            gpu_power_watts: metric("gpu_power_watts")?,
            gpu_core_clock_mhz: metric("gpu_core_clock_mhz")?,
            max_fan_rpm: metric("max_fan_rpm")?,
            max_disk_temperature_celsius: metric("max_disk_temperature_celsius")?,
            max_disk_percentage_used: metric("max_disk_percentage_used")?,
        });
    }
    tx.commit().await?;
    Ok(Some(HistorySeriesResponse {
        host_id: host_id.to_string(),
        requested_from,
        requested_to,
        actual_from,
        actual_to,
        step_seconds: step,
        source: match (has_raw, has_hourly) {
            (true, true) => "mixed",
            (false, true) => "hourly",
            _ => "raw",
        }
        .to_owned(),
        points,
    }))
}

pub async fn update_instance_name(
    pool: &SqlitePool,
    invite_id: Uuid,
    name: &str,
    actor: &str,
) -> anyhow::Result<bool> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let instance_id: Option<Uuid> = sqlx::query_scalar(
        "UPDATE client_instance_invites SET display_name=? WHERE invite_id=? RETURNING instance_id",
    )
    .bind(name)
    .bind(invite_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(instance_id) = instance_id else {
        tx.rollback().await?;
        return Ok(false);
    };
    sqlx::query("UPDATE monitored_hosts SET name=? WHERE host_id=?")
        .bind(name)
        .bind(instance_id)
        .execute(&mut *tx)
        .await?;
    audit(
        &mut tx,
        "monitoring.client_instance.name.update",
        &instance_id.to_string(),
        Some(&format!("invite_id={invite_id}")),
        actor,
    )
    .await?;
    tx.commit().await?;
    Ok(true)
}

pub async fn delete_host(pool: &SqlitePool, host_id: Uuid, actor: &str) -> anyhow::Result<bool> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
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
    delete_host_records(&mut tx, host_id).await?;
    tx.commit().await?;
    Ok(true)
}

async fn delete_host_records(
    tx: &mut Transaction<'_, Sqlite>,
    host_id: Uuid,
) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM client_pairing_requests WHERE instance_id=? OR requested_host_id=?")
        .bind(host_id)
        .bind(host_id)
        .execute(&mut **tx)
        .await?;
    sqlx::query("DELETE FROM client_instance_invites WHERE instance_id=?")
        .bind(host_id)
        .execute(&mut **tx)
        .await?;
    sqlx::query("DELETE FROM monitored_hosts WHERE host_id=?")
        .bind(host_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}
