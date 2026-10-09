//! The browser client and its immutable inventory are compiled into this binary.
include!(concat!(env!("OUT_DIR"), "/xcss-web-assets.rs"));

pub fn response(
    directory: Option<&xcss_web_assets::DirectoryAssets>,
    path: &str,
    method: &axum::http::Method,
    headers: &axum::http::HeaderMap,
) -> axum::response::Response {
    match directory {
        Some(directory) => directory.response(path, method, headers),
        None => xcss_web_assets::response(ASSETS, path, method, headers),
    }
    .map(axum::body::Body::from)
}

/// Check the final executable bytes against its compiled inventory.
pub fn verify() -> anyhow::Result<()> {
    xcss_web_assets::verify_embedded(ASSETS, MANIFEST, DIGEST)
}
