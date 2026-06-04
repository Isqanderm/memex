pub mod documents;
pub mod jobs;
pub mod memories;
pub mod query;
pub mod state;

use axum::Router;
use state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(documents::router())
        .merge(query::router())
        .merge(jobs::router())
        .merge(memories::router())
}
