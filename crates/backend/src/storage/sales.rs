//! Sales persistence — products, deals and the deal event log.
//! Rows are JSON documents plus a few indexed columns for filtering.

use boarddo_shared::v2::{Deal, DealEvent, DealStage, Product};
use sqlx::FromRow;
use uuid::Uuid;

use super::Storage;

#[derive(FromRow)]
struct DataRow {
    data: String,
}

fn decode<T: serde::de::DeserializeOwned>(rows: Vec<DataRow>, what: &'static str) -> Vec<T> {
    rows.into_iter()
        .filter_map(|r| match serde_json::from_str::<T>(&r.data) {
            Ok(v) => Some(v),
            Err(err) => {
                tracing::warn!(error = %err, kind = what, "sales.row_corrupt");
                None
            }
        })
        .collect()
}

/// Filters for `list_deals`; all optional.
#[derive(Debug, Default, Clone)]
pub struct DealFilter {
    pub stage: Option<DealStage>,
    pub product_id: Option<Uuid>,
    pub lead_id: Option<Uuid>,
}

impl Storage {
    pub async fn migrate_sales(&self) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS products (
                id TEXT PRIMARY KEY NOT NULL,
                name TEXT NOT NULL,
                active INTEGER NOT NULL DEFAULT 1,
                data TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS deals (
                id TEXT PRIMARY KEY NOT NULL,
                title TEXT NOT NULL,
                stage TEXT NOT NULL,
                product_id TEXT,
                lead_id TEXT,
                data TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_deals_stage ON deals(stage);
            CREATE INDEX IF NOT EXISTS idx_deals_product ON deals(product_id);
            CREATE INDEX IF NOT EXISTS idx_deals_lead ON deals(lead_id);
            CREATE TABLE IF NOT EXISTS deal_events (
                id TEXT PRIMARY KEY NOT NULL,
                deal_id TEXT NOT NULL,
                data TEXT NOT NULL,
                at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_deal_events_deal ON deal_events(deal_id, at);
            "#,
        )
        .execute(self.pool())
        .await?;
        Ok(())
    }

    // ----- products -------------------------------------------------------

    pub async fn list_products(&self, include_inactive: bool) -> anyhow::Result<Vec<Product>> {
        let sql = if include_inactive {
            "SELECT data FROM products ORDER BY name COLLATE NOCASE"
        } else {
            "SELECT data FROM products WHERE active = 1 ORDER BY name COLLATE NOCASE"
        };
        let rows: Vec<DataRow> = sqlx::query_as(sql).fetch_all(self.pool()).await?;
        Ok(decode(rows, "product"))
    }

    pub async fn get_product(&self, id: Uuid) -> anyhow::Result<Option<Product>> {
        let row: Option<DataRow> = sqlx::query_as("SELECT data FROM products WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(self.pool())
            .await?;
        row.map(|r| serde_json::from_str(&r.data).map_err(Into::into))
            .transpose()
    }

    /// Insert or replace.
    pub async fn save_product(&self, p: &Product) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO products (id, name, active, data, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                active = excluded.active,
                data = excluded.data,
                updated_at = excluded.updated_at",
        )
        .bind(p.id.to_string())
        .bind(&p.name)
        .bind(p.active)
        .bind(serde_json::to_string(p)?)
        .bind(p.created_at.to_rfc3339())
        .bind(p.updated_at.to_rfc3339())
        .execute(self.pool())
        .await?;
        Ok(())
    }

    /// Hard delete; deals keep their `product_id` (shown as unknown product).
    pub async fn delete_product(&self, id: Uuid) -> anyhow::Result<bool> {
        let res = sqlx::query("DELETE FROM products WHERE id = ?")
            .bind(id.to_string())
            .execute(self.pool())
            .await?;
        Ok(res.rows_affected() > 0)
    }

    pub async fn count_deals_for_product(&self, id: Uuid) -> anyhow::Result<i64> {
        let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM deals WHERE product_id = ?")
            .bind(id.to_string())
            .fetch_one(self.pool())
            .await?;
        Ok(n)
    }

    // ----- deals ----------------------------------------------------------

    pub async fn list_deals(&self, filter: &DealFilter) -> anyhow::Result<Vec<Deal>> {
        let mut sql = String::from("SELECT data FROM deals WHERE 1 = 1");
        if filter.stage.is_some() {
            sql.push_str(" AND stage = ?");
        }
        if filter.product_id.is_some() {
            sql.push_str(" AND product_id = ?");
        }
        if filter.lead_id.is_some() {
            sql.push_str(" AND lead_id = ?");
        }
        sql.push_str(" ORDER BY updated_at DESC");
        let mut q = sqlx::query_as::<_, DataRow>(&sql);
        if let Some(s) = filter.stage {
            q = q.bind(s.as_str());
        }
        if let Some(p) = filter.product_id {
            q = q.bind(p.to_string());
        }
        if let Some(l) = filter.lead_id {
            q = q.bind(l.to_string());
        }
        let rows = q.fetch_all(self.pool()).await?;
        Ok(decode(rows, "deal"))
    }

    pub async fn get_deal(&self, id: Uuid) -> anyhow::Result<Option<Deal>> {
        let row: Option<DataRow> = sqlx::query_as("SELECT data FROM deals WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(self.pool())
            .await?;
        row.map(|r| serde_json::from_str(&r.data).map_err(Into::into))
            .transpose()
    }

    /// Insert or replace.
    pub async fn save_deal(&self, d: &Deal) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO deals (id, title, stage, product_id, lead_id, data, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
                title = excluded.title,
                stage = excluded.stage,
                product_id = excluded.product_id,
                lead_id = excluded.lead_id,
                data = excluded.data,
                updated_at = excluded.updated_at",
        )
        .bind(d.id.to_string())
        .bind(&d.title)
        .bind(d.stage.as_str())
        .bind(d.product_id.map(|v| v.to_string()))
        .bind(d.lead_id.map(|v| v.to_string()))
        .bind(serde_json::to_string(d)?)
        .bind(d.created_at.to_rfc3339())
        .bind(d.updated_at.to_rfc3339())
        .execute(self.pool())
        .await?;
        Ok(())
    }

    /// Deletes the deal and its event log.
    pub async fn delete_deal(&self, id: Uuid) -> anyhow::Result<bool> {
        let mut tx = self.pool().begin().await?;
        sqlx::query("DELETE FROM deal_events WHERE deal_id = ?")
            .bind(id.to_string())
            .execute(&mut *tx)
            .await?;
        let res = sqlx::query("DELETE FROM deals WHERE id = ?")
            .bind(id.to_string())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(res.rows_affected() > 0)
    }

    // ----- events ---------------------------------------------------------

    pub async fn insert_deal_event(&self, ev: &DealEvent) -> anyhow::Result<()> {
        sqlx::query("INSERT INTO deal_events (id, deal_id, data, at) VALUES (?, ?, ?, ?)")
            .bind(ev.id.to_string())
            .bind(ev.deal_id.to_string())
            .bind(serde_json::to_string(ev)?)
            .bind(ev.at.to_rfc3339())
            .execute(self.pool())
            .await?;
        Ok(())
    }

    /// Oldest first.
    pub async fn list_deal_events(&self, deal_id: Uuid) -> anyhow::Result<Vec<DealEvent>> {
        let rows: Vec<DataRow> =
            sqlx::query_as("SELECT data FROM deal_events WHERE deal_id = ? ORDER BY at ASC")
                .bind(deal_id.to_string())
                .fetch_all(self.pool())
                .await?;
        Ok(decode(rows, "deal_event"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use boarddo_shared::v2::{CreateDealRequest, ProductInput};

    async fn storage() -> Storage {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        let storage = Storage::from_pool(pool);
        storage.migrate_sales().await.unwrap();
        storage
    }

    fn product(name: &str) -> Product {
        Product::new(ProductInput {
            name: name.into(),
            price: 100.0,
            cost: 40.0,
            ..Default::default()
        })
        .unwrap()
    }

    #[tokio::test]
    async fn product_roundtrip_and_inactive_filter() {
        let s = storage().await;
        let mut a = product("Альфа");
        let b = product("beta");
        s.save_product(&a).await.unwrap();
        s.save_product(&b).await.unwrap();
        assert_eq!(s.list_products(false).await.unwrap().len(), 2);

        a.active = false;
        s.save_product(&a).await.unwrap();
        assert_eq!(s.list_products(false).await.unwrap().len(), 1);
        assert_eq!(s.list_products(true).await.unwrap().len(), 2);
        assert_eq!(s.get_product(a.id).await.unwrap().unwrap(), a);

        assert!(s.delete_product(b.id).await.unwrap());
        assert!(!s.delete_product(b.id).await.unwrap());
        assert!(s.get_product(b.id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn deal_roundtrip_filters_and_events() {
        let s = storage().await;
        let p = product("Курс");
        s.save_product(&p).await.unwrap();
        let lead_id = Uuid::new_v4();

        let mut d1 = Deal::new(
            CreateDealRequest {
                product_id: Some(p.id),
                lead_id: Some(lead_id),
                ..Default::default()
            },
            Some(&p),
        )
        .unwrap();
        let d2 = Deal::new(
            CreateDealRequest {
                title: "Ручная".into(),
                amount: Some(50.0),
                ..Default::default()
            },
            None,
        )
        .unwrap();
        s.save_deal(&d1).await.unwrap();
        s.save_deal(&d2).await.unwrap();
        assert_eq!(s.count_deals_for_product(p.id).await.unwrap(), 1);

        let ev = d1.change_stage(DealStage::Won, Some("ok".into())).unwrap();
        s.save_deal(&d1).await.unwrap();
        s.insert_deal_event(&ev).await.unwrap();

        let all = s.list_deals(&DealFilter::default()).await.unwrap();
        assert_eq!(all.len(), 2);
        let won = s
            .list_deals(&DealFilter {
                stage: Some(DealStage::Won),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(won, vec![d1.clone()]);
        let by_lead = s
            .list_deals(&DealFilter {
                lead_id: Some(lead_id),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(by_lead.len(), 1);
        let by_product = s
            .list_deals(&DealFilter {
                product_id: Some(p.id),
                stage: Some(DealStage::New),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(by_product.is_empty());

        let events = s.list_deal_events(d1.id).await.unwrap();
        assert_eq!(events, vec![ev]);

        assert!(s.delete_deal(d1.id).await.unwrap());
        assert!(s.get_deal(d1.id).await.unwrap().is_none());
        assert!(s.list_deal_events(d1.id).await.unwrap().is_empty());
    }
}
