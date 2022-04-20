use anyhow::Result;
use log::warn;
use std::fs::File;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub struct TempFile {
    inner: File,
    path: PathBuf,
}

impl TempFile {
    pub fn new() -> Result<TempFile> {
        let mut path = {
            let mut dir = std::env::temp_dir();
            dir.push("simple-ws-server");
            dir
        };
        std::fs::create_dir_all(&path)?;
        let file_name = format!("{}.txt", Uuid::new_v4());
        path.push(file_name);
        let file = File::create(&path)?;
        Ok(TempFile { inner: file, path })
    }

    pub fn get_path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        std::fs::remove_file(&self.path).unwrap_or_else(|err| {
            warn!(
                "Failed to delete a temporary file at {:?}: {:?}",
                &self.path, err
            );
        })
    }
}
