//! Sales core: products, deals (pipeline) and aggregated sales/interest metrics.
//!
//! Leads come from the marketing loop (`Lead`); a lead can be converted into a
//! `Deal` for a `Product`. Winning a deal books revenue and gross profit.

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::marketing::{Lead, LeadStage};

pub const DEFAULT_CURRENCY: &str = "RUB";

fn default_currency() -> String {
    DEFAULT_CURRENCY.into()
}

fn default_true() -> bool {
    true
}

fn default_quantity() -> u32 {
    1
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Product {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Unit sale price.
    pub price: f64,
    /// Unit cost (COGS) — used for gross profit.
    #[serde(default)]
    pub cost: f64,
    #[serde(default = "default_currency")]
    pub currency: String,
    #[serde(default = "default_true")]
    pub active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProductInput {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub price: f64,
    #[serde(default)]
    pub cost: f64,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub active: Option<bool>,
}

impl ProductInput {
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("product name is required".into());
        }
        if !self.price.is_finite() || self.price < 0.0 {
            return Err("price must be >= 0".into());
        }
        if !self.cost.is_finite() || self.cost < 0.0 {
            return Err("cost must be >= 0".into());
        }
        Ok(())
    }
}

impl Product {
    pub fn new(input: ProductInput) -> Result<Self, String> {
        input.validate()?;
        let now = Utc::now();
        Ok(Self {
            id: Uuid::new_v4(),
            name: input.name.trim().to_string(),
            description: input.description,
            price: input.price,
            cost: input.cost,
            currency: input.currency.unwrap_or_else(default_currency),
            active: input.active.unwrap_or(true),
            created_at: now,
            updated_at: now,
        })
    }

    pub fn apply(&mut self, input: ProductInput) -> Result<(), String> {
        input.validate()?;
        self.name = input.name.trim().to_string();
        self.description = input.description;
        self.price = input.price;
        self.cost = input.cost;
        if let Some(c) = input.currency {
            self.currency = c;
        }
        if let Some(a) = input.active {
            self.active = a;
        }
        self.updated_at = Utc::now();
        Ok(())
    }

    pub fn unit_margin(&self) -> f64 {
        self.price - self.cost
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DealStage {
    New,
    Contacted,
    Proposal,
    Negotiation,
    Won,
    Lost,
}

impl DealStage {
    pub fn all() -> [Self; 6] {
        [
            Self::New,
            Self::Contacted,
            Self::Proposal,
            Self::Negotiation,
            Self::Won,
            Self::Lost,
        ]
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Contacted => "contacted",
            Self::Proposal => "proposal",
            Self::Negotiation => "negotiation",
            Self::Won => "won",
            Self::Lost => "lost",
        }
    }

    pub fn is_closed(self) -> bool {
        matches!(self, Self::Won | Self::Lost)
    }

    /// Lead stage implied by a deal stage (deals push leads forward, never back).
    pub fn lead_stage(self) -> LeadStage {
        match self {
            Self::Won => LeadStage::Converted,
            Self::Lost => LeadStage::Lost,
            _ => LeadStage::Qualified,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Deal {
    pub id: Uuid,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub product_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead_id: Option<Uuid>,
    /// Free-form contact (name, @username, phone, email).
    #[serde(default)]
    pub contact: String,
    /// Acquisition channel: telegram, site, referral, manual…
    #[serde(default)]
    pub source: String,
    pub stage: DealStage,
    #[serde(default = "default_quantity")]
    pub quantity: u32,
    /// Total deal amount (not unit price).
    pub amount: f64,
    #[serde(default = "default_currency")]
    pub currency: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lost_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CreateDealRequest {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub product_id: Option<Uuid>,
    #[serde(default)]
    pub lead_id: Option<Uuid>,
    #[serde(default)]
    pub contact: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub stage: Option<DealStage>,
    #[serde(default)]
    pub quantity: Option<u32>,
    /// When omitted, computed as `product.price * quantity`.
    #[serde(default)]
    pub amount: Option<f64>,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub notes: String,
}

/// Partial update — only provided fields change. Stage changes go through
/// `ChangeDealStageRequest` so they are logged as events.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateDealRequest {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub product_id: Option<Uuid>,
    #[serde(default)]
    pub contact: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub quantity: Option<u32>,
    #[serde(default)]
    pub amount: Option<f64>,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeDealStageRequest {
    pub stage: DealStage,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DealEvent {
    pub id: Uuid,
    pub deal_id: Uuid,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_stage: Option<DealStage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_stage: Option<DealStage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub at: DateTime<Utc>,
}

impl DealEvent {
    pub fn new(deal_id: Uuid, kind: &str) -> Self {
        Self {
            id: Uuid::new_v4(),
            deal_id,
            kind: kind.into(),
            from_stage: None,
            to_stage: None,
            note: None,
            at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DealDetails {
    pub deal: Deal,
    pub events: Vec<DealEvent>,
}

fn validate_amount(v: f64) -> Result<(), String> {
    if !v.is_finite() || v < 0.0 {
        return Err("amount must be >= 0".into());
    }
    Ok(())
}

impl Deal {
    /// Build a deal; `product` resolves default amount/currency/title.
    pub fn new(req: CreateDealRequest, product: Option<&Product>) -> Result<Self, String> {
        if req.product_id.is_some() && product.is_none() {
            return Err("product not found".into());
        }
        let quantity = req.quantity.unwrap_or(1).max(1);
        let amount = match req.amount {
            Some(a) => a,
            None => product.map(|p| p.price * quantity as f64).unwrap_or(0.0),
        };
        validate_amount(amount)?;
        let title = if req.title.trim().is_empty() {
            match (product, req.contact.trim()) {
                (Some(p), "") => p.name.clone(),
                (Some(p), c) => format!("{} — {}", p.name, c),
                (None, "") => return Err("deal title is required".into()),
                (None, c) => c.to_string(),
            }
        } else {
            req.title.trim().to_string()
        };
        let stage = req.stage.unwrap_or(DealStage::New);
        let now = Utc::now();
        Ok(Self {
            id: Uuid::new_v4(),
            title,
            product_id: req.product_id,
            lead_id: req.lead_id,
            contact: req.contact,
            source: if req.source.trim().is_empty() {
                "manual".into()
            } else {
                req.source
            },
            stage,
            quantity,
            amount,
            currency: req
                .currency
                .or_else(|| product.map(|p| p.currency.clone()))
                .unwrap_or_else(default_currency),
            notes: req.notes,
            lost_reason: None,
            created_at: now,
            updated_at: now,
            closed_at: stage.is_closed().then_some(now),
        })
    }

    pub fn apply(&mut self, req: UpdateDealRequest) -> Result<(), String> {
        if let Some(a) = req.amount {
            validate_amount(a)?;
            self.amount = a;
        }
        if let Some(t) = req.title {
            if t.trim().is_empty() {
                return Err("deal title is required".into());
            }
            self.title = t.trim().to_string();
        }
        if let Some(p) = req.product_id {
            self.product_id = Some(p);
        }
        if let Some(c) = req.contact {
            self.contact = c;
        }
        if let Some(s) = req.source {
            self.source = s;
        }
        if let Some(q) = req.quantity {
            self.quantity = q.max(1);
        }
        if let Some(c) = req.currency {
            self.currency = c;
        }
        if let Some(n) = req.notes {
            self.notes = n;
        }
        self.updated_at = Utc::now();
        Ok(())
    }

    /// Move to a new stage; returns the event to persist (None if unchanged).
    pub fn change_stage(&mut self, stage: DealStage, note: Option<String>) -> Option<DealEvent> {
        if self.stage == stage {
            return None;
        }
        let now = Utc::now();
        let mut ev = DealEvent::new(self.id, "stage_changed");
        ev.from_stage = Some(self.stage);
        ev.to_stage = Some(stage);
        ev.note = note.clone().filter(|n| !n.trim().is_empty());
        ev.at = now;
        self.stage = stage;
        self.closed_at = stage.is_closed().then_some(now);
        self.lost_reason = if stage == DealStage::Lost { ev.note.clone() } else { None };
        self.updated_at = now;
        Some(ev)
    }

    /// Gross profit for a won deal (amount − unit cost × qty); 0 otherwise.
    pub fn gross_profit(&self, product: Option<&Product>) -> f64 {
        if self.stage != DealStage::Won {
            return 0.0;
        }
        self.amount - product.map(|p| p.cost * self.quantity as f64).unwrap_or(0.0)
    }
}

/// Promote a lead according to a deal stage without demoting it
/// (a Converted lead stays Converted even if a second deal is lost).
pub fn promote_lead_stage(current: LeadStage, deal_stage: DealStage) -> LeadStage {
    fn rank(s: LeadStage) -> u8 {
        match s {
            LeadStage::Cold => 0,
            LeadStage::Lost => 1,
            LeadStage::Interested => 2,
            LeadStage::Qualified => 3,
            LeadStage::Converted => 4,
        }
    }
    let target = deal_stage.lead_stage();
    if target == LeadStage::Lost {
        // Losing a deal only marks leads that never went further.
        return if matches!(current, LeadStage::Converted) { current } else { LeadStage::Lost };
    }
    if rank(target) > rank(current) { target } else { current }
}

// ---------------------------------------------------------------------------
// Aggregated metrics
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SalesTotals {
    pub deals: u32,
    pub open: u32,
    pub won: u32,
    pub lost: u32,
    pub revenue: f64,
    pub gross_profit: f64,
    /// Sum of open deal amounts.
    pub pipeline_value: f64,
    pub avg_deal: f64,
    /// won / (won + lost), 0..1.
    pub win_rate: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LeadInterest {
    pub total: u32,
    pub interested: u32,
    pub qualified: u32,
    pub converted: u32,
    pub lost: u32,
    /// Leads that have at least one deal / total leads, 0..1.
    pub lead_to_deal: f64,
    /// Converted / total leads, 0..1.
    pub lead_to_sale: f64,
    pub by_source: Vec<NamedCount>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NamedCount {
    pub name: String,
    pub count: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProductSales {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub product_id: Option<Uuid>,
    pub name: String,
    /// All deals for the product = interest signal.
    pub deals: u32,
    pub open: u32,
    pub won: u32,
    pub lost: u32,
    pub units: u32,
    pub revenue: f64,
    pub gross_profit: f64,
    pub pipeline_value: f64,
    pub win_rate: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SourceSales {
    pub source: String,
    pub deals: u32,
    pub won: u32,
    pub revenue: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StageStat {
    pub stage: Option<DealStage>,
    pub count: u32,
    pub amount: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DailyPoint {
    pub date: NaiveDate,
    pub new_deals: u32,
    pub won: u32,
    pub revenue: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SalesSummary {
    pub days: u32,
    pub currency: String,
    pub totals: SalesTotals,
    pub leads: LeadInterest,
    pub by_product: Vec<ProductSales>,
    pub by_source: Vec<SourceSales>,
    pub by_stage: Vec<StageStat>,
    pub daily: Vec<DailyPoint>,
}

fn ratio(a: u32, b: u32) -> f64 {
    if b == 0 { 0.0 } else { a as f64 / b as f64 }
}

/// Aggregate sales + interest. Totals cover all deals; `daily` covers the last
/// `days` days ending at `now` (revenue is booked on `closed_at`).
/// Simulated leads are excluded.
pub fn compute_sales_summary(
    products: &[Product],
    deals: &[Deal],
    leads: &[Lead],
    days: u32,
    now: DateTime<Utc>,
) -> SalesSummary {
    let days = days.clamp(1, 366);
    let product_by_id: BTreeMap<Uuid, &Product> = products.iter().map(|p| (p.id, p)).collect();

    let mut totals = SalesTotals::default();
    let mut per_product: BTreeMap<Option<Uuid>, ProductSales> = BTreeMap::new();
    let mut per_source: BTreeMap<String, SourceSales> = BTreeMap::new();
    let mut per_stage: BTreeMap<DealStage, StageStat> = BTreeMap::new();

    for p in products {
        per_product.insert(
            Some(p.id),
            ProductSales {
                product_id: Some(p.id),
                name: p.name.clone(),
                ..Default::default()
            },
        );
    }

    let today = now.date_naive();
    let start = today - Duration::days(days as i64 - 1);
    let mut daily: BTreeMap<NaiveDate, DailyPoint> = (0..days)
        .map(|i| {
            let d = start + Duration::days(i as i64);
            (d, DailyPoint { date: d, ..Default::default() })
        })
        .collect();

    for d in deals {
        let product = d.product_id.and_then(|id| product_by_id.get(&id).copied());
        let gp = d.gross_profit(product);
        totals.deals += 1;

        let key = if product.is_some() { d.product_id } else { None };
        let ps = per_product.entry(key).or_insert_with(|| ProductSales {
            product_id: None,
            name: "Без продукта".into(),
            ..Default::default()
        });
        ps.deals += 1;

        let source = if d.source.trim().is_empty() { "manual".to_string() } else { d.source.clone() };
        let ss = per_source.entry(source.clone()).or_insert_with(|| SourceSales {
            source,
            ..Default::default()
        });
        ss.deals += 1;

        let st = per_stage.entry(d.stage).or_insert_with(|| StageStat {
            stage: Some(d.stage),
            ..Default::default()
        });
        st.count += 1;
        st.amount += d.amount;

        match d.stage {
            DealStage::Won => {
                totals.won += 1;
                totals.revenue += d.amount;
                totals.gross_profit += gp;
                ps.won += 1;
                ps.units += d.quantity;
                ps.revenue += d.amount;
                ps.gross_profit += gp;
                ss.won += 1;
                ss.revenue += d.amount;
            }
            DealStage::Lost => {
                totals.lost += 1;
                ps.lost += 1;
            }
            _ => {
                totals.open += 1;
                totals.pipeline_value += d.amount;
                ps.open += 1;
                ps.pipeline_value += d.amount;
            }
        }

        if let Some(p) = daily.get_mut(&d.created_at.date_naive()) {
            p.new_deals += 1;
        }
        if d.stage == DealStage::Won {
            let closed = d.closed_at.unwrap_or(d.updated_at).date_naive();
            if let Some(p) = daily.get_mut(&closed) {
                p.won += 1;
                p.revenue += d.amount;
            }
        }
    }

    totals.avg_deal = if totals.won == 0 { 0.0 } else { totals.revenue / totals.won as f64 };
    totals.win_rate = ratio(totals.won, totals.won + totals.lost);

    let mut by_product: Vec<ProductSales> = per_product
        .into_values()
        .map(|mut p| {
            p.win_rate = ratio(p.won, p.won + p.lost);
            p
        })
        .filter(|p| p.product_id.is_some() || p.deals > 0)
        .collect();
    by_product.sort_by(|a, b| {
        b.revenue
            .partial_cmp(&a.revenue)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(b.deals.cmp(&a.deals))
            .then(a.name.cmp(&b.name))
    });

    let mut by_source: Vec<SourceSales> = per_source.into_values().collect();
    by_source.sort_by(|a, b| b.deals.cmp(&a.deals).then(a.source.cmp(&b.source)));

    let by_stage = DealStage::all()
        .into_iter()
        .map(|s| {
            per_stage.remove(&s).unwrap_or(StageStat {
                stage: Some(s),
                ..Default::default()
            })
        })
        .collect();

    // Lead interest (production leads only).
    let real: Vec<&Lead> = leads.iter().filter(|l| !l.simulated).collect();
    let leads_with_deal: std::collections::BTreeSet<Uuid> =
        deals.iter().filter_map(|d| d.lead_id).collect();
    let mut lead_sources: BTreeMap<String, u32> = BTreeMap::new();
    let mut li = LeadInterest {
        total: real.len() as u32,
        ..Default::default()
    };
    let mut with_deal = 0u32;
    for l in &real {
        match l.stage {
            LeadStage::Interested => li.interested += 1,
            LeadStage::Qualified => {
                li.interested += 1;
                li.qualified += 1;
            }
            LeadStage::Converted => {
                li.interested += 1;
                li.qualified += 1;
                li.converted += 1;
            }
            LeadStage::Lost => li.lost += 1,
            LeadStage::Cold => {}
        }
        if leads_with_deal.contains(&l.id) {
            with_deal += 1;
        }
        *lead_sources.entry(l.source.clone()).or_default() += 1;
    }
    li.lead_to_deal = ratio(with_deal, li.total);
    li.lead_to_sale = ratio(li.converted, li.total);
    li.by_source = lead_sources
        .into_iter()
        .map(|(name, count)| NamedCount { name, count })
        .collect();
    li.by_source.sort_by(|a, b| b.count.cmp(&a.count).then(a.name.cmp(&b.name)));

    let currency = deals
        .first()
        .map(|d| d.currency.clone())
        .or_else(|| products.first().map(|p| p.currency.clone()))
        .unwrap_or_else(default_currency);

    SalesSummary {
        days,
        currency,
        totals,
        leads: li,
        by_product,
        by_source,
        by_stage,
        daily: daily.into_values().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Map;

    fn product(name: &str, price: f64, cost: f64) -> Product {
        Product::new(ProductInput {
            name: name.into(),
            price,
            cost,
            ..Default::default()
        })
        .unwrap()
    }

    fn lead(stage: LeadStage, simulated: bool) -> Lead {
        let now = Utc::now();
        Lead {
            id: Uuid::new_v4(),
            goal_id: Uuid::new_v4(),
            run_id: Uuid::new_v4(),
            campaign_id: None,
            experiment_id: None,
            source: "telegram".into(),
            source_reference: None,
            chat_id: None,
            account_id: None,
            stage,
            score: 0,
            score_reasons: vec![],
            first_seen_at: now,
            last_activity_at: now,
            metadata: Map::new(),
            simulated,
        }
    }

    #[test]
    fn product_validation() {
        assert!(Product::new(ProductInput { name: " ".into(), ..Default::default() }).is_err());
        assert!(
            Product::new(ProductInput { name: "X".into(), price: -1.0, ..Default::default() })
                .is_err()
        );
        let p = product("Курс", 1000.0, 300.0);
        assert_eq!(p.unit_margin(), 700.0);
        assert_eq!(p.currency, DEFAULT_CURRENCY);
    }

    #[test]
    fn deal_defaults_from_product() {
        let p = product("Курс", 1000.0, 300.0);
        let d = Deal::new(
            CreateDealRequest {
                product_id: Some(p.id),
                contact: "@ivan".into(),
                quantity: Some(3),
                ..Default::default()
            },
            Some(&p),
        )
        .unwrap();
        assert_eq!(d.amount, 3000.0);
        assert_eq!(d.title, "Курс — @ivan");
        assert_eq!(d.source, "manual");
        assert_eq!(d.stage, DealStage::New);
        assert!(d.closed_at.is_none());

        // product_id without resolved product → error
        assert!(
            Deal::new(CreateDealRequest { product_id: Some(p.id), ..Default::default() }, None)
                .is_err()
        );
        // no title, no product, no contact → error
        assert!(Deal::new(CreateDealRequest::default(), None).is_err());
    }

    #[test]
    fn stage_change_logs_event_and_closes() {
        let p = product("Курс", 1000.0, 300.0);
        let mut d = Deal::new(
            CreateDealRequest { product_id: Some(p.id), ..Default::default() },
            Some(&p),
        )
        .unwrap();
        assert!(d.change_stage(DealStage::New, None).is_none());
        let ev = d.change_stage(DealStage::Won, Some("оплачено".into())).unwrap();
        assert_eq!(ev.from_stage, Some(DealStage::New));
        assert_eq!(ev.to_stage, Some(DealStage::Won));
        assert!(d.closed_at.is_some());
        assert_eq!(d.gross_profit(Some(&p)), 700.0);

        d.change_stage(DealStage::Lost, Some("дорого".into()));
        assert_eq!(d.lost_reason.as_deref(), Some("дорого"));
        assert_eq!(d.gross_profit(Some(&p)), 0.0);
        d.change_stage(DealStage::Proposal, None);
        assert!(d.closed_at.is_none());
        assert!(d.lost_reason.is_none());
    }

    #[test]
    fn lead_promotion_never_demotes() {
        assert_eq!(promote_lead_stage(LeadStage::Cold, DealStage::New), LeadStage::Qualified);
        assert_eq!(promote_lead_stage(LeadStage::Qualified, DealStage::Won), LeadStage::Converted);
        assert_eq!(
            promote_lead_stage(LeadStage::Converted, DealStage::Lost),
            LeadStage::Converted
        );
        assert_eq!(
            promote_lead_stage(LeadStage::Converted, DealStage::Proposal),
            LeadStage::Converted
        );
        assert_eq!(promote_lead_stage(LeadStage::Interested, DealStage::Lost), LeadStage::Lost);
    }

    #[test]
    fn summary_aggregates() {
        let a = product("A", 1000.0, 400.0);
        let b = product("B", 500.0, 0.0);
        let l1 = lead(LeadStage::Converted, false);
        let l2 = lead(LeadStage::Interested, false);
        let sim = lead(LeadStage::Converted, true);

        let mk = |p: &Product, stage: DealStage, source: &str, lead_id: Option<Uuid>| {
            let mut d = Deal::new(
                CreateDealRequest {
                    product_id: Some(p.id),
                    source: source.into(),
                    lead_id,
                    ..Default::default()
                },
                Some(p),
            )
            .unwrap();
            d.change_stage(stage, None);
            d
        };
        let deals = vec![
            mk(&a, DealStage::Won, "telegram", Some(l1.id)),
            mk(&a, DealStage::Won, "telegram", None),
            mk(&a, DealStage::Lost, "site", None),
            mk(&b, DealStage::Proposal, "site", None),
        ];
        let s = compute_sales_summary(&[a.clone(), b.clone()], &deals, &[l1, l2, sim], 7, Utc::now());

        assert_eq!(s.totals.deals, 4);
        assert_eq!(s.totals.won, 2);
        assert_eq!(s.totals.lost, 1);
        assert_eq!(s.totals.open, 1);
        assert_eq!(s.totals.revenue, 2000.0);
        assert_eq!(s.totals.gross_profit, 1200.0);
        assert_eq!(s.totals.pipeline_value, 500.0);
        assert_eq!(s.totals.avg_deal, 1000.0);
        assert!((s.totals.win_rate - 2.0 / 3.0).abs() < 1e-9);

        assert_eq!(s.by_product[0].name, "A");
        assert_eq!(s.by_product[0].units, 2);
        assert_eq!(s.by_product[1].open, 1);
        assert_eq!(s.by_source[0].deals, 2);
        assert_eq!(s.by_stage.len(), 6);

        assert_eq!(s.daily.len(), 7);
        let today = s.daily.last().unwrap();
        assert_eq!(today.new_deals, 4);
        assert_eq!(today.won, 2);
        assert_eq!(today.revenue, 2000.0);

        assert_eq!(s.leads.total, 2); // simulated excluded
        assert_eq!(s.leads.converted, 1);
        assert_eq!(s.leads.interested, 2);
        assert_eq!(s.leads.lead_to_deal, 0.5);
        assert_eq!(s.leads.lead_to_sale, 0.5);
    }

    #[test]
    fn empty_summary() {
        let s = compute_sales_summary(&[], &[], &[], 0, Utc::now());
        assert_eq!(s.days, 1);
        assert_eq!(s.totals, SalesTotals::default());
        assert!(s.by_product.is_empty());
    }
}
