use std::path::{Path, PathBuf};

use tglib::TelegramAccountId;

#[derive(Clone, Debug)]
pub struct SessionPaths {
    pub root: PathBuf,
    pub tdlib: PathBuf,
    pub database: PathBuf,
    pub files: PathBuf,
}

impl SessionPaths {
    pub fn for_account(data_dir: &Path, account_id: TelegramAccountId) -> Self {
        let root = data_dir
            .join("telegram")
            .join("accounts")
            .join(account_id.to_string());
        Self {
            tdlib: root.join("tdlib"),
            database: root.join("database"),
            files: root.join("files"),
            root,
        }
    }

    pub fn ensure(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.tdlib)?;
        std::fs::create_dir_all(&self.database)?;
        std::fs::create_dir_all(&self.files)?;
        Ok(())
    }

    pub fn remove_all(&self) -> std::io::Result<()> {
        if self.root.exists() {
            std::fs::remove_dir_all(&self.root)?;
        }
        Ok(())
    }
}

pub fn session_paths(data_dir: &Path, account_id: TelegramAccountId) -> SessionPaths {
    SessionPaths::for_account(data_dir, account_id)
}
