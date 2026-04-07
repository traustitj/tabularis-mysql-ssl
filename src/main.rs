#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod common;
mod config;
mod models;
mod mysql;
mod pool_manager;
mod rpc;

use models::{ColumnDefinition, ConnectionParams};
use rpc::{JsonRpcError, JsonRpcRequest, JsonRpcResponse};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{self, BufRead, Write};

#[cfg(target_os = "windows")]
fn hide_console_window() {
    use windows_sys::Win32::System::Console::GetConsoleWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};

    unsafe {
        let console = GetConsoleWindow();
        if !console.is_null() {
            ShowWindow(console, SW_HIDE);
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn hide_console_window() {}

fn get_required<'a>(params: &'a Value, key: &str) -> Result<&'a Value, String> {
    params
        .get(key)
        .ok_or_else(|| format!("Missing parameter: {key}"))
}

fn parse_connection_params(params: &Value) -> Result<ConnectionParams, String> {
    let raw = get_required(params, "params")?;
    serde_json::from_value(raw.clone()).map_err(|e| e.to_string())
}

fn parse_value<T: serde::de::DeserializeOwned>(params: &Value, key: &str) -> Result<T, String> {
    serde_json::from_value(get_required(params, key)?.clone()).map_err(|e| e.to_string())
}

fn parse_optional_value<T: serde::de::DeserializeOwned>(params: &Value, key: &str) -> Result<Option<T>, String> {
    match params.get(key) {
        Some(value) if !value.is_null() => serde_json::from_value(value.clone())
            .map(Some)
            .map_err(|e| e.to_string()),
        _ => Ok(None),
    }
}

fn send_response(response: &JsonRpcResponse) {
    println!("{}", serde_json::to_string(response).unwrap_or_else(|_| {
        "{\"jsonrpc\":\"2.0\",\"error\":{\"code\":-32603,\"message\":\"Failed to serialize response\"},\"id\":null}".to_string()
    }));
    let _ = io::stdout().flush();
}

fn success(id: Value, result: Value) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0",
        id,
        result: Some(result),
        error: None,
    }
}

fn failure(id: Value, code: i32, message: impl Into<String>) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0",
        id,
        result: None,
        error: Some(JsonRpcError {
            code,
            message: message.into(),
        }),
    }
}

async fn dispatch(method: &str, params: Value) -> Result<Value, String> {
    match method {
        "initialize" => {
            let settings = params.get("settings").cloned().unwrap_or(Value::Null);
            config::apply_initialize_settings(&settings)?;
            Ok(Value::Null)
        }
        "ping" => {
            let conn = parse_connection_params(&params)?;
            mysql::ping(&conn).await?;
            Ok(Value::Null)
        }
        "test_connection" => {
            let conn = parse_connection_params(&params)?;
            mysql::test_connection(&conn).await?;
            Ok(json!({ "success": true }))
        }
        "get_databases" => {
            let conn = parse_connection_params(&params)?;
            Ok(serde_json::to_value(mysql::get_databases(&conn).await?).map_err(|e| e.to_string())?)
        }
        "get_schemas" => {
            let conn = parse_connection_params(&params)?;
            Ok(serde_json::to_value(mysql::get_schemas(&conn).await?).map_err(|e| e.to_string())?)
        }
        "get_tables" => {
            let conn = parse_connection_params(&params)?;
            let schema = parse_optional_value::<String>(&params, "schema")?;
            Ok(serde_json::to_value(mysql::get_tables(&conn, schema.as_deref()).await?).map_err(|e| e.to_string())?)
        }
        "get_columns" => {
            let conn = parse_connection_params(&params)?;
            let table = parse_value::<String>(&params, "table")?;
            let schema = parse_optional_value::<String>(&params, "schema")?;
            Ok(serde_json::to_value(mysql::get_columns(&conn, &table, schema.as_deref()).await?).map_err(|e| e.to_string())?)
        }
        "get_foreign_keys" => {
            let conn = parse_connection_params(&params)?;
            let table = parse_value::<String>(&params, "table")?;
            let schema = parse_optional_value::<String>(&params, "schema")?;
            Ok(serde_json::to_value(mysql::get_foreign_keys(&conn, &table, schema.as_deref()).await?).map_err(|e| e.to_string())?)
        }
        "get_indexes" => {
            let conn = parse_connection_params(&params)?;
            let table = parse_value::<String>(&params, "table")?;
            let schema = parse_optional_value::<String>(&params, "schema")?;
            Ok(serde_json::to_value(mysql::get_indexes(&conn, &table, schema.as_deref()).await?).map_err(|e| e.to_string())?)
        }
        "get_views" => {
            let conn = parse_connection_params(&params)?;
            let schema = parse_optional_value::<String>(&params, "schema")?;
            Ok(serde_json::to_value(mysql::get_views(&conn, schema.as_deref()).await?).map_err(|e| e.to_string())?)
        }
        "get_view_definition" => {
            let conn = parse_connection_params(&params)?;
            let view_name = parse_value::<String>(&params, "view_name")?;
            Ok(json!(mysql::get_view_definition(&conn, &view_name).await?))
        }
        "get_view_columns" => {
            let conn = parse_connection_params(&params)?;
            let view_name = parse_value::<String>(&params, "view_name")?;
            let schema = parse_optional_value::<String>(&params, "schema")?;
            Ok(serde_json::to_value(mysql::get_view_columns(&conn, &view_name, schema.as_deref()).await?).map_err(|e| e.to_string())?)
        }
        "create_view" => {
            let conn = parse_connection_params(&params)?;
            let view_name = parse_value::<String>(&params, "view_name")?;
            let definition = parse_value::<String>(&params, "definition")?;
            mysql::create_view(&conn, &view_name, &definition).await?;
            Ok(Value::Null)
        }
        "alter_view" => {
            let conn = parse_connection_params(&params)?;
            let view_name = parse_value::<String>(&params, "view_name")?;
            let definition = parse_value::<String>(&params, "definition")?;
            mysql::alter_view(&conn, &view_name, &definition).await?;
            Ok(Value::Null)
        }
        "drop_view" => {
            let conn = parse_connection_params(&params)?;
            let view_name = parse_value::<String>(&params, "view_name")?;
            mysql::drop_view(&conn, &view_name).await?;
            Ok(Value::Null)
        }
        "get_routines" => {
            let conn = parse_connection_params(&params)?;
            let schema = parse_optional_value::<String>(&params, "schema")?;
            Ok(serde_json::to_value(mysql::get_routines(&conn, schema.as_deref()).await?).map_err(|e| e.to_string())?)
        }
        "get_routine_parameters" => {
            let conn = parse_connection_params(&params)?;
            let routine_name = parse_value::<String>(&params, "routine_name")?;
            let schema = parse_optional_value::<String>(&params, "schema")?;
            Ok(serde_json::to_value(mysql::get_routine_parameters(&conn, &routine_name, schema.as_deref()).await?).map_err(|e| e.to_string())?)
        }
        "get_routine_definition" => {
            let conn = parse_connection_params(&params)?;
            let routine_name = parse_value::<String>(&params, "routine_name")?;
            let routine_type = parse_value::<String>(&params, "routine_type")?;
            Ok(json!(mysql::get_routine_definition(&conn, &routine_name, &routine_type).await?))
        }
        "execute_query" => {
            let conn = parse_connection_params(&params)?;
            let query = parse_value::<String>(&params, "query")?;
            let limit = parse_optional_value::<u32>(&params, "limit")?;
            let page = parse_optional_value::<u32>(&params, "page")?.unwrap_or(1);
            let schema = parse_optional_value::<String>(&params, "schema")?;
            Ok(serde_json::to_value(mysql::execute_query(&conn, &query, limit, page, schema.as_deref()).await?).map_err(|e| e.to_string())?)
        }
        "insert_record" | "insert_row" => {
            let conn = parse_connection_params(&params)?;
            let table = parse_value::<String>(&params, "table")?;
            let data = parse_value::<HashMap<String, Value>>(&params, "data")?;
            let max_blob_size = parse_optional_value::<u64>(&params, "max_blob_size")?.unwrap_or_else(config::max_blob_size);
            Ok(json!(mysql::insert_record(&conn, &table, data, max_blob_size).await?))
        }
        "update_record" | "update_row" => {
            let conn = parse_connection_params(&params)?;
            let table = parse_value::<String>(&params, "table")?;
            let pk_col = parse_value::<String>(&params, "pk_col")?;
            let pk_val = get_required(&params, "pk_val")?.clone();
            let col_name = parse_value::<String>(&params, "col_name")?;
            let new_val = get_required(&params, "new_val")?.clone();
            let max_blob_size = parse_optional_value::<u64>(&params, "max_blob_size")?.unwrap_or_else(config::max_blob_size);
            Ok(json!(mysql::update_record(&conn, &table, &pk_col, pk_val, &col_name, new_val, max_blob_size).await?))
        }
        "delete_record" | "delete_row" => {
            let conn = parse_connection_params(&params)?;
            let table = parse_value::<String>(&params, "table")?;
            let pk_col = parse_value::<String>(&params, "pk_col")?;
            let pk_val = get_required(&params, "pk_val")?.clone();
            Ok(json!(mysql::delete_record(&conn, &table, &pk_col, pk_val).await?))
        }
        "save_blob_to_file" => {
            let conn = parse_connection_params(&params)?;
            let table = parse_value::<String>(&params, "table")?;
            let col_name = parse_value::<String>(&params, "col_name")?;
            let pk_col = parse_value::<String>(&params, "pk_col")?;
            let pk_val = get_required(&params, "pk_val")?.clone();
            let file_path = parse_value::<String>(&params, "file_path")?;
            mysql::save_blob_column_to_file(&conn, &table, &col_name, &pk_col, pk_val, &file_path).await?;
            Ok(Value::Null)
        }
        "fetch_blob_as_data_url" => {
            let conn = parse_connection_params(&params)?;
            let table = parse_value::<String>(&params, "table")?;
            let col_name = parse_value::<String>(&params, "col_name")?;
            let pk_col = parse_value::<String>(&params, "pk_col")?;
            let pk_val = get_required(&params, "pk_val")?.clone();
            Ok(json!(mysql::fetch_blob_column_as_data_url(&conn, &table, &col_name, &pk_col, pk_val).await?))
        }
        "get_create_table_sql" => {
            let table_name = parse_value::<String>(&params, "table_name")?;
            let columns = parse_value::<Vec<ColumnDefinition>>(&params, "columns")?;
            Ok(json!(mysql::get_create_table_sql(&table_name, columns)?))
        }
        "get_add_column_sql" => {
            let table = parse_value::<String>(&params, "table")?;
            let column = parse_value::<ColumnDefinition>(&params, "column")?;
            Ok(json!(mysql::get_add_column_sql(&table, column)?))
        }
        "get_alter_column_sql" => {
            let table = parse_value::<String>(&params, "table")?;
            let old_column = parse_value::<ColumnDefinition>(&params, "old_column")?;
            let new_column = parse_value::<ColumnDefinition>(&params, "new_column")?;
            Ok(json!(mysql::get_alter_column_sql(&table, old_column, new_column)?))
        }
        "get_create_index_sql" => {
            let table = parse_value::<String>(&params, "table")?;
            let index_name = parse_value::<String>(&params, "index_name")?;
            let columns = parse_value::<Vec<String>>(&params, "columns")?;
            let is_unique = parse_optional_value::<bool>(&params, "is_unique")?.unwrap_or(false);
            Ok(json!(mysql::get_create_index_sql(&table, &index_name, columns, is_unique)?))
        }
        "get_create_foreign_key_sql" => {
            let table = parse_value::<String>(&params, "table")?;
            let fk_name = parse_value::<String>(&params, "fk_name")?;
            let column = parse_value::<String>(&params, "column")?;
            let ref_table = parse_value::<String>(&params, "ref_table")?;
            let ref_column = parse_value::<String>(&params, "ref_column")?;
            let on_delete = parse_optional_value::<String>(&params, "on_delete")?;
            let on_update = parse_optional_value::<String>(&params, "on_update")?;
            Ok(json!(mysql::get_create_foreign_key_sql(
                &table,
                &fk_name,
                &column,
                &ref_table,
                &ref_column,
                on_delete.as_deref(),
                on_update.as_deref(),
            )?))
        }
        "drop_index" => {
            let conn = parse_connection_params(&params)?;
            let table = parse_value::<String>(&params, "table")?;
            let index_name = parse_value::<String>(&params, "index_name")?;
            mysql::drop_index(&conn, &table, &index_name).await?;
            Ok(Value::Null)
        }
        "drop_foreign_key" => {
            let conn = parse_connection_params(&params)?;
            let table = parse_value::<String>(&params, "table")?;
            let fk_name = parse_value::<String>(&params, "fk_name")?;
            mysql::drop_foreign_key(&conn, &table, &fk_name).await?;
            Ok(Value::Null)
        }
        "get_schema_snapshot" => {
            let conn = parse_connection_params(&params)?;
            let schema = parse_optional_value::<String>(&params, "schema")?;
            Ok(json!(mysql::get_schema_snapshot(&conn, schema.as_deref()).await?))
        }
        "get_all_columns_batch" => {
            let conn = parse_connection_params(&params)?;
            let schema = parse_optional_value::<String>(&params, "schema")?;
            Ok(json!(mysql::get_all_columns_batch(&conn, schema.as_deref()).await?))
        }
        "get_all_foreign_keys_batch" => {
            let conn = parse_connection_params(&params)?;
            let schema = parse_optional_value::<String>(&params, "schema")?;
            Ok(json!(mysql::get_all_foreign_keys_batch(&conn, schema.as_deref()).await?))
        }
        _ => Err(format!("Method '{method}' not implemented")),
    }
}

#[tokio::main]
async fn main() {
    hide_console_window();

    let stdin = io::stdin();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(line) => line,
            Err(err) => {
                eprintln!("Failed to read stdin: {err}");
                continue;
            }
        };

        if line.trim().is_empty() {
            continue;
        }

        let request: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(request) => request,
            Err(err) => {
                send_response(&failure(Value::Null, -32700, format!("Parse error: {err}")));
                continue;
            }
        };

        if request.jsonrpc != "2.0" {
            send_response(&failure(request.id, -32600, "Invalid JSON-RPC version"));
            continue;
        }

        let response = match dispatch(&request.method, request.params).await {
            Ok(result) => success(request.id, result),
            Err(message) => {
                let code = if message.contains("Missing parameter") { -32602 } else if message.contains("not implemented") { -32601 } else { -32603 };
                failure(request.id, code, message)
            }
        };

        send_response(&response);
    }
}
