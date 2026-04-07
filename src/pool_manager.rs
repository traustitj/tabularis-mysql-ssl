use crate::config;
use crate::models::ConnectionParams;
use once_cell::sync::Lazy;
use sqlx::mysql::{MySqlConnectOptions, MySqlPool, MySqlPoolOptions, MySqlSslMode};
use std::collections::HashMap;
use std::path::Path;
use tokio::sync::RwLock;

static POOLS: Lazy<RwLock<HashMap<String, MySqlPool>>> = Lazy::new(|| RwLock::new(HashMap::new()));

fn effective_ssl_mode(params: &ConnectionParams) -> MySqlSslMode {
    let config = config::load();
    let requested_mode = config
        .ssl_mode
        .as_deref()
        .or(params.ssl_mode.as_deref())
        .unwrap_or("required");

    let mode = match requested_mode {
        "disabled" | "disable" => MySqlSslMode::Disabled,
        "preferred" | "prefer" => MySqlSslMode::Preferred,
        "required" | "require" => MySqlSslMode::Required,
        "verify_identity" => MySqlSslMode::VerifyIdentity,
        _ => MySqlSslMode::VerifyCa,
    };

    if matches!(mode, MySqlSslMode::VerifyCa | MySqlSslMode::VerifyIdentity) && config.ssl_ca.is_none() {
        return MySqlSslMode::Required;
    }

    mode
}

fn build_pool_key(params: &ConnectionParams, override_db: Option<&str>) -> String {
    if let Some(connection_id) = &params.connection_id {
        if let Some(database) = override_db {
            return format!("{connection_id}:{database}");
        }
        return connection_id.clone();
    }

    let config = config::load();
    format!(
        "{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
        params.driver,
        params.host.as_deref().unwrap_or("localhost"),
        params.port.unwrap_or(3306),
        params.username.as_deref().unwrap_or(""),
        params.password.as_deref().unwrap_or(""),
        override_db.unwrap_or_else(|| params.database.primary()),
        params.ssl_mode.as_deref().or(config.ssl_mode.as_deref()).unwrap_or("verify_ca"),
        config.ssl_ca.as_deref().unwrap_or(""),
        config.ssl_cert.as_deref().unwrap_or(""),
        config.ssl_key.as_deref().unwrap_or(""),
    )
}

fn build_options(params: &ConnectionParams, override_db: Option<&str>) -> Result<MySqlConnectOptions, String> {
    let username = params.username.as_deref().unwrap_or_default();
    let password = params.password.as_deref().unwrap_or_default();
    let host = params.host.as_deref().unwrap_or("localhost");
    let port = params.port.unwrap_or(3306);
    let database = override_db.unwrap_or_else(|| params.database.primary());

    let config = config::load();
    let mut options = MySqlConnectOptions::new()
        .host(host)
        .port(port)
        .username(username)
        .password(password)
        .database(database);
    options = options.ssl_mode(effective_ssl_mode(params));

    if let Some(ca) = &config.ssl_ca {
        options = options.ssl_ca(ca);
    }
    if let Some(cert) = &config.ssl_cert {
        options = options.ssl_client_cert(cert);
    }
    if let Some(key) = &config.ssl_key {
        options = options.ssl_client_key(key);
    }

    Ok(options)
}

fn ssl_mode_label(params: &ConnectionParams) -> &'static str {
    match effective_ssl_mode(params) {
        MySqlSslMode::Disabled => "disabled",
        MySqlSslMode::Preferred => "preferred",
        MySqlSslMode::Required => "required",
        MySqlSslMode::VerifyCa => "verify_ca",
        MySqlSslMode::VerifyIdentity => "verify_identity",
    }
}

fn file_status(path: Option<&String>) -> String {
    match path {
        Some(value) => {
            let exists = Path::new(value).exists();
            format!("{} (exists={exists})", value)
        }
        None => "<none>".to_string(),
    }
}

pub async fn has_pool(params: &ConnectionParams, override_db: Option<&str>) -> bool {
    let key = build_pool_key(params, override_db);
    POOLS.read().await.contains_key(&key)
}

pub async fn get_mysql_pool(params: &ConnectionParams) -> Result<MySqlPool, String> {
    get_mysql_pool_for_database(params, None).await
}

pub async fn get_mysql_pool_for_database(
    params: &ConnectionParams,
    override_db: Option<&str>,
) -> Result<MySqlPool, String> {
    let key = build_pool_key(params, override_db);
    if let Some(pool) = POOLS.read().await.get(&key).cloned() {
        return Ok(pool);
    }

    let options = build_options(params, override_db)?;
    let config = config::load();
    let pool = MySqlPoolOptions::new()
        .max_connections(config.max_connections.unwrap_or(5))
        .connect_with(options)
        .await
        .map_err(|e| {
            format!(
                "MySQL connect failed: {} | ssl_mode={} | ssl_ca={} | ssl_cert={} | ssl_key={}",
                e,
                ssl_mode_label(params),
                file_status(config.ssl_ca.as_ref()),
                file_status(config.ssl_cert.as_ref()),
                file_status(config.ssl_key.as_ref()),
            )
        })?;

    let mut guard = POOLS.write().await;
    let entry = guard.entry(key).or_insert_with(|| pool.clone());
    Ok(entry.clone())
}