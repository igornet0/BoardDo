//! BoardDo Telegram multi-account engine.
//!
//! TelegramAccountManager is lifecycle-only. Auth, sessions, events, permissions,
//! audit, and automation matching live in dedicated modules.

mod account_manager;
mod audit;
mod automation;
mod engine;
mod events;
mod outbound;
mod permission;
mod reconnect;
mod session;
mod store;

pub use account_manager::TelegramAccountManager;
pub use audit::TelegramAuditService;
pub use automation::{matches_message_trigger, MessageTriggerFilter};
pub use engine::{NullTelegramUserGateway, TelegramEngine, TelegramUserGateway};
pub use events::TelegramEventRouter;
pub use outbound::OutboundTracker;
pub use permission::TelegramPermissionService;
pub use session::{session_paths, SessionPaths};
pub use store::TelegramSessionStore;

pub const TELEGRAM_ENGINE_STATUS: &str = tglib::TELEGRAM_ENGINE_STATUS;

#[cfg(test)]
mod tests;
