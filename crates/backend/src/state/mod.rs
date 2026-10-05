use std::path::PathBuf;
use std::sync::Arc;

use boarddo_shared::ExecutionEvent;
use boarddo_shared::v2::GoalRuntimeEvent;
use boarddo_telegram::TelegramEngine;
use tokio::sync::broadcast;

use crate::connections::StorageConnectionProvider;
use crate::engine::Engine;
use crate::engine::node_state::SqliteNodeStateStore;
use crate::ingress::IngressService;
use crate::integrations::media::MediaStore;
use crate::runtime::RuntimeSupervisor;
use crate::storage::Storage;
use crate::tools::service::{CustomToolGateway, ToolService};

const EVENT_CAPACITY: usize = 256;

pub struct AppState {
    pub storage: Storage,
    pub engine: Engine,
    pub events: broadcast::Sender<ExecutionEvent>,
    pub goal_events: broadcast::Sender<GoalRuntimeEvent>,
    pub connections: Arc<StorageConnectionProvider>,
    pub secrets: Arc<dyn crate::secrets::SecretStore>,
    pub ingress: Arc<IngressService>,
    pub telegram: Arc<TelegramEngine>,
    pub runtime: RuntimeSupervisor,
    pub custom_tools: Arc<dyn CustomToolGateway>,
    pub media: MediaStore,
    pub data_dir: PathBuf,
}

impl AppState {
    pub fn new(
        storage: Storage,
        secrets: Arc<dyn crate::secrets::SecretStore>,
        connections: Arc<StorageConnectionProvider>,
        telegram: Arc<TelegramEngine>,
        data_dir: PathBuf,
    ) -> Self {
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        let (goal_events, _) = broadcast::channel(EVENT_CAPACITY);
        let gateway: Arc<dyn boarddo_telegram::TelegramUserGateway> = telegram.clone();
        let node_state = Arc::new(SqliteNodeStateStore::new(storage.clone()));
        let custom_tools: Arc<dyn CustomToolGateway> =
            Arc::new(ToolService::new(storage.clone()));
        let media = MediaStore::new(&data_dir);
        let engine = Engine::with_connections(connections.clone())
            .with_telegram_user(gateway)
            .with_node_state(node_state)
            .with_custom_tools(custom_tools.clone())
            .with_media(media.clone());
        let ingress = Arc::new(IngressService::new(storage.clone()));
        Self {
            storage,
            engine,
            events,
            goal_events,
            connections,
            secrets,
            ingress,
            telegram,
            runtime: RuntimeSupervisor::new(),
            custom_tools,
            media,
            data_dir,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ExecutionEvent> {
        self.events.subscribe()
    }

    pub fn emit(&self, event: ExecutionEvent) {
        let _ = self.events.send(event);
    }

    pub fn subscribe_goal(&self) -> broadcast::Receiver<GoalRuntimeEvent> {
        self.goal_events.subscribe()
    }

    pub fn emit_goal(&self, event: GoalRuntimeEvent) {
        let _ = self.goal_events.send(event);
    }
}

pub type SharedState = Arc<AppState>;
