//! Objective persistence — one JSON document per objective.

use boarddo_shared::v2::Objective;
use sqlx::FromRow;
use uuid::Uuid;

use super::Storage;

#[derive(FromRow)]
struct ObjectiveRow {
    data: String,
}

impl Storage {
    pub async fn migrate_objectives(&self) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS objectives (
                id TEXT PRIMARY KEY NOT NULL,
                title TEXT NOT NULL,
                status TEXT NOT NULL,
                data TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            "#,
        )
        .execute(self.pool())
        .await?;
        Ok(())
    }

    pub async fn list_objectives(&self) -> anyhow::Result<Vec<Objective>> {
        let rows: Vec<ObjectiveRow> =
            sqlx::query_as("SELECT data FROM objectives ORDER BY updated_at DESC")
                .fetch_all(self.pool())
                .await?;
        Ok(rows
            .into_iter()
            .filter_map(|r| match serde_json::from_str::<Objective>(&r.data) {
                Ok(o) => Some(o),
                Err(err) => {
                    tracing::warn!(error = %err, "objective.row_corrupt");
                    None
                }
            })
            .collect())
    }

    pub async fn get_objective(&self, id: Uuid) -> anyhow::Result<Option<Objective>> {
        let row: Option<ObjectiveRow> = sqlx::query_as("SELECT data FROM objectives WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(self.pool())
            .await?;
        row.map(|r| serde_json::from_str(&r.data).map_err(Into::into))
            .transpose()
    }

    /// Insert or replace.
    pub async fn save_objective(&self, o: &Objective) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO objectives (id, title, status, data, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
                title = excluded.title,
                status = excluded.status,
                data = excluded.data,
                updated_at = excluded.updated_at",
        )
        .bind(o.id.to_string())
        .bind(&o.title)
        .bind(o.status.as_str())
        .bind(serde_json::to_string(o)?)
        .bind(o.created_at.to_rfc3339())
        .bind(o.updated_at.to_rfc3339())
        .execute(self.pool())
        .await?;
        Ok(())
    }

    pub async fn delete_objective(&self, id: Uuid) -> anyhow::Result<bool> {
        let res = sqlx::query("DELETE FROM objectives WHERE id = ?")
            .bind(id.to_string())
            .execute(self.pool())
            .await?;
        Ok(res.rows_affected() > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use boarddo_shared::v2::{ObjectiveItem, ObjectiveStatus};

    #[tokio::test]
    async fn roundtrip() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        let storage = Storage::from_pool(pool);
        storage.migrate_objectives().await.unwrap();
        let mut o = Objective::new("Test");
        o.items.push(ObjectiveItem::new("i1", "Step"));
        storage.save_objective(&o).await.unwrap();
        o.status = ObjectiveStatus::Running;
        storage.save_objective(&o).await.unwrap();
        let got = storage.get_objective(o.id).await.unwrap().unwrap();
        assert_eq!(got.status, ObjectiveStatus::Running);
        assert_eq!(got.items.len(), 1);
        assert_eq!(storage.list_objectives().await.unwrap().len(), 1);
        assert!(storage.delete_objective(o.id).await.unwrap());
        assert!(storage.get_objective(o.id).await.unwrap().is_none());
    }
}
