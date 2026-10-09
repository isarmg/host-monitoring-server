use chrono::{Duration, Utc};
use sqlx::SqlitePool;
use std::collections::HashSet;
use uuid::Uuid;
use xsos::{pagination::Cursor, store};

async fn fixture() -> (tempfile::TempDir, SqlitePool) {
    let directory = tempfile::tempdir_in(std::env::current_dir().unwrap()).unwrap();
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(3)
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(directory.path().join("current.sqlite"))
                .create_if_missing(true),
        )
        .await
        .unwrap();
    store::initialize_empty(&pool).await.unwrap();
    (directory, pool)
}
async fn host(pool: &SqlitePool, name: &str) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO monitored_hosts(host_id,name,os,arch,client_version,registered_at,last_seen_at) VALUES(?,?,'Linux','x86_64','1',?,?)").bind(id).bind(name).bind(Utc::now()).bind(Utc::now()).execute(pool).await.unwrap();
    id
}

#[tokio::test]
async fn hosts_and_instances_are_fully_traversable_with_inverse_and_focused_pages() {
    let (_directory, pool) = fixture().await;
    let mut ids = Vec::new();
    for i in 0..106 {
        ids.push(host(&pool, &format!("Host {i:03}")).await);
    }
    // A corrupt but boundedly projected row remains visible, and cannot swallow
    // the healthy rows which follow it. The original cell is not rewritten.
    let bad = "x".repeat(2 * 1024 * 1024);
    sqlx::query("UPDATE monitored_hosts SET capabilities=? WHERE host_id=?")
        .bind(&bad)
        .bind(ids[49])
        .execute(&pool)
        .await
        .unwrap();
    let mut all = HashSet::new();
    let mut cursor = None;
    let mut first = None;
    let mut second = None;
    loop {
        let (page, statistics) =
            store::list_hosts_page(&pool, cursor.clone(), None, "hosts-admin-a")
                .await
                .unwrap();
        assert!(page.rows.len() <= 50);
        assert_eq!(statistics.total.total, 106);
        if first.is_none() {
            assert_eq!(page.rows[49].data_error, Some("stored_host_data_invalid"));
            assert_eq!(page.rows[49].status, "unavailable");
            first = Some(
                page.rows
                    .iter()
                    .map(|row| row.id.clone())
                    .collect::<Vec<_>>(),
            );
        } else if second.is_none() {
            second = page.previous_cursor.clone();
        }
        for row in &page.rows {
            assert!(all.insert(row.id.clone()));
        }
        let Some(next) = page.next_cursor else { break };
        cursor = Cursor::decode(Some(&next), "hosts-admin-a").unwrap();
    }
    assert_eq!(all.len(), 106);
    let previous = Cursor::decode(second.as_deref(), "hosts-admin-a").unwrap();
    let (back, _) = store::list_hosts_page(&pool, previous, None, "hosts-admin-a")
        .await
        .unwrap();
    assert_eq!(
        back.rows
            .iter()
            .map(|row| row.id.clone())
            .collect::<Vec<_>>(),
        first.unwrap()
    );
    let (focused, _) = store::list_hosts_page(&pool, None, Some(ids[100]), "hosts-admin-a")
        .await
        .unwrap();
    assert_eq!(focused.rows.len(), 1);
    assert_eq!(focused.rows[0].id, ids[100].to_string());
    let unchanged: String =
        sqlx::query_scalar("SELECT capabilities FROM monitored_hosts WHERE host_id=?")
            .bind(ids[49])
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(unchanged, bad);
    let secrets = xsos::crypto::SecretBox::new([0x42; 32]);
    let mut expected = HashSet::new();
    for i in 0..56 {
        let (created, _) =
            store::create_invite(&pool, &secrets, &format!("Instance {i:03}"), "admin-a")
                .await
                .unwrap();
        let store::CreateInviteResult::Created(created) = created else {
            panic!("create invitation")
        };
        expected.insert(created.request_id);
    }
    let (first, _) = store::list_invites_page(&pool, &secrets, None, None, "instances-admin-a")
        .await
        .unwrap();
    assert_eq!(first.rows.len(), 50);
    let next = Cursor::decode(first.next_cursor.as_deref(), "instances-admin-a").unwrap();
    let (second, _) = store::list_invites_page(&pool, &secrets, next, None, "instances-admin-a")
        .await
        .unwrap();
    assert_eq!(second.rows.len(), 6);
    assert!(second.next_cursor.is_none());
    let actual: HashSet<_> = first
        .rows
        .iter()
        .chain(second.rows.iter())
        .map(|row| row.request_id.clone())
        .collect();
    assert_eq!(actual, expected);
    let inverse = Cursor::decode(second.previous_cursor.as_deref(), "instances-admin-a").unwrap();
    let (back, _) = store::list_invites_page(&pool, &secrets, inverse, None, "instances-admin-a")
        .await
        .unwrap();
    assert_eq!(
        first
            .rows
            .iter()
            .map(|row| &row.request_id)
            .collect::<Vec<_>>(),
        back.rows
            .iter()
            .map(|row| &row.request_id)
            .collect::<Vec<_>>()
    );
    let focus = Uuid::parse_str(&second.rows[5].request_id).unwrap();
    let (focused, _) =
        store::list_invites_page(&pool, &secrets, None, Some(focus), "instances-admin-a")
            .await
            .unwrap();
    assert_eq!(focused.rows.len(), 1);
    assert_eq!(focused.rows[0].request_id, focus.to_string());
}

#[tokio::test]
async fn tied_report_receipts_cross_all_pages_without_duplicates_or_lost_history() {
    let (_directory, pool) = fixture().await;
    let id = host(&pool, "report host").await;
    let now = Utc::now();
    let from = now - Duration::days(1);
    let to = now + Duration::days(1);
    let mut expected = HashSet::new();
    for _ in 0..123 {
        let report = Uuid::new_v4();
        expected.insert(report);
        sqlx::query("INSERT INTO client_metric_reports(report_id,host_id,schema_version,collected_at,received_at,interval_seconds) VALUES(?,?,3,?,?,10)").bind(report).bind(id).bind(now).bind(now).execute(&pool).await.unwrap();
    }
    for (instant, included) in [
        (from - Duration::microseconds(1), false),
        (from, true),
        (to - Duration::microseconds(1), true),
        (to, false),
    ] {
        let report = Uuid::new_v4();
        if included {
            expected.insert(report);
        }
        sqlx::query("INSERT INTO client_metric_reports(report_id,host_id,schema_version,collected_at,received_at,interval_seconds) VALUES(?,?,3,?,?,10)")
            .bind(report).bind(id).bind(instant).bind(instant).execute(&pool).await.unwrap();
    }
    let mut actual = HashSet::new();
    let mut cursor = None;
    let mut first = None;
    let mut previous = None;
    loop {
        let page = store::report_logs_page(&pool, id, from, to, cursor, "reports-admin-host-date")
            .await
            .unwrap()
            .unwrap();
        assert!(page.rows.len() <= 50);
        if first.is_none() {
            first = Some(
                page.rows
                    .iter()
                    .map(|row| row.report_id)
                    .collect::<Vec<_>>(),
            )
        } else if previous.is_none() {
            previous = page.previous_cursor.clone()
        };
        for row in page.rows {
            assert!(actual.insert(row.report_id));
        }
        let Some(next) = page.next_cursor else { break };
        assert!(Cursor::decode(Some(&next), "another-date").is_err());
        cursor = Cursor::decode(Some(&next), "reports-admin-host-date").unwrap();
    }
    assert_eq!(actual, expected);
    let cursor = Cursor::decode(previous.as_deref(), "reports-admin-host-date").unwrap();
    let back = store::report_logs_page(&pool, id, from, to, cursor, "reports-admin-host-date")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        back.rows
            .iter()
            .map(|row| row.report_id)
            .collect::<Vec<_>>(),
        first.unwrap()
    );
}
