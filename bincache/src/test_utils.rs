use std::{
    ops::Deref,
    path::{Path, PathBuf},
};
use uuid::Uuid;

pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    pub fn new() -> Self {
        let uuid = Uuid::new_v4();
        let path = std::env::temp_dir().join(format!("bincache_{uuid}"));
        std::fs::create_dir_all(&path).unwrap();
        Self { path }
    }
}

impl AsRef<Path> for TempDir {
    fn as_ref(&self) -> &Path {
        self.path.as_ref()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.path).unwrap();
    }
}
