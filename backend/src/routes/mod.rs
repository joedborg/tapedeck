pub mod queue;
pub mod search;
pub mod settings;
pub mod users;
pub mod ws;

use axum::{
    Router,
    routing::{delete, get, post, put},
};
use tower_http::{
    compression::CompressionLayer,
    cors::{Any, CorsLayer},
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};

use crate::{auth::login_handler, state::AppState};

pub fn build_router(state: AppState, static_dir: &str) -> Router {
    let api = Router::new()
        // Auth
        .route("/auth/login", post(login_handler))
        // Queue
        .route("/queue", get(queue::list_queue).post(queue::add_to_queue))
        .route(
            "/queue/{id}",
            get(queue::get_queue_item).delete(queue::remove_from_queue),
        )
        .route("/queue/{id}/retry", post(queue::retry_queue_item))
        .route("/queue/reorder", post(queue::reorder_queue))
        // Search
        .route("/search", get(search::search))
        .route("/search/episodes", get(search::list_episodes))
        .route("/search/refresh", post(search::refresh_cache))
        // Settings
        .route(
            "/settings",
            get(settings::list_settings).patch(settings::bulk_update_settings),
        )
        .route(
            "/settings/{key}",
            get(settings::get_setting).put(settings::set_setting),
        )
        // Users
        .route("/users", get(users::list_users).post(users::create_user))
        .route("/users/me", get(users::get_me))
        .route("/users/{id}", delete(users::delete_user))
        .route("/users/{id}/password", put(users::change_password));

    // CORS — in production, restrict `allow_origin` to your domain
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        // WebSocket endpoint (outside /api, no CORS needed)
        .route("/ws", get(ws::ws_handler))
        // REST API
        .nest("/api", api)
        // Serve the compiled Ember.js app for all other paths (SPA fallback)
        .fallback_service(
            ServeDir::new(static_dir)
                .not_found_service(ServeFile::new(format!("{static_dir}/index.html"))),
        )
        .layer(cors)
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
