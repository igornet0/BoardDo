mod agent;
mod browser;
mod connections;
mod executions;
mod goals;
mod health;
mod ingress;
mod media;
pub(crate) mod run;
mod runtime;
mod telegram;
mod tools;
mod webhooks;
mod websockets;
mod workflows;

use axum::Router;
use axum::routing::{get, post, put};
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::state::SharedState;

pub fn router(state: SharedState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/api/health", get(health::health))
        .route("/api/browser/status", get(browser::status))
        .route("/api/browser/stop", post(browser::stop))
        .route(
            "/api/workflows",
            get(workflows::list).post(workflows::create),
        )
        .route(
            "/api/workflows/{id}",
            get(workflows::get)
                .put(workflows::update)
                .delete(workflows::delete),
        )
        .route("/api/workflows/{id}/validate", post(workflows::validate))
        .route("/api/workflows/{id}/run", post(workflows::run))
        .route("/api/runtimes", get(runtime::list))
        .route("/api/workflows/{id}/runtime", get(runtime::get))
        .route("/api/workflows/{id}/runtime/start", post(runtime::start))
        .route("/api/workflows/{id}/runtime/stop", post(runtime::stop))
        .route("/api/workflows/{id}/runtime/reload", post(runtime::reload))
        .route(
            "/api/workflows/{id}/schedules",
            get(workflows::list_schedules),
        )
        .route(
            "/api/connections",
            get(connections::list).post(connections::create),
        )
        .route(
            "/api/connections/{id}",
            get(connections::get)
                .put(connections::update)
                .delete(connections::delete),
        )
        .route("/api/connections/{id}/test", post(connections::test))
        .route(
            "/api/channels",
            get(ingress::list_channels).post(ingress::create_channel),
        )
        .route(
            "/api/channels/{id}",
            get(ingress::get_channel)
                .put(ingress::update_channel)
                .delete(ingress::delete_channel),
        )
        .route(
            "/api/streams",
            get(ingress::list_streams).post(ingress::create_stream),
        )
        .route(
            "/api/streams/{id}",
            get(ingress::get_stream)
                .put(ingress::update_stream)
                .delete(ingress::delete_stream),
        )
        .route(
            "/api/triggers",
            get(ingress::list_triggers).post(ingress::create_trigger),
        )
        .route(
            "/api/triggers/{id}",
            get(ingress::get_trigger)
                .put(ingress::update_trigger)
                .delete(ingress::delete_trigger),
        )
        .route(
            "/api/telegram/accounts",
            get(telegram::list_accounts).post(telegram::create_account),
        )
        .route(
            "/api/telegram/accounts/{id}",
            get(telegram::get_account).delete(telegram::delete_account),
        )
        .route(
            "/api/telegram/accounts/{id}/start",
            post(telegram::start_account),
        )
        .route(
            "/api/telegram/accounts/{id}/stop",
            post(telegram::stop_account),
        )
        .route(
            "/api/telegram/accounts/{id}/connect",
            post(telegram::connect_account),
        )
        .route(
            "/api/telegram/accounts/{id}/disconnect",
            post(telegram::disconnect_account),
        )
        .route(
            "/api/telegram/accounts/{id}/status",
            get(telegram::account_status),
        )
        .route(
            "/api/telegram/accounts/{id}/auth/phone",
            post(telegram::submit_phone),
        )
        .route(
            "/api/telegram/accounts/{id}/auth/code",
            post(telegram::submit_code),
        )
        .route(
            "/api/telegram/accounts/{id}/auth/password",
            post(telegram::submit_password),
        )
        .route(
            "/api/telegram/accounts/{id}/chats",
            get(telegram::list_chats),
        )
        .route(
            "/api/telegram/accounts/{id}/chats/search",
            get(telegram::search_chats),
        )
        .route("/api/telegram/capabilities", get(telegram::capabilities))
        .route(
            "/api/telegram/accounts/{id}/chats/{chat_id}",
            get(telegram::get_chat),
        )
        .route(
            "/api/telegram/accounts/{id}/messages",
            get(telegram::list_messages),
        )
        .route(
            "/api/telegram/accounts/{id}/messages/send",
            post(telegram::send_message),
        )
        .route(
            "/api/telegram/accounts/{id}/permissions",
            put(telegram::update_permissions),
        )
        .route(
            "/api/telegram/accounts/{id}/simulate/message",
            post(telegram::simulate_message),
        )
        .route("/api/telegram/audit", get(telegram::list_audit))
        .route("/api/webhooks/{workflow_id}", post(webhooks::webhook))
        .route("/api/executions", get(executions::list))
        .route("/api/executions/changelogs", get(executions::list_changelogs))
        .route("/api/executions/{id}", get(executions::get))
        .route("/api/executions/{id}/nodes", get(executions::nodes))
        .route("/api/executions/{id}/changelog", get(executions::changelog))
        .route("/ws", get(websockets::ws_handler))
        .route("/ws/telegram", get(websockets::telegram_ws_handler))
        .route("/ws/goals", get(websockets::goals_ws_handler))
        .route("/api/tools", get(tools::list).post(tools::create))
        .route("/api/tools/find", get(tools::find))
        .route("/api/tools/{id}", get(tools::get).delete(tools::delete))
        .route("/api/tools/{id}/run", post(tools::run))
        .route("/api/tools/{id}/test", post(tools::test_run))
        .route("/api/media/{name}", get(media::get_media))
        .nest("/api/agent", agent::router())
        .nest("/api", goals::router())
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
