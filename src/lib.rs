pub mod cli;
pub mod config;
pub mod hook;
pub mod mux;
pub mod notify;
pub mod paths;
pub mod store;

/// Unix seconds.
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
