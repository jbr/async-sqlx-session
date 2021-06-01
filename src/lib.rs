/*!
# async-sqlx-session

This crate currently provides several session stores, each of which is
enabled by a feature flag.

* To use [`SqliteSessionStore`], enable the `sqlite` feature on this
crate.
* To use [`PostgresSessionStore`], enable the `pg` feature on this
crate.
* To use [`MysqlSessionStore`], enable the `mysql` feature on this
crate.

To use the `spawn_cleanup_task` function for either store on
async-std, enable the `async_std` feature. To perform session cleanup
intermittently with a different runtime, use a function like:

```rust,ignore
fn clean_up_intermittently(store: &SqliteSessionStore, period: Duration) {
    let store = store.clone();
    other_runtime::spawn(async move {
        loop {
            other_runtime::sleep(period).await;
            if let Err(error) = store.cleanup().await {
                log::error!("cleanup error: {}", error);
            }
        }
    });
}
```
*/

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
