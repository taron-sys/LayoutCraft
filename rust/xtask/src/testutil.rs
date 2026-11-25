//! Test helper: a scratch directory that removes itself (the tools may not use extra crates, so no `tempfile`).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new() -> TempDir {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!("xtask-test-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&path).unwrap();
        TempDir(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// Writes `contents` to `relative` below the directory, creating the folders on the way.
    pub fn write(&self, relative: &str, contents: &[u8]) {
        let file = self.0.join(relative);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, contents).unwrap();
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
