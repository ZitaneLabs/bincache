mod temp_dir;
pub use temp_dir::TempDir;
mod temp_arb_data;
pub use temp_arb_data::create_arb_data;

use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::Mutex,
};

// Exact paths keep injected read/copy failures isolated across parallel tests.
pub static IO_FAILURES: Mutex<Vec<(PathBuf, ErrorKind)>> = Mutex::new(Vec::new());

pub fn check_io(path: &Path, kind: ErrorKind) -> std::io::Result<()> {
    if IO_FAILURES
        .lock()
        .unwrap()
        .iter()
        .any(|(p, k)| p == path && *k == kind)
    {
        return Err(kind.into());
    }
    Ok(())
}
