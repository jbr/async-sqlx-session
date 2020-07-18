use async_session::{async_trait, chrono::Utc, log, serde_json, Result, Session, SessionStore};
use async_std::task;
use sqlx::prelude::*;
use sqlx::{pool::PoolConnection, sqlite::SqlitePool, SqliteConnection};
use std::time::Duration;

/// sqlx sqlite session store for async-sessions
///
/// ```rust
/// use async_sqlx_session::SqliteStore;
/// use async_session::SessionStore;
/// # fn main() -> async_session::Result { async_std::task::block_on(async {
/// let store = SqliteStore::new("sqlite:%3Amemory:").await?;
/// store.migrate().await?;
/// store.spawn_cleanup_task(std::time::Duration::from_secs(60 * 60));
///
/// let mut session = async_session::Session::new();
/// session.insert("key".into(), "value".into());
///
/// let cookie_value = store.store_session(session).await.unwrap();
/// let session = store.load_session(cookie_value).await.unwrap();
/// assert_eq!(session.get("key"), Some("value".to_owned()));
/// # Ok(()) }) }
///
#[derive(Clone, Debug)]
pub struct SqliteStore {
    client: SqlitePool,
    table_name: String,
}

impl SqliteStore {
    pub fn from_client(client: SqlitePool) -> Self {
        Self {
            client,
            table_name: "async_sessions".into(),
        }
    }

    pub async fn new(database_url: &str) -> sqlx::Result<Self> {
        Ok(Self::from_client(SqlitePool::new(database_url).await?))
    }

    pub async fn new_with_table_name(database_url: &str, table_name: &str) -> sqlx::Result<Self> {
        Ok(Self::new(database_url).await?.with_table_name(table_name))
    }

    pub fn with_table_name(mut self, table_name: impl AsRef<str>) -> Self {
        let table_name = table_name.as_ref();
        if table_name.is_empty()
            || !table_name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            panic!(
                "table name must be [a-zA-Z0-9_-]+, but {} was not",
                table_name
            );
        }

        self.table_name = table_name.to_owned();
        self
    }

    pub async fn migrate(&self) -> sqlx::Result<()> {
        log::info!("migrating sessions on `{}`", self.table_name);

        let mut conn = self.client.acquire().await?;
        sqlx::query(&self.substitute_table_name(
            r#"
            CREATE TABLE IF NOT EXISTS %%TABLE_NAME%% (
                id TEXT PRIMARY KEY NOT NULL,
                expires INTEGER NULL,
                session TEXT NOT NULL
            )
            "#,
        ))
        .execute(&mut conn)
        .await?;
        Ok(())
    }

    fn substitute_table_name(&self, query: &str) -> String {
        query.replace("%%TABLE_NAME%%", &self.table_name)
    }

    async fn connection(&self) -> sqlx::Result<PoolConnection<SqliteConnection>> {
        self.client.acquire().await
    }

    pub fn spawn_cleanup_task(&self, period: Duration) -> task::JoinHandle<()> {
        let store = self.clone();
        task::spawn(async move {
            loop {
                task::sleep(period).await;
                if let Err(error) = store.cleanup().await {
                    log::error!("cleanup error: {}", error);
                }
            }
        })
    }

    pub async fn cleanup(&self) -> sqlx::Result<()> {
        let mut connection = self.connection().await?;
        sqlx::query(&self.substitute_table_name(
            r#"
            DELETE FROM %%TABLE_NAME%%
            WHERE expires < ?
            "#,
        ))
        .bind(Utc::now().timestamp())
        .execute(&mut connection)
        .await?;

        Ok(())
    }
}

#[async_trait]
impl SessionStore for SqliteStore {
    async fn load_session(&self, cookie_value: String) -> Option<Session> {
        let id = Session::id_from_cookie_value(&cookie_value).ok()?;
        let mut connection = self.connection().await.ok()?;

        let (session,): (String,) = sqlx::query_as(&self.substitute_table_name(
            r#"
            SELECT session FROM %%TABLE_NAME%%
              WHERE id = ? AND (expires IS NULL || expires > ?)
            "#,
        ))
        .bind(&id)
        .bind(Utc::now().timestamp())
        .fetch_one(&mut connection)
        .await
        .ok()?;

        serde_json::from_str(&session).ok()?
    }

    async fn store_session(&self, session: Session) -> Option<String> {
        let id = session.id();
        let string = serde_json::to_string(&session).ok()?;
        let mut connection = self.connection().await.ok()?;

        let result = sqlx::query(&self.substitute_table_name(
            r#"
            INSERT INTO %%TABLE_NAME%%
              (id, session, expires) VALUES (?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
              expires = excluded.expires,
              session = excluded.session
            "#,
        ))
        .bind(&id)
        .bind(&string)
        .bind(&session.expiry().map(|expiry| expiry.timestamp()))
        .execute(&mut connection)
        .await;

        if let Err(e) = result {
            dbg!(e);
            None
        } else {
            session.into_cookie_value()
        }
    }

    async fn destroy_session(&self, session: Session) -> Result {
        let id = session.id();
        let mut connection = self.connection().await?;
        sqlx::query(&self.substitute_table_name(
            r#"
            DELETE FROM %%TABLE_NAME%% WHERE id = ?
            "#,
        ))
        .bind(&id)
        .execute(&mut connection)
        .await?;

        Ok(())
    }

    async fn clear_store(&self) -> Result {
        let mut connection = self.connection().await?;
        sqlx::query(&self.substitute_table_name(
            r#"
            DELETE FROM %%TABLE_NAME%%
            "#,
        ))
        .execute(&mut connection)
        .await?;

        Ok(())
    }
}
