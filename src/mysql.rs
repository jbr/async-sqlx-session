use async_session::{async_trait, chrono::Utc, log, serde_json, Result, Session, SessionStore};
use sqlx::{pool::PoolConnection, Executor, MySql, MySqlPool};

/// sqlx mysql session store for async-sessions
///
/// ```rust
/// use async_sqlx_session::MySqlSessionStore;
/// use async_session::{Session, SessionStore};
/// use std::time::Duration;
///
/// # fn main() -> async_session::Result { async_std::task::block_on(async {
/// let store = MySqlSessionStore::new(&std::env::var("MYSQL_TEST_DB_URL").unwrap()).await?;
/// store.migrate().await?;
/// # store.clear_store().await?;
/// # #[cfg(feature = "async_std")]
/// store.spawn_cleanup_task(Duration::from_secs(60 * 60));
///
/// let mut session = Session::new();
/// session.insert("key", vec![1,2,3]);
///
/// let cookie_value = store.store_session(session).await?.unwrap();
/// let session = store.load_session(cookie_value).await?.unwrap();
/// assert_eq!(session.get::<Vec<i8>>("key").unwrap(), vec![1,2,3]);
/// # Ok(()) }) }
///
#[derive(Clone, Debug)]
pub struct MySqlSessionStore {
    client: MySqlPool,
    table_name: String,
}

impl MySqlSessionStore {
    /// constructs a new MySqlSessionStore from an existing
    /// sqlx::MySqlPool.  the default table name for this session
    /// store will be "async_sessions". To override this, chain this
    /// with [`with_table_name`](crate::MySqlSessionStore::with_table_name).
    ///
    /// ```rust
    /// # use async_sqlx_session::MySqlSessionStore;
    /// # use async_session::Result;
    /// # fn main() -> Result { async_std::task::block_on(async {
    /// let pool = sqlx::MySqlPool::connect(&std::env::var("MYSQL_TEST_DB_URL").unwrap()).await.unwrap();
    /// let store = MySqlSessionStore::from_client(pool)
    ///     .with_table_name("custom_table_name");
    /// store.migrate().await;
    /// # Ok(()) }) }
    /// ```
    pub fn from_client(client: MySqlPool) -> Self {
        Self {
            client,
            table_name: "async_sessions".into(),
        }
    }

    /// Constructs a new MySqlSessionStore from a mysql://
    /// database url. The default table name for this session store
    /// will be "async_sessions". To override this, either chain with
    /// [`with_table_name`](crate::MySqlSessionStore::with_table_name)
    /// or use
    /// [`new_with_table_name`](crate::MySqlSessionStore::new_with_table_name)
    ///
    /// ```rust
    /// # use async_sqlx_session::MySqlSessionStore;
    /// # use async_session::Result;
    /// # fn main() -> Result { async_std::task::block_on(async {
    /// let store = MySqlSessionStore::new(&std::env::var("MYSQL_TEST_DB_URL").unwrap()).await?;
    /// store.migrate().await;
    /// # Ok(()) }) }
    /// ```
    pub async fn new(database_url: &str) -> sqlx::Result<Self> {
        let pool = MySqlPool::connect(database_url).await?;
        Ok(Self::from_client(pool))
    }

    /// constructs a new MySqlSessionStore from a mysql:// url. the
    /// default table name for this session store will be
    /// "async_sessions". To override this, either chain with
    /// [`with_table_name`](crate::MySqlSessionStore::with_table_name) or
    /// use
    /// [`new_with_table_name`](crate::MySqlSessionStore::new_with_table_name)
    ///
    /// ```rust
    /// # use async_sqlx_session::MySqlSessionStore;
    /// # use async_session::Result;
    /// # fn main() -> Result { async_std::task::block_on(async {
    /// let store = MySqlSessionStore::new_with_table_name(&std::env::var("MYSQL_TEST_DB_URL").unwrap(), "custom_table_name").await?;
    /// store.migrate().await;
    /// # Ok(()) }) }
    /// ```
    pub async fn new_with_table_name(database_url: &str, table_name: &str) -> sqlx::Result<Self> {
        Ok(Self::new(database_url).await?.with_table_name(table_name))
    }

    /// Chainable method to add a custom table name. This will panic
    /// if the table name is not `[a-zA-Z0-9_-]`.
    /// ```rust
    /// # use async_sqlx_session::MySqlSessionStore;
    /// # use async_session::Result;
    /// # fn main() -> Result { async_std::task::block_on(async {
    /// let store = MySqlSessionStore::new(&std::env::var("MYSQL_TEST_DB_URL").unwrap()).await?
    ///     .with_table_name("custom_name");
    /// store.migrate().await;
    /// # Ok(()) }) }
    /// ```
    ///
    /// ```should_panic
    /// # use async_sqlx_session::MySqlSessionStore;
    /// # use async_session::Result;
    /// # fn main() -> Result { async_std::task::block_on(async {
    /// let store = MySqlSessionStore::new(&std::env::var("MYSQL_TEST_DB_URL").unwrap()).await?
    ///     .with_table_name("johnny (); drop users;");
    /// # Ok(()) }) }
    /// ```
    pub fn with_table_name(mut self, table_name: impl AsRef<str>) -> Self {
        let table_name = table_name.as_ref();
        if table_name.is_empty()
            || !table_name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            panic!(
                "table name must be [a-zA-Z0-9_-], but {} was not",
                table_name
            );
        }

        self.table_name = table_name.to_owned();
        self
    }

    /// Creates a session table if it does not already exist. If it
    /// does, this will noop, making it safe to call repeatedly on
    /// store initialization. In the future, this may make
    /// exactly-once modifications to the schema of the session table
    /// on breaking releases.
    /// ```rust
    /// # use async_sqlx_session::MySqlSessionStore;
    /// # use async_session::{Result, SessionStore, Session};
    /// # fn main() -> Result { async_std::task::block_on(async {
    /// let store = MySqlSessionStore::new(&std::env::var("MYSQL_TEST_DB_URL").unwrap()).await?;
    /// # store.clear_store().await?;
    /// store.migrate().await?;
    /// store.store_session(Session::new()).await?;
    /// store.migrate().await?; // calling it a second time is safe
    /// assert_eq!(store.count().await?, 1);
    /// # Ok(()) }) }
    /// ```
    pub async fn migrate(&self) -> sqlx::Result<()> {
        log::info!("migrating sessions on `{}`", self.table_name);

        let mut conn = self.client.acquire().await?;
        conn.execute(&*self.substitute_table_name(
            r#"
            CREATE TABLE IF NOT EXISTS %%TABLE_NAME%% (
                `id` VARCHAR(128) NOT NULL,
                `expires` TIMESTAMP(6) NULL,
                `session` TEXT NOT NULL,
                PRIMARY KEY (`id`),
                KEY `expires` (`expires`)
            )
            ENGINE=InnoDB
            DEFAULT CHARSET=utf8
            "#,
        ))
        .await?;

        Ok(())
    }

    fn substitute_table_name(&self, query: &str) -> String {
        query.replace("%%TABLE_NAME%%", &self.table_name)
    }

    /// retrieve a connection from the pool
    async fn connection(&self) -> sqlx::Result<PoolConnection<MySql>> {
        self.client.acquire().await
    }

    /// Spawns an async_std::task that clears out stale (expired)
    /// sessions on a periodic basis. Only available with the
    /// async_std feature enabled.
    ///
    /// ```rust,no_run
    /// # use async_sqlx_session::MySqlSessionStore;
    /// # use async_session::{Result, SessionStore, Session};
    /// # use std::time::Duration;
    /// # fn main() -> Result { async_std::task::block_on(async {
    /// let store = MySqlSessionStore::new(&std::env::var("MYSQL_TEST_DB_URL").unwrap()).await?;
    /// store.migrate().await?;
    /// # let join_handle =
    /// store.spawn_cleanup_task(Duration::from_secs(1));
    /// let mut session = Session::new();
    /// session.expire_in(Duration::from_secs(0));
    /// store.store_session(session).await?;
    /// assert_eq!(store.count().await?, 1);
    /// async_std::task::sleep(Duration::from_secs(2)).await;
    /// assert_eq!(store.count().await?, 0);
    /// # join_handle.cancel().await;
    /// # Ok(()) }) }
    /// ```
    #[cfg(feature = "async_std")]
    pub fn spawn_cleanup_task(
        &self,
        period: std::time::Duration,
    ) -> async_std::task::JoinHandle<()> {
        let store = self.clone();
        async_std::task::spawn(async move {
            loop {
                async_std::task::sleep(period).await;
                if let Err(error) = store.cleanup().await {
                    log::error!("cleanup error: {}", error);
                }
            }
        })
    }

    /// Performs a one-time cleanup task that clears out stale
    /// (expired) sessions. You may want to call this from cron.
    /// ```rust
    /// # use async_sqlx_session::MySqlSessionStore;
    /// # use async_session::{chrono::{Utc,Duration}, Result, SessionStore, Session};
    /// # fn main() -> Result { async_std::task::block_on(async {
    /// let store = MySqlSessionStore::new(&std::env::var("MYSQL_TEST_DB_URL").unwrap()).await?;
    /// store.migrate().await?;
    /// # store.clear_store().await?;
    /// let mut session = Session::new();
    /// session.set_expiry(Utc::now() - Duration::seconds(5));
    /// store.store_session(session).await?;
    /// assert_eq!(store.count().await?, 1);
    /// store.cleanup().await?;
    /// assert_eq!(store.count().await?, 0);
    /// # Ok(()) }) }
    /// ```
    pub async fn cleanup(&self) -> sqlx::Result<()> {
        let mut connection = self.connection().await?;
        sqlx::query(&self.substitute_table_name("DELETE FROM %%TABLE_NAME%% WHERE expires < ?"))
            .bind(Utc::now())
            .execute(&mut connection)
            .await?;

        Ok(())
    }

    /// retrieves the number of sessions currently stored, including
    /// expired sessions
    ///
    /// ```rust
    /// # use async_sqlx_session::MySqlSessionStore;
    /// # use async_session::{Result, SessionStore, Session};
    /// # use std::time::Duration;
    /// # fn main() -> Result { async_std::task::block_on(async {
    /// let store = MySqlSessionStore::new(&std::env::var("MYSQL_TEST_DB_URL").unwrap()).await?;
    /// store.migrate().await?;
    /// # store.clear_store().await?;
    /// assert_eq!(store.count().await?, 0);
    /// store.store_session(Session::new()).await?;
    /// assert_eq!(store.count().await?, 1);
    /// # Ok(()) }) }
    /// ```

    pub async fn count(&self) -> sqlx::Result<i64> {
        let (count,) =
            sqlx::query_as(&self.substitute_table_name("SELECT COUNT(*) FROM %%TABLE_NAME%%"))
                .fetch_one(&mut self.connection().await?)
                .await?;

        Ok(count)
    }
}

#[async_trait]
impl SessionStore for MySqlSessionStore {
    async fn load_session(&self, cookie_value: String) -> Result<Option<Session>> {
        let id = Session::id_from_cookie_value(&cookie_value)?;
        let mut connection = self.connection().await?;

        let result: Option<(String,)> = sqlx::query_as(&self.substitute_table_name(
            "SELECT session FROM %%TABLE_NAME%% WHERE id = ? AND (expires IS NULL OR expires > ?)",
        ))
        .bind(&id)
        .bind(Utc::now())
        .fetch_optional(&mut connection)
        .await?;

        Ok(result
            .map(|(session,)| serde_json::from_str(&session))
            .transpose()?)
    }

    async fn store_session(&self, session: Session) -> Result<Option<String>> {
        let id = session.id();
        let string = serde_json::to_string(&session)?;
        let mut connection = self.connection().await?;

        sqlx::query(&self.substitute_table_name(
            r#"
            INSERT INTO %%TABLE_NAME%%
              (id, session, expires) VALUES(?, ?, ?)
            ON DUPLICATE KEY UPDATE
              expires = VALUES(expires),
              session = VALUES(session)
            "#,
        ))
        .bind(&id)
        .bind(&string)
        .bind(&session.expiry())
        .execute(&mut connection)
        .await?;

        Ok(session.into_cookie_value())
    }

    async fn destroy_session(&self, session: Session) -> Result {
        let id = session.id();
        let mut connection = self.connection().await?;
        sqlx::query(&self.substitute_table_name("DELETE FROM %%TABLE_NAME%% WHERE id = ?"))
            .bind(&id)
            .execute(&mut connection)
            .await?;

        Ok(())
    }

    async fn clear_store(&self) -> Result {
        let mut connection = self.connection().await?;
        sqlx::query(&self.substitute_table_name("TRUNCATE %%TABLE_NAME%%"))
            .execute(&mut connection)
            .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_session::chrono::DateTime;
    use std::time::Duration;

    async fn test_store() -> MySqlSessionStore {
        let store = MySqlSessionStore::new(&std::env::var("MYSQL_TEST_DB_URL").unwrap())
            .await
            .expect("building a MySqlSessionStore");

        store
            .migrate()
            .await
            .expect("migrating a MySqlSessionStore");

        store.clear_store().await.expect("clearing");

        store
    }

    #[async_std::test]
    async fn creating_a_new_session_with_no_expiry() -> Result {
        let store = test_store().await;
        let mut session = Session::new();
        session.insert("key", "value")?;
        let cloned = session.clone();
        let cookie_value = store.store_session(session).await?.unwrap();

        let (id, expires, serialized, count): (String, Option<DateTime<Utc>>, String, i64) =
            sqlx::query_as("select id, expires, session, (select count(*) from async_sessions) from async_sessions")
                .fetch_one(&mut store.connection().await?)
                .await?;

        assert_eq!(1, count);
        assert_eq!(id, cloned.id());
        assert_eq!(expires, None);

        let deserialized_session: Session = serde_json::from_str(&serialized)?;
        assert_eq!(cloned.id(), deserialized_session.id());
        assert_eq!("value", &deserialized_session.get::<String>("key").unwrap());

        let loaded_session = store.load_session(cookie_value).await?.unwrap();
        assert_eq!(cloned.id(), loaded_session.id());
        assert_eq!("value", &loaded_session.get::<String>("key").unwrap());

        assert!(!loaded_session.is_expired());
        Ok(())
    }

    #[async_std::test]
    async fn updating_a_session() -> Result {
        let store = test_store().await;
        let mut session = Session::new();
        let original_id = session.id().to_owned();

        session.insert("key", "value")?;
        let cookie_value = store.store_session(session).await?.unwrap();

        let mut session = store.load_session(cookie_value.clone()).await?.unwrap();
        session.insert("key", "other value")?;
        assert_eq!(None, store.store_session(session).await?);

        let session = store.load_session(cookie_value.clone()).await?.unwrap();
        assert_eq!(session.get::<String>("key").unwrap(), "other value");

        let (id, count): (String, i64) =
            sqlx::query_as("select id, (select count(*) from async_sessions) from async_sessions")
                .fetch_one(&mut store.connection().await?)
                .await?;

        assert_eq!(1, count);
        assert_eq!(original_id, id);

        Ok(())
    }

    #[async_std::test]
    async fn updating_a_session_extending_expiry() -> Result {
        let store = test_store().await;
        let mut session = Session::new();
        session.expire_in(Duration::from_secs(10));
        let original_id = session.id().to_owned();
        let original_expires = session.expiry().unwrap().clone();
        let cookie_value = store.store_session(session).await?.unwrap();

        let mut session = store.load_session(cookie_value.clone()).await?.unwrap();
        assert_eq!(session.expiry().unwrap(), &original_expires);
        session.expire_in(Duration::from_secs(20));
        let new_expires = session.expiry().unwrap().clone();
        store.store_session(session).await?;

        let session = store.load_session(cookie_value.clone()).await?.unwrap();
        assert_eq!(session.expiry().unwrap(), &new_expires);

        let (id, expires, count): (String, DateTime<Utc>, i64) = sqlx::query_as(
            "select id, expires, (select count(*) from async_sessions) from async_sessions",
        )
        .fetch_one(&mut store.connection().await?)
        .await?;

        assert_eq!(1, count);
        assert_eq!(expires.timestamp_millis(), new_expires.timestamp_millis());
        assert_eq!(original_id, id);

        Ok(())
    }

    #[async_std::test]
    async fn creating_a_new_session_with_expiry() -> Result {
        let store = test_store().await;
        let mut session = Session::new();
        session.expire_in(Duration::from_secs(1));
        session.insert("key", "value")?;
        let cloned = session.clone();

        let cookie_value = store.store_session(session).await?.unwrap();

        let (id, expires, serialized, count): (String, Option<DateTime<Utc>>, String, i64) =
            sqlx::query_as("select id, expires, session, (select count(*) from async_sessions) from async_sessions")
                .fetch_one(&mut store.connection().await?)
                .await?;

        assert_eq!(1, count);
        assert_eq!(id, cloned.id());
        assert!(expires.unwrap() > Utc::now());

        let deserialized_session: Session = serde_json::from_str(&serialized)?;
        assert_eq!(cloned.id(), deserialized_session.id());
        assert_eq!("value", &deserialized_session.get::<String>("key").unwrap());

        let loaded_session = store.load_session(cookie_value.clone()).await?.unwrap();
        assert_eq!(cloned.id(), loaded_session.id());
        assert_eq!("value", &loaded_session.get::<String>("key").unwrap());

        assert!(!loaded_session.is_expired());

        async_std::task::sleep(Duration::from_secs(1)).await;
        assert_eq!(None, store.load_session(cookie_value).await?);

        Ok(())
    }

    #[async_std::test]
    async fn destroying_a_single_session() -> Result {
        let store = test_store().await;
        for _ in 0..3i8 {
            store.store_session(Session::new()).await?;
        }

        let cookie = store.store_session(Session::new()).await?.unwrap();
        assert_eq!(4, store.count().await?);
        let session = store.load_session(cookie.clone()).await?.unwrap();
        store.destroy_session(session.clone()).await.unwrap();
        assert_eq!(None, store.load_session(cookie).await?);
        assert_eq!(3, store.count().await?);

        // // attempting to destroy the session again is not an error
        assert!(store.destroy_session(session).await.is_ok());
        Ok(())
    }

    #[async_std::test]
    async fn clearing_the_whole_store() -> Result {
        let store = test_store().await;
        for _ in 0..3i8 {
            store.store_session(Session::new()).await?;
        }

        assert_eq!(3, store.count().await?);
        store.clear_store().await.unwrap();
        assert_eq!(0, store.count().await?);

        Ok(())
    }
}
