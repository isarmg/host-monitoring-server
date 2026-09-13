pub mod config;
pub mod crypto;
pub mod database_lock;
pub mod database_schema;
pub mod error;
pub mod http;
pub mod model;
mod pairing_admission;
pub mod release_bundle;
pub mod release_contract;
pub mod retention;
pub mod store;
pub mod telemetry;

pub fn token_hash(token: &str) -> String {
    sarmg_admin_auth::token_hash_hex(token)
}
