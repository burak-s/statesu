//! Statesu backend: a small headless Markdown CMS over HTTP.
//!
//! Layers, inside out: `http` handlers parse requests and map statuses, the
//! `auth`/`blog`/`post` services own rules and transactions, the `*/repo`
//! modules own SQL, and `crypto` owns hashing, encryption and tokens.
//! Public delivery lives on `/public/...` routes with per-blog browser
//! origins; management requires a bearer token. Everything is wired by
//! constructor injection in [`bootstrap::build_app`].

pub mod auth;
pub mod blog;
pub mod bootstrap;
pub mod clock;
pub mod config;
pub mod crypto;
pub mod error;
pub mod http;
pub mod ids;
pub mod pagination;
pub mod post;
