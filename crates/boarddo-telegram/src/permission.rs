use std::sync::Arc;

use tglib::{
    TelegramAccountId, TelegramAccountPermissions, TelegramActionKind, TelegramError,
};

use crate::store::TelegramSessionStore;

pub struct TelegramPermissionService {
    store: Arc<TelegramSessionStore>,
}

impl TelegramPermissionService {
    pub fn new(store: Arc<TelegramSessionStore>) -> Self {
        Self { store }
    }

    pub async fn get(
        &self,
        account_id: TelegramAccountId,
    ) -> Result<TelegramAccountPermissions, TelegramError> {
        let account = self
            .store
            .get_account(account_id)
            .await
            .map_err(|e| TelegramError::Account(e.to_string()))?
            .ok_or(TelegramError::AccountNotFound)?;
        Ok(account.permissions)
    }

    pub async fn check(
        &self,
        account_id: TelegramAccountId,
        action: TelegramActionKind,
    ) -> Result<(), TelegramError> {
        let permissions = self.get(account_id).await?;
        if permissions.allows(action) {
            Ok(())
        } else {
            Err(TelegramError::PermissionDenied(action.as_str()))
        }
    }

    pub async fn set(
        &self,
        account_id: TelegramAccountId,
        permissions: TelegramAccountPermissions,
    ) -> Result<(), TelegramError> {
        if self
            .store
            .get_account(account_id)
            .await
            .map_err(|e| TelegramError::Account(e.to_string()))?
            .is_none()
        {
            return Err(TelegramError::AccountNotFound);
        }
        self.store
            .upsert_permissions(account_id, &permissions)
            .await
            .map_err(|e| TelegramError::Account(e.to_string()))
    }
}
