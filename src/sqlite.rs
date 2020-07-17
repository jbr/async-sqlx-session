use async_session::{async_trait, chrono::Utc, log, serde_json, Result, Session, SessionStore};
use sqlx::prelude::*;
use sqlx::{pool::PoolConnection, sqlite::SqlitePool, SqliteConnection};

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

    async fn connection(&self) -> sqlx::Result<PoolConnection<SqliteConnection>> {
        self.client.acquire().await
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

    async fn store_session(&self, session: Session) -> Option<String> {
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
        .bind(&session.expiry().map(|expiry| expiry.timestamp()))
        .bind(&string)
        .execute(&mut connection)
        .await
        .unwrap();

        session.into_cookie_value()
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

    async fn cleanup(&self) -> Result {
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
