use sqlx::PgPool;

use crate::names::{assert_plain_identifier, unique_role_name};

#[derive(Clone)]
pub struct IsolatedDatabase {
    name: String,
    url: String,
}

impl IsolatedDatabase {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn url(&self) -> &str {
        &self.url
    }
}

pub async fn create_isolated_database(admin: &PgPool, admin_url: &str) -> IsolatedDatabase {
    let name = unique_role_name();
    assert_plain_identifier(&name);
    let mut url = url::Url::parse(admin_url).expect("TEST_DATABASE_URL must parse as a URL");
    url.set_path(&format!("/{name}"));
    sqlx::query(&format!("CREATE DATABASE \"{name}\""))
        .execute(admin)
        .await
        .expect("create isolated database");
    IsolatedDatabase {
        name,
        url: url.to_string(),
    }
}

pub async fn drop_isolated_database(
    admin: &PgPool,
    database: &IsolatedDatabase,
) -> Result<(), sqlx::Error> {
    assert_plain_identifier(&database.name);
    sqlx::query(&format!(
        "DROP DATABASE IF EXISTS \"{}\" WITH (FORCE)",
        database.name
    ))
    .execute(admin)
    .await
    .map(|_| ())
}
