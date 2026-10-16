//! Sales API — products, deals pipeline, lead → deal conversion and the
//! aggregated sales / interest summary.

use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use boarddo_shared::v2::{
    ChangeDealStageRequest, CreateDealRequest, Deal, DealDetails, DealEvent, DealStage, Lead,
    Product, ProductInput, SalesSummary, UpdateDealRequest, compute_sales_summary,
    promote_lead_stage,
};
use chrono::Utc;
use serde::Deserialize;
use uuid::Uuid;

use crate::state::SharedState;
use crate::storage::DealFilter;

type ApiError = (StatusCode, String);
type ApiResult<T> = Result<Json<T>, ApiError>;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/products", get(list_products).post(create_product))
        .route(
            "/products/{id}",
            get(get_product).put(update_product).delete(delete_product),
        )
        .route("/deals", get(list_deals).post(create_deal))
        .route("/deals/{id}", get(get_deal).patch(update_deal).delete(delete_deal))
        .route("/deals/{id}/stage", post(change_stage))
        .route("/deals/from-lead/{lead_id}", post(deal_from_lead))
        .route("/sales/leads", get(list_leads))
        .route("/sales/summary", get(summary))
}

fn internal(e: anyhow::Error) -> ApiError {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn bad(err: String) -> ApiError {
    if err.contains("not found") {
        (StatusCode::NOT_FOUND, err)
    } else {
        (StatusCode::BAD_REQUEST, err)
    }
}

async fn load_product(state: &SharedState, id: Uuid) -> Result<Product, ApiError> {
    state
        .storage
        .get_product(id)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "product not found".into()))
}

async fn load_deal(state: &SharedState, id: Uuid) -> Result<Deal, ApiError> {
    state
        .storage
        .get_deal(id)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "deal not found".into()))
}

async fn optional_product(
    state: &SharedState,
    id: Option<Uuid>,
) -> Result<Option<Product>, ApiError> {
    match id {
        Some(id) => state.storage.get_product(id).await.map_err(internal),
        None => Ok(None),
    }
}

/// Push the linked lead forward according to the deal stage.
async fn sync_lead(state: &SharedState, deal: &Deal) -> Result<(), ApiError> {
    let Some(lead_id) = deal.lead_id else {
        return Ok(());
    };
    let Some(mut lead) = state.storage.get_lead(lead_id).await.map_err(internal)? else {
        return Ok(());
    };
    let next = promote_lead_stage(lead.stage, deal.stage);
    if next != lead.stage {
        lead.stage = next;
        lead.last_activity_at = Utc::now();
        state.storage.update_lead(&lead).await.map_err(internal)?;
    }
    Ok(())
}

async fn save_new_deal(state: &SharedState, deal: &Deal) -> Result<(), ApiError> {
    state.storage.save_deal(deal).await.map_err(internal)?;
    let mut ev = DealEvent::new(deal.id, "created");
    ev.to_stage = Some(deal.stage);
    state.storage.insert_deal_event(&ev).await.map_err(internal)?;
    sync_lead(state, deal).await
}

// ----- products -------------------------------------------------------------

#[derive(Deserialize)]
struct ProductQuery {
    #[serde(default)]
    include_inactive: bool,
}

async fn list_products(
    State(state): State<SharedState>,
    Query(q): Query<ProductQuery>,
) -> ApiResult<Vec<Product>> {
    state
        .storage
        .list_products(q.include_inactive)
        .await
        .map(Json)
        .map_err(internal)
}

async fn create_product(
    State(state): State<SharedState>,
    Json(input): Json<ProductInput>,
) -> Result<(StatusCode, Json<Product>), ApiError> {
    let p = Product::new(input).map_err(bad)?;
    state.storage.save_product(&p).await.map_err(internal)?;
    Ok((StatusCode::CREATED, Json(p)))
}

async fn get_product(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Product> {
    load_product(&state, id).await.map(Json)
}

async fn update_product(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(input): Json<ProductInput>,
) -> ApiResult<Product> {
    let mut p = load_product(&state, id).await?;
    p.apply(input).map_err(bad)?;
    state.storage.save_product(&p).await.map_err(internal)?;
    Ok(Json(p))
}

/// Products with deals are archived (active = false) instead of deleted,
/// so historical revenue stays attributed.
async fn delete_product(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<axum::response::Response, ApiError> {
    use axum::response::IntoResponse;
    let mut p = load_product(&state, id).await?;
    if state.storage.count_deals_for_product(id).await.map_err(internal)? > 0 {
        p.active = false;
        p.updated_at = Utc::now();
        state.storage.save_product(&p).await.map_err(internal)?;
        return Ok((StatusCode::OK, Json(p)).into_response());
    }
    state.storage.delete_product(id).await.map_err(internal)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

// ----- deals ----------------------------------------------------------------

#[derive(Deserialize, Default)]
struct DealQuery {
    #[serde(default)]
    stage: Option<DealStage>,
    #[serde(default)]
    product_id: Option<Uuid>,
    #[serde(default)]
    lead_id: Option<Uuid>,
}

async fn list_deals(
    State(state): State<SharedState>,
    Query(q): Query<DealQuery>,
) -> ApiResult<Vec<Deal>> {
    let filter = DealFilter {
        stage: q.stage,
        product_id: q.product_id,
        lead_id: q.lead_id,
    };
    state
        .storage
        .list_deals(&filter)
        .await
        .map(Json)
        .map_err(internal)
}

async fn create_deal(
    State(state): State<SharedState>,
    Json(req): Json<CreateDealRequest>,
) -> Result<(StatusCode, Json<Deal>), ApiError> {
    let product = optional_product(&state, req.product_id).await?;
    if let Some(lead_id) = req.lead_id {
        if state.storage.get_lead(lead_id).await.map_err(internal)?.is_none() {
            return Err((StatusCode::NOT_FOUND, "lead not found".into()));
        }
    }
    let deal = Deal::new(req, product.as_ref()).map_err(bad)?;
    save_new_deal(&state, &deal).await?;
    Ok((StatusCode::CREATED, Json(deal)))
}

async fn get_deal(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> ApiResult<DealDetails> {
    let deal = load_deal(&state, id).await?;
    let events = state.storage.list_deal_events(id).await.map_err(internal)?;
    Ok(Json(DealDetails { deal, events }))
}

async fn update_deal(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateDealRequest>,
) -> ApiResult<Deal> {
    if req.product_id.is_some() && optional_product(&state, req.product_id).await?.is_none() {
        return Err((StatusCode::NOT_FOUND, "product not found".into()));
    }
    let mut deal = load_deal(&state, id).await?;
    deal.apply(req).map_err(bad)?;
    state.storage.save_deal(&deal).await.map_err(internal)?;
    Ok(Json(deal))
}

async fn delete_deal(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    if state.storage.delete_deal(id).await.map_err(internal)? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((StatusCode::NOT_FOUND, "deal not found".into()))
    }
}

async fn change_stage(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(req): Json<ChangeDealStageRequest>,
) -> ApiResult<Deal> {
    let mut deal = load_deal(&state, id).await?;
    if let Some(ev) = deal.change_stage(req.stage, req.note) {
        state.storage.save_deal(&deal).await.map_err(internal)?;
        state.storage.insert_deal_event(&ev).await.map_err(internal)?;
        sync_lead(&state, &deal).await?;
    }
    Ok(Json(deal))
}

/// Create a deal from a marketing lead: inherits source/contact and marks the
/// lead Qualified. Body is a `CreateDealRequest` (all fields optional).
async fn deal_from_lead(
    State(state): State<SharedState>,
    Path(lead_id): Path<Uuid>,
    body: Option<Json<CreateDealRequest>>,
) -> Result<(StatusCode, Json<Deal>), ApiError> {
    let lead = state
        .storage
        .get_lead(lead_id)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "lead not found".into()))?;
    let mut req = body.map(|Json(b)| b).unwrap_or_default();
    req.lead_id = Some(lead.id);
    if req.source.trim().is_empty() {
        req.source = lead.source.clone();
    }
    if req.contact.trim().is_empty() {
        req.contact = lead_contact(&lead);
    }
    let product = optional_product(&state, req.product_id).await?;
    let deal = Deal::new(req, product.as_ref()).map_err(bad)?;
    save_new_deal(&state, &deal).await?;
    Ok((StatusCode::CREATED, Json(deal)))
}

fn lead_contact(lead: &Lead) -> String {
    for key in ["username", "name", "contact", "title"] {
        if let Some(v) = lead.metadata.get(key).and_then(|v| v.as_str()) {
            if !v.trim().is_empty() {
                return v.to_string();
            }
        }
    }
    lead.chat_id
        .clone()
        .or_else(|| lead.source_reference.clone())
        .unwrap_or_else(|| format!("lead {}", &lead.id.to_string()[..8]))
}

// ----- leads & summary ------------------------------------------------------

#[derive(Deserialize)]
struct LeadQuery {
    #[serde(default)]
    include_simulated: bool,
}

/// All marketing leads across runs — the CRM inbox for responses.
async fn list_leads(
    State(state): State<SharedState>,
    Query(q): Query<LeadQuery>,
) -> ApiResult<Vec<Lead>> {
    state
        .storage
        .list_all_leads(q.include_simulated)
        .await
        .map(Json)
        .map_err(internal)
}

#[derive(Deserialize)]
struct SummaryQuery {
    #[serde(default = "default_days")]
    days: u32,
}

fn default_days() -> u32 {
    30
}

async fn summary(
    State(state): State<SharedState>,
    Query(q): Query<SummaryQuery>,
) -> ApiResult<SalesSummary> {
    let products = state.storage.list_products(true).await.map_err(internal)?;
    let deals = state
        .storage
        .list_deals(&DealFilter::default())
        .await
        .map_err(internal)?;
    let leads = state.storage.list_all_leads(false).await.map_err(internal)?;
    Ok(Json(compute_sales_summary(
        &products,
        &deals,
        &leads,
        q.days,
        Utc::now(),
    )))
}

#[cfg(test)]
mod tests {
    use boarddo_shared::v2::{
        CapabilityBudget, CreateGoalRequest, GoalRun, GoalRunStatus, LeadStage, PolicyBundle,
    };
    use reqwest::Method;
    use serde_json::{Value, json};

    use super::*;
    use crate::objectives::tests::{call, serve_api, test_state};

    async fn seed_lead(state: &SharedState) -> Lead {
        let spec = crate::agent_runtime::spec_from_request(CreateGoalRequest {
            title: "Growth".into(),
            text: "leads".into(),
            metric: "leads_interested".into(),
            target: 10.0,
            deadline: None,
            constraints: json!({}),
            budget: CapabilityBudget::default(),
            policy: PolicyBundle::default(),
            agent_type_id: "agent.growth".into(),
            instructions: String::new(),
            tools: vec![],
            tick_interval_secs: Some(30),
            openai_connection_id: None,
            template_id: None,
        });
        state.storage.insert_goal_spec(&spec).await.unwrap();
        let run = GoalRun {
            id: Uuid::now_v7(),
            goal_id: spec.id,
            status: GoalRunStatus::Running,
            current: 0.0,
            target: spec.target,
            strategy: Default::default(),
            actions_today: 0,
            actions_day: "2026-10-15".into(),
            tick_count: 0,
            last_tick_at: None,
            next_tick_at: None,
            last_observe: None,
            last_think: None,
            error: None,
            started_at: Utc::now(),
            finished_at: None,
            replan: Default::default(),
        };
        state.storage.insert_goal_run(&run).await.unwrap();
        let mut metadata = serde_json::Map::new();
        metadata.insert("username".into(), json!("@buyer"));
        let lead = Lead {
            id: Uuid::now_v7(),
            goal_id: spec.id,
            run_id: run.id,
            campaign_id: None,
            experiment_id: None,
            source: "telegram".into(),
            source_reference: None,
            chat_id: Some("42".into()),
            account_id: None,
            stage: LeadStage::Interested,
            score: 5,
            score_reasons: vec![],
            first_seen_at: Utc::now(),
            last_activity_at: Utc::now(),
            metadata,
            simulated: false,
        };
        state.storage.insert_lead(&lead).await.unwrap();
        lead
    }

    async fn status_of(client: &reqwest::Client, method: Method, url: String, body: Value) -> u16 {
        client
            .request(method, &url)
            .json(&body)
            .send()
            .await
            .unwrap()
            .status()
            .as_u16()
    }

    #[tokio::test]
    async fn crud_pipeline_and_summary() {
        let state = test_state().await;
        let api = serve_api(state.clone()).await;
        let c = reqwest::Client::new();

        // validation
        assert_eq!(
            status_of(&c, Method::POST, format!("{api}/products"), json!({"name": ""})).await,
            400
        );

        let p = call(
            &c,
            Method::POST,
            format!("{api}/products"),
            json!({"name": "Курс", "price": 1000, "cost": 300}),
        )
        .await;
        let pid = p["id"].as_str().unwrap().to_string();
        assert_eq!(p["currency"], "RUB");

        let p = call(
            &c,
            Method::PUT,
            format!("{api}/products/{pid}"),
            json!({"name": "Курс PRO", "price": 1500, "cost": 500}),
        )
        .await;
        assert_eq!(p["name"], "Курс PRO");

        // deal from lead inherits source/contact and qualifies the lead
        let lead = seed_lead(&state).await;
        let d = call(
            &c,
            Method::POST,
            format!("{api}/deals/from-lead/{}", lead.id),
            json!({"product_id": pid, "quantity": 2}),
        )
        .await;
        let did = d["id"].as_str().unwrap().to_string();
        assert_eq!(d["amount"], 3000.0);
        assert_eq!(d["source"], "telegram");
        assert_eq!(d["contact"], "@buyer");
        let l = state.storage.get_lead(lead.id).await.unwrap().unwrap();
        assert_eq!(l.stage, LeadStage::Qualified);

        // manual deal, then lose it
        let d2 = call(
            &c,
            Method::POST,
            format!("{api}/deals"),
            json!({"title": "Сайт", "amount": 500, "source": "site"}),
        )
        .await;
        let d2id = d2["id"].as_str().unwrap().to_string();
        call(
            &c,
            Method::POST,
            format!("{api}/deals/{d2id}/stage"),
            json!({"stage": "lost", "note": "дорого"}),
        )
        .await;

        // win the lead deal → lead converted
        let won = call(
            &c,
            Method::POST,
            format!("{api}/deals/{did}/stage"),
            json!({"stage": "won"}),
        )
        .await;
        assert!(won["closed_at"].is_string());
        let l = state.storage.get_lead(lead.id).await.unwrap().unwrap();
        assert_eq!(l.stage, LeadStage::Converted);

        let details = call(&c, Method::GET, format!("{api}/deals/{did}"), Value::Null).await;
        let events = details["events"].as_array().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[1]["to_stage"], "won");

        let patched = call(
            &c,
            Method::PATCH,
            format!("{api}/deals/{did}"),
            json!({"notes": "оплата картой"}),
        )
        .await;
        assert_eq!(patched["notes"], "оплата картой");
        assert_eq!(patched["stage"], "won");

        let won_list = call(&c, Method::GET, format!("{api}/deals?stage=won"), Value::Null).await;
        assert_eq!(won_list.as_array().unwrap().len(), 1);

        let leads = call(&c, Method::GET, format!("{api}/sales/leads"), Value::Null).await;
        assert_eq!(leads.as_array().unwrap().len(), 1);

        let s = call(&c, Method::GET, format!("{api}/sales/summary?days=7"), Value::Null).await;
        assert_eq!(s["totals"]["deals"], 2);
        assert_eq!(s["totals"]["won"], 1);
        assert_eq!(s["totals"]["lost"], 1);
        assert_eq!(s["totals"]["revenue"], 3000.0);
        assert_eq!(s["totals"]["gross_profit"], 2000.0);
        assert_eq!(s["totals"]["win_rate"], 0.5);
        assert_eq!(s["leads"]["converted"], 1);
        assert_eq!(s["leads"]["lead_to_sale"], 1.0);
        assert_eq!(s["daily"].as_array().unwrap().len(), 7);

        // product with deals is archived, not deleted
        assert_eq!(
            status_of(&c, Method::DELETE, format!("{api}/products/{pid}"), Value::Null).await,
            200
        );
        let active = call(&c, Method::GET, format!("{api}/products"), Value::Null).await;
        assert!(active.as_array().unwrap().is_empty());
        let all = call(
            &c,
            Method::GET,
            format!("{api}/products?include_inactive=true"),
            Value::Null,
        )
        .await;
        assert_eq!(all[0]["active"], false);

        // errors
        let missing = Uuid::new_v4();
        assert_eq!(
            status_of(&c, Method::GET, format!("{api}/deals/{missing}"), Value::Null).await,
            404
        );
        assert_eq!(
            status_of(
                &c,
                Method::POST,
                format!("{api}/deals"),
                json!({"title": "x", "product_id": missing})
            )
            .await,
            404
        );
        assert_eq!(
            status_of(&c, Method::POST, format!("{api}/deals/from-lead/{missing}"), json!({}))
                .await,
            404
        );

        assert_eq!(
            status_of(&c, Method::DELETE, format!("{api}/deals/{d2id}"), Value::Null).await,
            204
        );
    }
}
