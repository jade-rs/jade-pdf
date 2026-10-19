#![allow(dead_code)]

use std::{
    env,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("failed to resolve workspace root")
}

pub fn temp_file_path(prefix: &str) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time error")
        .as_nanos();
    env::temp_dir().join(format!("{prefix}_{timestamp}.tmp"))
}
