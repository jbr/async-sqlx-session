//! # async-sqlx-session
//!
//! This crate currently provides two session stores: [`PostgresSessionStore`] and [`SqliteSessionStore`], each of which is enabled by a feature flag.
//! To use `SqliteSessionStore`, enable the `sqlite` feature on this crate.
//! To use `PostgresSessionStore`, enable the `pg` feature on this crate.
#![forbid(unsafe_code, future_incompatible)]
#![deny(
    missing_debug_implementations,
    nonstandard_style,
    missing_docs,
    unreachable_pub,
    missing_copy_implementations,
    unused_qualifications
)]

#[cfg(feature = "sqlite")]
mod sqlite;
#[cfg(feature = "sqlite")]
pub use sqlite::SqliteSessionStore;

#[cfg(feature = "pg")]
mod pg;
#[cfg(feature = "pg")]
pub use pg::PostgresSessionStore;

#[cfg(feature = "mysql")]
mod mysql;
#[cfg(feature = "mysql")]
pub use mysql::MySqlSessionStore;
