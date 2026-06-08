pub mod audit;
pub mod background;
pub mod config;
pub mod db;
pub mod email;
pub mod error;
pub mod handlers;
pub mod i18n;
pub mod middleware;
pub mod report_pdf;
pub mod repository;
pub mod roles;
pub mod router;
pub mod services;
pub mod state;
pub mod time_calc;

pub use router::{build_api_router, build_app};
pub use state::AppState;
