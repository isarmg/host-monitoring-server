use std::{collections::BTreeSet, env, fs};

const SUPPORTED_SERVER_TARGET: &str = "x86_64-unknown-linux-gnu";
const FOUNDATION_SOURCE: &str = "git+https://github.com/isarmg/sarmg-foundation-server.git?rev=";

fn locked_foundation_revision(lockfile: &str) -> String {
    let revisions = lockfile
        .lines()
        .map(str::trim)
        .filter_map(|line| line.strip_prefix("source = \"")?.strip_suffix('"'))
        .filter_map(|source| source.strip_prefix(FOUNDATION_SOURCE))
        .map(|source| source.split_once('#').expect("locked Foundation source"))
        .map(|(requested, locked)| {
            assert_eq!(requested, locked, "Foundation revision must be immutable");
            assert!(
                locked.len() == 40 && locked.bytes().all(|byte| byte.is_ascii_hexdigit()),
                "Foundation revision must be full hexadecimal"
            );
            locked.to_owned()
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        revisions.len(),
        1,
        "all Foundation crates must share one revision"
    );
    revisions
        .into_iter()
        .next()
        .expect("one Foundation revision")
}

fn main() {
    let target = env::var("TARGET").expect("Cargo must provide TARGET");
    assert_eq!(
        target, SUPPORTED_SERVER_TARGET,
        "Host Monitoring Server only supports the {SUPPORTED_SERVER_TARGET} compilation target"
    );
    let source_revision =
        env::var("HOST_MONITORING_SOURCE_REVISION").unwrap_or_else(|_| "unbound".to_owned());
    assert!(
        source_revision == "unbound"
            || (source_revision.len() == 40
                && source_revision
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))),
        "HOST_MONITORING_SOURCE_REVISION must be a full lowercase 40-hex Git commit"
    );
    println!("cargo:rustc-env=HOST_MONITORING_BUILD_TARGET={target}");
    println!("cargo:rustc-env=HOST_MONITORING_SOURCE_REVISION={source_revision}");
    let foundation_revision =
        locked_foundation_revision(&fs::read_to_string("../Cargo.lock").expect("read Cargo.lock"));
    println!("cargo:rustc-env=SARMG_FOUNDATION_REVISION={foundation_revision}");
    println!("cargo:rerun-if-env-changed=HOST_MONITORING_SOURCE_REVISION");
    println!("cargo:rerun-if-changed=../Cargo.lock");
    println!("cargo:rerun-if-changed=release.json");
}
