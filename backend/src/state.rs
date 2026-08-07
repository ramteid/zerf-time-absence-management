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
    /// Shared circuit breaker guarding every real SMTP attempt (both the
    /// email-queue drain and the payroll report's attachment send) so a
    /// down mail server isn't hammered by both paths independently.
    pub email_circuit_breaker: Arc<crate::email::CircuitBreaker>,
}
