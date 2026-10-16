use std::future::Future;
use std::mem;
use std::panic::{AssertUnwindSafe, resume_unwind};
use std::sync::{Arc, Mutex};

use futures_util::FutureExt;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

use crate::database::{IsolatedDatabase, create_isolated_database, drop_isolated_database};
use crate::env::require_test_db_url;
use crate::names::unique_role_name;
use crate::roles::{Caller, cleanup_role, create_caller};

#[derive(Default)]
struct Registry {
    databases: Vec<IsolatedDatabase>,
    roles: Vec<String>,
}

#[derive(Clone)]
pub struct TestWorld {
    admin: PgPool,
    url: String,
    registry: Arc<Mutex<Registry>>,
}

impl TestWorld {
    pub fn admin(&self) -> &PgPool {
        &self.admin
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn unique_role(&self) -> String {
        let role = unique_role_name();
        self.register_role(&role);
        role
    }

    pub async fn caller(&self) -> Caller {
        let role = self.unique_role();
        create_caller(&self.admin, &self.url, role).await
    }

    pub async fn isolated_database(&self) -> IsolatedDatabase {
        let database = create_isolated_database(&self.admin, &self.url).await;
        self.registry
            .lock()
            .expect("registry lock")
            .databases
            .push(database.clone());
        database
    }

    pub async fn caller_in(&self, database: &IsolatedDatabase) -> Caller {
        let role = self.unique_role();
        let database_admin = PgPoolOptions::new()
            .max_connections(2)
            .connect(database.url())
            .await
            .expect("connect as admin to the isolated database");
        let caller = create_caller(&database_admin, database.url(), role).await;
        database_admin.close().await;
        caller
    }

    fn register_role(&self, role: &str) {
        self.registry
            .lock()
            .expect("registry lock")
            .roles
            .push(role.to_string());
    }

    async fn teardown(&self) -> Result<(), sqlx::Error> {
        let Registry { databases, roles } = mem::take(&mut *self.registry.lock().expect("lock"));
        let mut first_error = None;
        for database in databases.iter().rev() {
            if let Err(error) = drop_isolated_database(&self.admin, database).await {
                first_error.get_or_insert(error);
            }
        }
        for role in roles.iter().rev() {
            cleanup_role(&self.admin, role).await;
        }
        first_error.map_or(Ok(()), Err)
    }
}

pub async fn with_test_world<T, F, Fut>(body: F) -> T
where
    F: FnOnce(TestWorld) -> Fut,
    Fut: Future<Output = T>,
{
    let url = require_test_db_url();
    let admin = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .expect("connect as admin");
    let world = TestWorld {
        admin,
        url,
        registry: Arc::default(),
    };
    let outcome = AssertUnwindSafe(body(world.clone())).catch_unwind().await;
    let teardown = world.teardown().await;
    world.admin.close().await;
    match outcome {
        Ok(value) => {
            teardown.expect("drop isolated database");
            value
        }
        Err(panic) => resume_unwind(panic),
    }
}
