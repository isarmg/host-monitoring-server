#[path = "../../scripts/server-target-gate.rs"]
mod target_gate;
use std::{collections::BTreeSet, env, fs};

const FOUNDATION_SOURCE: &str = "git+https://github.com/isarmg/xcss.git?rev=";

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
    target_gate::main();
    let product_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("server crate belongs to the product workspace");
    let web_root = env::var_os("XCSS_WEB_DIST")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| product_root.join("web/dist"));
    xcss_web_assets::build::generate(&web_root).expect("generate embedded Web assets");
    println!("cargo:rerun-if-env-changed=XCSS_WEB_DIST");
    let source_revision = env::var("XSOS_SOURCE_REVISION").unwrap_or_else(|_| "unbound".to_owned());
    assert!(
        source_revision == "unbound"
            || (source_revision.len() == 40
                && source_revision
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))),
        "XSOS_SOURCE_REVISION must be a full lowercase 40-hex Git commit"
    );
    println!("cargo:rustc-env=XSOS_BUILD_TARGET={target}");
    println!("cargo:rustc-env=XSOS_SOURCE_REVISION={source_revision}");
    let foundation_revision = locked_foundation_revision(
        &fs::read_to_string(product_root.join("Cargo.lock")).expect("read Cargo.lock"),
    );
    println!("cargo:rustc-env=XCSS_FOUNDATION_REVISION={foundation_revision}");
    println!("cargo:rerun-if-env-changed=XSOS_SOURCE_REVISION");
    println!(
        "cargo:rerun-if-changed={}",
        product_root.join("Cargo.lock").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        product_root.join("release.json").display()
    );
}
