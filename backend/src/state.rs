use crate::db::DatabasePool;
use crate::repository;
use crate::services::notifications::NotificationBroadcaster;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub pool: DatabasePool,
    pub db: repository::Db,
    pub cfg: Arc<crate::config::Config>,
    pub notifications: NotificationBroadcaster,
}
