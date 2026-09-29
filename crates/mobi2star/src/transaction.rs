use lexicon_core::{Error, Result};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Reserve a new owner-only directory. Publish only the complete child `bundle`.
/// No replacement of existing paths; no unsafe rename flags or shell execution.
pub(crate) struct Transaction {
    root: PathBuf,
    stage: Option<tempfile::TempDir>,
    committed: bool,
}
impl Transaction {
    pub fn begin(root: &Path) -> Result<Self> {
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        // create (not create_dir_all) is the atomic exclusive reservation.
        builder.create(root)?;
        let mut tx = Self {
            root: root.to_owned(),
            stage: None,
            committed: false,
        };
        tx.stage = Some(
            tempfile::Builder::new()
                .prefix(".staging-")
                .tempdir_in(root)?,
        );
        Ok(tx)
    }
    pub fn path(&self) -> Result<&Path> {
        self.stage
            .as_ref()
            .map(|s| s.path())
            .ok_or_else(|| Error::Incomplete("transaction has no staging directory".into()))
    }
    pub fn commit(mut self) -> Result<PathBuf> {
        let destination = self.root.join("bundle");
        if destination.symlink_metadata().is_ok() {
            return Err(Error::Incomplete(
                "reserved destination was modified by another process".into(),
            ));
        }
        fs::rename(self.path()?, &destination)?;
        // Drop a guard referring to the old, now nonexistent path.
        self.stage.take();
        sync_directory(&self.root)?;
        let parent = self
            .root
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        sync_directory(parent)?;
        self.committed = true;
        Ok(destination)
    }
}
impl Drop for Transaction {
    fn drop(&mut self) {
        self.stage.take();
        if !self.committed {
            // Only this newly reserved output tree is eligible for rollback.
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}
pub(crate) fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        fs::File::open(path)?.sync_all()?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}
