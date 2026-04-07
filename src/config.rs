use once_cell::sync::{Lazy, OnceCell};
use serde::Deserialize;
use serde_json::Value;
use std::sync::RwLock;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize, Default)]
pub struct PluginConfig {
    pub ssl_ca: Option<String>,
    pub ssl_cert: Option<String>,
    pub ssl_key: Option<String>,
    pub ssl_mode: Option<String>,
    pub max_connections: Option<u32>,
    pub max_blob_size: Option<u64>,
}

static FILE_CONFIG: OnceCell<PluginConfig> = OnceCell::new();
static RUNTIME_CONFIG: Lazy<RwLock<PluginConfig>> = Lazy::new(|| RwLock::new(PluginConfig::default()));

fn default_config_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    Some(dir.join("mysqlssl-plugin.config.json"))
}

fn resolve_path(base_dir: &Path, value: Option<String>) -> Option<String> {
    value.map(|raw| {
        let path = PathBuf::from(raw);
        let resolved = if path.is_absolute() { path } else { base_dir.join(path) };
        resolved.to_string_lossy().to_string()
    })
}

fn executable_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.parent().map(Path::to_path_buf)
}

fn normalize_paths(mut config: PluginConfig, base_dir: &Path) -> PluginConfig {
    config.ssl_ca = resolve_path(base_dir, config.ssl_ca);
    config.ssl_cert = resolve_path(base_dir, config.ssl_cert);
    config.ssl_key = resolve_path(base_dir, config.ssl_key);
    config
}

fn load_file_config() -> &'static PluginConfig {
    FILE_CONFIG.get_or_init(|| {
        let path = std::env::var("MYSQLSSL_PLUGIN_CONFIG")
            .ok()
            .map(PathBuf::from)
            .or_else(default_config_path);

        let Some(path) = path else {
            return PluginConfig::default();
        };

        if !path.exists() {
            return PluginConfig::default();
        }

        let raw = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(err) => {
                eprintln!("Failed to read config file {}: {err}", path.display());
                return PluginConfig::default();
            }
        };

        let parsed = match serde_json::from_str::<PluginConfig>(&raw) {
            Ok(config) => config,
            Err(err) => {
                eprintln!("Failed to parse config file {}: {err}", path.display());
                return PluginConfig::default();
            }
        };

        let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
        normalize_paths(parsed, base_dir)
    })
}

fn merge_config(file: &PluginConfig, runtime: &PluginConfig) -> PluginConfig {
    PluginConfig {
        ssl_ca: runtime.ssl_ca.clone().or_else(|| file.ssl_ca.clone()),
        ssl_cert: runtime.ssl_cert.clone().or_else(|| file.ssl_cert.clone()),
        ssl_key: runtime.ssl_key.clone().or_else(|| file.ssl_key.clone()),
        ssl_mode: runtime.ssl_mode.clone().or_else(|| file.ssl_mode.clone()),
        max_connections: runtime.max_connections.or(file.max_connections),
        max_blob_size: runtime.max_blob_size.or(file.max_blob_size),
    }
}

pub fn apply_initialize_settings(settings: &Value) -> Result<(), String> {
    let parsed = if settings.is_null() {
        PluginConfig::default()
    } else {
        serde_json::from_value::<PluginConfig>(settings.clone()).map_err(|err| err.to_string())?
    };

    let base_dir = executable_dir().unwrap_or_else(|| PathBuf::from("."));
    let normalized = normalize_paths(parsed, &base_dir);
    let mut guard = RUNTIME_CONFIG.write().map_err(|_| "Runtime config lock poisoned".to_string())?;
    *guard = normalized;
    Ok(())
}

pub fn load() -> PluginConfig {
    let runtime = RUNTIME_CONFIG
        .read()
        .map(|guard| guard.clone())
        .unwrap_or_default();
    merge_config(load_file_config(), &runtime)
}

pub fn max_blob_size() -> u64 {
    load().max_blob_size.unwrap_or(crate::common::DEFAULT_MAX_BLOB_SIZE)
}