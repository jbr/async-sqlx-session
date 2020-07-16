use async_session::{async_trait, base64, log, serde_json, Session, SessionStore};
use chrono::Utc;
use sqlx::prelude::*;
use sqlx::sqlite::SqlitePool;

#[derive(Clone, Debug)]
pub struct SqliteStore {
    client: SqlitePool,
    ttl: chrono::Duration,
    prefix: Option<String>,
    table_name: String,
}

impl SqliteStore {
    pub fn from_client(client: SqlitePool) -> Self {
        Self {
            client,
            table_name: "async_sessions".into(),
            ttl: chrono::Duration::days(1),
            prefix: None,
        }
    }

    pub async fn new(database_url: &str) -> Result<Self, sqlx::Error> {
        Ok(Self::from_client(SqlitePool::new(database_url).await?))
    }

    pub fn with_table_name(mut self, table_name: String) -> Self {
        if table_name.chars().any(|c| c.is_ascii_alphanumeric()) {
            panic!(
                "table name must be alphanumeric, but {} was not",
                table_name
            );
        }

        self.table_name = table_name;
        self
    }

    pub fn with_ttl(mut self, ttl: std::time::Duration) -> Self {
        self.ttl = chrono::Duration::from_std(ttl).unwrap();
        self
    }

    pub fn expiry(&self) -> i64 {
        (Utc::now() + self.ttl).timestamp()
    }

    pub async fn migrate(&self) -> sqlx::Result<()> {
        log::info!("migrating sessions on `{}`", self.table_name);

        let mut conn = self.client.acquire().await?;
        sqlx::query(&self.substitute_table_name(
            r#"
            CREATE TABLE IF NOT EXISTS %%TABLE_NAME%% (
                id TEXT PRIMARY KEY NOT NULL,
                expires DATETIME NOT NULL,
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

    async fn connection(&self) -> sqlx::Result<sqlx::pool::PoolConnection<sqlx::SqliteConnection>> {
        self.client.acquire().await
    }
}

#[async_trait]
impl SessionStore for SqliteStore {
    type Error = sqlx::Error;

    async fn load_session(&self, cookie_value: String) -> Option<Session> {
        let id = Session::id_from_cookie_value(&cookie_value).ok()?;
        let mut connection = self.connection().await.ok()?;

        let (session,): (String,) = sqlx::query_as(&self.substitute_table_name(
            r#"
            SELECT session FROM %%TABLE_NAME%%
              WHERE id = ? AND expires > ?
            "#,
        ))
        .bind(&id)
        .bind(Utc::now().timestamp())
        .fetch_one(&mut connection)
        .await
        .ok()?;

        serde_json::from_str(&session).ok()?
    }

    async fn store_session(&self, mut session: Session) -> Option<String> {
        let id = session.id();
        let string = serde_json::to_string(&session).ok()?;
        let mut connection = self.connection().await.ok()?;

        sqlx::query(&self.substitute_table_name(
            r#"
            INSERT INTO %%TABLE_NAME%%
              (id, expires, session) VALUES (?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
              expires = excluded.expires,
              session = excluded.session
            "#,
        ))
        .bind(&id)
        .bind(self.expiry())
        .bind(&string)
        .execute(&mut connection)
        .await
        .unwrap();

        session.take_cookie_value()
    }

    async fn destroy_session(&self, session: Session) -> Result<(), Self::Error> {
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

    async fn clear_store(&self) -> Result<(), Self::Error> {
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

#[derive(Debug)]
pub enum Error {
    SqlxError(sqlx::Error),
    SerdeError(serde_json::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::SqlxError(e) => e.fmt(f),
            Error::SerdeError(e) => e.fmt(f),
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Self::SerdeError(e)
    }
}

impl From<sqlx::Error> for Error {
    fn from(e: sqlx::Error) -> Self {
        Self::SqlxError(e)
    }
}

impl std::error::Error for Error {}
