pub mod cli;
pub mod config;
pub mod dash;
pub mod doctor;
pub mod hook;
pub mod init;
pub mod mux;
pub mod newtask;
pub mod notify;
pub mod paths;
pub mod setup;
pub mod sidebar;
pub mod store;
pub mod task;
pub mod theme;
pub mod ui;

/// Unix seconds.
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
