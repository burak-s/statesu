//! Statesu backend: user statuses over HTTP.
//!
//! Layers, inside out: `http` handlers parse requests and map statuses, the
//! `auth`/`state` services own rules and transactions, the `*/repo` modules
//! own SQL, and `crypto` owns hashing, encryption and tokens. Everything is
//! wired by constructor injection in [`bootstrap::build_app`].

pub mod auth;
pub mod bootstrap;
pub mod clock;
pub mod config;
pub mod crypto;
pub mod error;
pub mod http;
pub mod state;
