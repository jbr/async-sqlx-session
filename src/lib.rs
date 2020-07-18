//! # async-sqlx-session
//!
//! This crate currently only provides a sqlite session store, but
//! will eventually support other databases as well, configurable
//! through a feature flag.
//!
//! For now, see the documentation for
//! [`SqliteStore`](crate::SqliteSessionStore)
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
