pub mod extract;

use crate::common::{decode_blob_wire_format, encode_blob_full};
use crate::models::{
    ColumnDefinition, ConnectionParams, DatabaseSelection, ForeignKey, Index, Pagination,
    QueryResult, RoutineInfo, RoutineParameter, TableColumn, TableInfo, TableSchema, ViewInfo,
};
use crate::pool_manager::{get_mysql_pool, get_mysql_pool_for_database, has_pool};
use extract::extract_value;
use sqlx::{Column, Row};
use std::collections::HashMap;

fn escape_identifier(name: &str) -> String {
    name.replace('`', "``")
}

fn mysql_row_str(row: &sqlx::mysql::MySqlRow, idx: usize) -> String {
    row.try_get::<String, _>(idx).unwrap_or_else(|_| {
        row.try_get::<Vec<u8>, _>(idx)
            .map(|bytes| String::from_utf8_lossy(&bytes).to_string())
            .unwrap_or_default()
    })
}

fn mysql_row_str_opt(row: &sqlx::mysql::MySqlRow, idx: usize) -> Option<String> {
    match row.try_get::<Option<String>, _>(idx) {
        Ok(value) => value,
        Err(_) => row
            .try_get::<Option<Vec<u8>>, _>(idx)
            .ok()
            .flatten()
            .map(|bytes| String::from_utf8_lossy(&bytes).to_string()),
    }
}

pub async fn ping(params: &ConnectionParams) -> Result<(), String> {
    if !has_pool(params, None).await {
        return Err("No active connection pool".into());
    }
    let pool = get_mysql_pool(params).await?;
    let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;
    sqlx::Connection::ping(&mut *conn).await.map_err(|e| e.to_string())
}

pub async fn test_connection(params: &ConnectionParams) -> Result<(), String> {
    let pool = get_mysql_pool(params).await?;
    let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;
    sqlx::Connection::ping(&mut *conn).await.map_err(|e| e.to_string())
}

pub async fn get_schemas(_params: &ConnectionParams) -> Result<Vec<String>, String> {
    Ok(vec![])
}

pub async fn get_databases(params: &ConnectionParams) -> Result<Vec<String>, String> {
    let mut info_params = params.clone();
    info_params.database = DatabaseSelection::Single("information_schema".to_string());
    info_params.connection_id = None;
    let pool = get_mysql_pool(&info_params).await?;
    let rows = sqlx::query("SHOW DATABASES")
        .fetch_all(&pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(rows.iter().map(|row| mysql_row_str(row, 0)).collect())
}

pub async fn get_tables(params: &ConnectionParams, schema: Option<&str>) -> Result<Vec<TableInfo>, String> {
    let db_name = schema.unwrap_or_else(|| params.database.primary());
    let pool = get_mysql_pool_for_database(params, Some(db_name)).await?;
    let rows = sqlx::query(
        "SELECT table_name as name FROM information_schema.tables WHERE table_schema = ? AND table_type = 'BASE TABLE' ORDER BY table_name ASC",
    )
    .bind(db_name)
    .fetch_all(&pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(rows.iter().map(|row| TableInfo { name: mysql_row_str(row, 0) }).collect())
}

pub async fn get_columns(
    params: &ConnectionParams,
    table_name: &str,
    schema: Option<&str>,
) -> Result<Vec<TableColumn>, String> {
    let db_name = schema.unwrap_or_else(|| params.database.primary());
    let pool = get_mysql_pool_for_database(params, Some(db_name)).await?;
    let rows = sqlx::query(
        r#"
        SELECT column_name, data_type, column_key, is_nullable, extra, column_default, character_maximum_length
        FROM information_schema.columns
        WHERE table_schema = ? AND table_name = ?
        ORDER BY ordinal_position
    "#,
    )
    .bind(db_name)
    .bind(table_name)
    .fetch_all(&pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(rows
        .iter()
        .map(|row| {
            let extra = mysql_row_str(row, 4);
            let default_val = mysql_row_str_opt(row, 5);
            let is_auto_increment = extra.contains("auto_increment");

            TableColumn {
                name: mysql_row_str(row, 0),
                data_type: mysql_row_str(row, 1),
                is_pk: mysql_row_str(row, 2) == "PRI",
                is_nullable: mysql_row_str(row, 3) == "YES",
                is_auto_increment,
                default_value: if is_auto_increment {
                    None
                } else {
                    match default_val {
                        Some(value) if !value.is_empty() && !value.eq_ignore_ascii_case("null") => Some(value),
                        _ => None,
                    }
                },
                character_maximum_length: row.try_get(6).ok(),
            }
        })
        .collect())
}

pub async fn get_foreign_keys(
    params: &ConnectionParams,
    table_name: &str,
    schema: Option<&str>,
) -> Result<Vec<ForeignKey>, String> {
    let db_name = schema.unwrap_or_else(|| params.database.primary());
    let pool = get_mysql_pool_for_database(params, Some(db_name)).await?;
    let rows = sqlx::query(
        r#"
        SELECT
            kcu.CONSTRAINT_NAME,
            kcu.COLUMN_NAME,
            kcu.REFERENCED_TABLE_NAME,
            kcu.REFERENCED_COLUMN_NAME,
            rc.UPDATE_RULE,
            rc.DELETE_RULE
        FROM information_schema.KEY_COLUMN_USAGE kcu
        JOIN information_schema.REFERENTIAL_CONSTRAINTS rc
            ON kcu.CONSTRAINT_NAME = rc.CONSTRAINT_NAME
            AND kcu.CONSTRAINT_SCHEMA = rc.CONSTRAINT_SCHEMA
        WHERE kcu.TABLE_SCHEMA = ?
            AND kcu.TABLE_NAME = ?
            AND kcu.REFERENCED_TABLE_NAME IS NOT NULL
        ORDER BY kcu.CONSTRAINT_NAME, kcu.ORDINAL_POSITION
    "#,
    )
    .bind(db_name)
    .bind(table_name)
    .fetch_all(&pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(rows
        .iter()
        .map(|row| ForeignKey {
            name: mysql_row_str(row, 0),
            column_name: mysql_row_str(row, 1),
            ref_table: mysql_row_str(row, 2),
            ref_column: mysql_row_str(row, 3),
            on_update: mysql_row_str_opt(row, 4),
            on_delete: mysql_row_str_opt(row, 5),
        })
        .collect())
}

pub async fn get_all_columns_batch(
    params: &ConnectionParams,
    schema: Option<&str>,
) -> Result<HashMap<String, Vec<TableColumn>>, String> {
    let db_name = schema.unwrap_or_else(|| params.database.primary());
    let pool = get_mysql_pool_for_database(params, Some(db_name)).await?;
    let rows = sqlx::query(
        r#"
        SELECT table_name, column_name, data_type, column_key, is_nullable, extra, column_default, character_maximum_length
        FROM information_schema.columns
        WHERE table_schema = ?
        ORDER BY table_name, ordinal_position
    "#,
    )
    .bind(db_name)
    .fetch_all(&pool)
    .await
    .map_err(|e| e.to_string())?;

    let mut result = HashMap::new();
    for row in &rows {
        let extra = mysql_row_str(row, 5);
        let default_val = mysql_row_str_opt(row, 6);
        let is_auto_increment = extra.contains("auto_increment");
        let column = TableColumn {
            name: mysql_row_str(row, 1),
            data_type: mysql_row_str(row, 2),
            is_pk: mysql_row_str(row, 3) == "PRI",
            is_nullable: mysql_row_str(row, 4) == "YES",
            is_auto_increment,
            default_value: if is_auto_increment {
                None
            } else {
                match default_val {
                    Some(value) if !value.is_empty() && !value.eq_ignore_ascii_case("null") => Some(value),
                    _ => None,
                }
            },
            character_maximum_length: row.try_get(7).ok(),
        };
        result.entry(mysql_row_str(row, 0)).or_insert_with(Vec::new).push(column);
    }
    Ok(result)
}

pub async fn get_all_foreign_keys_batch(
    params: &ConnectionParams,
    schema: Option<&str>,
) -> Result<HashMap<String, Vec<ForeignKey>>, String> {
    let db_name = schema.unwrap_or_else(|| params.database.primary());
    let pool = get_mysql_pool_for_database(params, Some(db_name)).await?;
    let rows = sqlx::query(
        r#"
        SELECT
            kcu.TABLE_NAME,
            kcu.CONSTRAINT_NAME,
            kcu.COLUMN_NAME,
            kcu.REFERENCED_TABLE_NAME,
            kcu.REFERENCED_COLUMN_NAME,
            rc.UPDATE_RULE,
            rc.DELETE_RULE
        FROM information_schema.KEY_COLUMN_USAGE kcu
        JOIN information_schema.REFERENTIAL_CONSTRAINTS rc
            ON kcu.CONSTRAINT_NAME = rc.CONSTRAINT_NAME
            AND kcu.CONSTRAINT_SCHEMA = rc.CONSTRAINT_SCHEMA
        WHERE kcu.TABLE_SCHEMA = ?
            AND kcu.REFERENCED_TABLE_NAME IS NOT NULL
        ORDER BY kcu.TABLE_NAME, kcu.CONSTRAINT_NAME, kcu.ORDINAL_POSITION
    "#,
    )
    .bind(db_name)
    .fetch_all(&pool)
    .await
    .map_err(|e| e.to_string())?;

    let mut result = HashMap::new();
    for row in &rows {
        let fk = ForeignKey {
            name: mysql_row_str(row, 1),
            column_name: mysql_row_str(row, 2),
            ref_table: mysql_row_str(row, 3),
            ref_column: mysql_row_str(row, 4),
            on_update: mysql_row_str_opt(row, 5),
            on_delete: mysql_row_str_opt(row, 6),
        };
        result.entry(mysql_row_str(row, 0)).or_insert_with(Vec::new).push(fk);
    }
    Ok(result)
}

pub async fn get_indexes(
    params: &ConnectionParams,
    table_name: &str,
    schema: Option<&str>,
) -> Result<Vec<Index>, String> {
    let db_name = schema.unwrap_or_else(|| params.database.primary());
    let pool = get_mysql_pool_for_database(params, Some(db_name)).await?;
    let rows = sqlx::query(
        r#"
        SELECT INDEX_NAME, COLUMN_NAME, NON_UNIQUE, SEQ_IN_INDEX
        FROM information_schema.STATISTICS
        WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?
        ORDER BY INDEX_NAME, SEQ_IN_INDEX
    "#,
    )
    .bind(db_name)
    .bind(table_name)
    .fetch_all(&pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(rows
        .iter()
        .map(|row| {
            let index_name = mysql_row_str(row, 0);
            let non_unique: i64 = row.try_get(2).unwrap_or(1);
            Index {
                name: index_name.clone(),
                column_name: mysql_row_str(row, 1),
                is_unique: non_unique == 0,
                is_primary: index_name == "PRIMARY",
                seq_in_index: row.try_get::<i64, _>(3).unwrap_or(0) as i32,
            }
        })
        .collect())
}

pub async fn save_blob_column_to_file(
    params: &ConnectionParams,
    table: &str,
    col_name: &str,
    pk_col: &str,
    pk_val: serde_json::Value,
    file_path: &str,
) -> Result<(), String> {
    let pool = get_mysql_pool(params).await?;
    let query = format!("SELECT `{}` FROM `{}` WHERE `{}` = ?", col_name, table, pk_col);
    let row = match pk_val {
        serde_json::Value::Number(number) => {
            if number.is_i64() {
                sqlx::query(&query).bind(number.as_i64()).fetch_one(&pool).await
            } else if number.is_f64() {
                sqlx::query(&query).bind(number.as_f64()).fetch_one(&pool).await
            } else {
                sqlx::query(&query).bind(number.to_string()).fetch_one(&pool).await
            }
        }
        serde_json::Value::String(value) => sqlx::query(&query).bind(value).fetch_one(&pool).await,
        _ => return Err("Unsupported PK type".into()),
    }
    .map_err(|e| e.to_string())?;

    let bytes: Vec<u8> = row.try_get(0).map_err(|e| e.to_string())?;
    std::fs::write(file_path, bytes).map_err(|e| e.to_string())
}

pub async fn fetch_blob_column_as_data_url(
    params: &ConnectionParams,
    table: &str,
    col_name: &str,
    pk_col: &str,
    pk_val: serde_json::Value,
) -> Result<String, String> {
    let pool = get_mysql_pool(params).await?;
    let query = format!("SELECT `{}` FROM `{}` WHERE `{}` = ?", col_name, table, pk_col);
    let row = match pk_val {
        serde_json::Value::Number(number) => {
            if number.is_i64() {
                sqlx::query(&query).bind(number.as_i64()).fetch_one(&pool).await
            } else if number.is_f64() {
                sqlx::query(&query).bind(number.as_f64()).fetch_one(&pool).await
            } else {
                sqlx::query(&query).bind(number.to_string()).fetch_one(&pool).await
            }
        }
        serde_json::Value::String(value) => sqlx::query(&query).bind(value).fetch_one(&pool).await,
        _ => return Err("Unsupported PK type".into()),
    }
    .map_err(|e| e.to_string())?;

    let bytes: Vec<u8> = row.try_get(0).map_err(|e| e.to_string())?;
    Ok(encode_blob_full(&bytes))
}

pub async fn delete_record(
    params: &ConnectionParams,
    table: &str,
    pk_col: &str,
    pk_val: serde_json::Value,
) -> Result<u64, String> {
    let pool = get_mysql_pool(params).await?;
    let query = format!("DELETE FROM `{}` WHERE `{}` = ?", table, pk_col);
    let result = match pk_val {
        serde_json::Value::Number(number) => {
            if number.is_i64() {
                sqlx::query(&query).bind(number.as_i64()).execute(&pool).await
            } else if number.is_f64() {
                sqlx::query(&query).bind(number.as_f64()).execute(&pool).await
            } else {
                sqlx::query(&query).bind(number.to_string()).execute(&pool).await
            }
        }
        serde_json::Value::String(value) => sqlx::query(&query).bind(value).execute(&pool).await,
        _ => return Err("Unsupported PK type".into()),
    };

    result.map(|value| value.rows_affected()).map_err(|e| e.to_string())
}

fn is_wkt_geometry(value: &str) -> bool {
    let upper = value.trim().to_uppercase();
    upper.starts_with("POINT(")
        || upper.starts_with("LINESTRING(")
        || upper.starts_with("POLYGON(")
        || upper.starts_with("MULTIPOINT(")
        || upper.starts_with("MULTILINESTRING(")
        || upper.starts_with("MULTIPOLYGON(")
        || upper.starts_with("GEOMETRYCOLLECTION(")
        || upper.starts_with("GEOMETRY(")
}

fn is_raw_sql_function(value: &str) -> bool {
    let upper = value.trim().to_uppercase();
    if upper.starts_with("ST_") {
        return upper.contains('(');
    }
    upper.starts_with("GEOMFROMTEXT(")
        || upper.starts_with("GEOMFROMWKB(")
        || upper.starts_with("POINTFROMTEXT(")
        || upper.starts_with("POINTFROMWKB(")
}

pub async fn update_record(
    params: &ConnectionParams,
    table: &str,
    pk_col: &str,
    pk_val: serde_json::Value,
    col_name: &str,
    new_val: serde_json::Value,
    max_blob_size: u64,
) -> Result<u64, String> {
    let pool = get_mysql_pool(params).await?;
    let mut builder = sqlx::QueryBuilder::new(format!("UPDATE `{}` SET `{}` = ", table, col_name));

    match new_val {
        serde_json::Value::Number(number) => {
            if number.is_i64() {
                builder.push_bind(number.as_i64());
            } else {
                builder.push_bind(number.as_f64());
            }
        }
        serde_json::Value::String(value) => {
            if value == "__USE_DEFAULT__" {
                builder.push("DEFAULT");
            } else if let Some(bytes) = decode_blob_wire_format(&value, max_blob_size) {
                builder.push_bind(bytes);
            } else if is_raw_sql_function(&value) {
                builder.push(value);
            } else if is_wkt_geometry(&value) {
                builder.push("ST_GeomFromText(");
                builder.push_bind(value);
                builder.push(")");
            } else {
                builder.push_bind(value);
            }
        }
        serde_json::Value::Bool(value) => {
            builder.push_bind(value);
        }
        serde_json::Value::Null => {
            builder.push("NULL");
        }
        serde_json::Value::Object(_) | serde_json::Value::Array(_) => {
            let json_string = serde_json::to_string(&new_val).map_err(|e| e.to_string())?;
            builder.push("CAST(");
            builder.push_bind(json_string);
            builder.push(" AS JSON)");
        }
    }

    builder.push(format!(" WHERE `{}` = ", pk_col));
    match pk_val {
        serde_json::Value::Number(number) => {
            if number.is_i64() {
                builder.push_bind(number.as_i64());
            } else {
                builder.push_bind(number.as_f64());
            }
        }
        serde_json::Value::String(value) => {
            builder.push_bind(value);
        }
        _ => return Err("Unsupported PK type".into()),
    }

    let result = builder.build().execute(&pool).await.map_err(|e| e.to_string())?;
    Ok(result.rows_affected())
}

pub async fn insert_record(
    params: &ConnectionParams,
    table: &str,
    data: HashMap<String, serde_json::Value>,
    max_blob_size: u64,
) -> Result<u64, String> {
    let pool = get_mysql_pool(params).await?;

    let mut columns = Vec::new();
    let mut values = Vec::new();
    for (key, value) in data {
        columns.push(format!("`{}`", key));
        values.push(value);
    }

    let mut builder = if columns.is_empty() {
        sqlx::QueryBuilder::new(format!("INSERT INTO `{}` () VALUES ()", table))
    } else {
        let mut builder = sqlx::QueryBuilder::new(format!(
            "INSERT INTO `{}` ({}) VALUES (",
            table,
            columns.join(", ")
        ));
        let mut separated = builder.separated(", ");
        for value in values {
            match value {
                serde_json::Value::Number(number) => {
                    if number.is_i64() {
                        separated.push_bind(number.as_i64());
                    } else {
                        separated.push_bind(number.as_f64());
                    }
                }
                serde_json::Value::String(text) => {
                    if let Some(bytes) = decode_blob_wire_format(&text, max_blob_size) {
                        separated.push_bind(bytes);
                    } else if is_raw_sql_function(&text) {
                        separated.push_unseparated(&text);
                    } else if is_wkt_geometry(&text) {
                        separated.push_unseparated("ST_GeomFromText(");
                        separated.push_bind_unseparated(text);
                        separated.push_unseparated(")");
                    } else {
                        separated.push_bind(text);
                    }
                }
                serde_json::Value::Bool(value) => {
                    separated.push_bind(value);
                }
                serde_json::Value::Null => {
                    separated.push("NULL");
                }
                serde_json::Value::Object(_) | serde_json::Value::Array(_) => {
                    let json_string = serde_json::to_string(&value).map_err(|e| e.to_string())?;
                    separated.push_unseparated("CAST(");
                    separated.push_bind_unseparated(json_string);
                    separated.push_unseparated(" AS JSON)");
                }
            }
        }
        separated.push_unseparated(")");
        builder
    };

    let result = builder.build().execute(&pool).await.map_err(|e| e.to_string())?;
    Ok(result.rows_affected())
}

fn extract_order_by(query: &str) -> String {
    let upper = query.to_uppercase();
    if let Some(pos) = upper.rfind("ORDER BY") {
        let after_order = &query[pos..];
        let upper_after = after_order.to_uppercase();
        if let Some(limit_pos) = upper_after.find("LIMIT") {
            after_order[..limit_pos].trim().to_string()
        } else {
            after_order.trim().to_string()
        }
    } else {
        String::new()
    }
}

fn remove_order_by(query: &str) -> String {
    let upper = query.to_uppercase();
    if let Some(pos) = upper.rfind("ORDER BY") {
        let before = query[..pos].trim();
        let after_order = &query[pos..];
        let upper_after = after_order.to_uppercase();
        if let Some(limit_pos) = upper_after.find("LIMIT") {
            let suffix = after_order[limit_pos..].trim();
            format!("{before} {suffix}")
        } else {
            before.to_string()
        }
    } else {
        query.to_string()
    }
}

pub async fn get_views(params: &ConnectionParams, schema: Option<&str>) -> Result<Vec<ViewInfo>, String> {
    let db_name = schema.unwrap_or_else(|| params.database.primary());
    let pool = get_mysql_pool_for_database(params, Some(db_name)).await?;
    let rows = sqlx::query(
        "SELECT table_name as name FROM information_schema.views WHERE table_schema = ? ORDER BY table_name ASC",
    )
    .bind(db_name)
    .fetch_all(&pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(rows
        .iter()
        .map(|row| ViewInfo { name: mysql_row_str(row, 0), definition: None })
        .collect())
}

pub async fn get_view_definition(params: &ConnectionParams, view_name: &str) -> Result<String, String> {
    let pool = get_mysql_pool(params).await?;
    let query = format!("SHOW CREATE VIEW `{}`", escape_identifier(view_name));
    let row = sqlx::query(&query)
        .fetch_one(&pool)
        .await
        .map_err(|e| format!("Failed to get view definition: {e}"))?;
    Ok(mysql_row_str(&row, 1))
}

pub async fn create_view(params: &ConnectionParams, view_name: &str, definition: &str) -> Result<(), String> {
    let pool = get_mysql_pool(params).await?;
    let query = format!("CREATE VIEW `{}` AS {}", escape_identifier(view_name), definition);
    sqlx::query(&query)
        .execute(&pool)
        .await
        .map_err(|e| format!("Failed to create view: {e}"))?;
    Ok(())
}

pub async fn alter_view(params: &ConnectionParams, view_name: &str, definition: &str) -> Result<(), String> {
    let pool = get_mysql_pool(params).await?;
    let query = format!("ALTER VIEW `{}` AS {}", escape_identifier(view_name), definition);
    sqlx::query(&query)
        .execute(&pool)
        .await
        .map_err(|e| format!("Failed to alter view: {e}"))?;
    Ok(())
}

pub async fn drop_view(params: &ConnectionParams, view_name: &str) -> Result<(), String> {
    let pool = get_mysql_pool(params).await?;
    let query = format!("DROP VIEW IF EXISTS `{}`", escape_identifier(view_name));
    sqlx::query(&query)
        .execute(&pool)
        .await
        .map_err(|e| format!("Failed to drop view: {e}"))?;
    Ok(())
}

pub async fn get_view_columns(
    params: &ConnectionParams,
    view_name: &str,
    schema: Option<&str>,
) -> Result<Vec<TableColumn>, String> {
    get_columns(params, view_name, schema).await
}

pub async fn get_routines(params: &ConnectionParams, schema: Option<&str>) -> Result<Vec<RoutineInfo>, String> {
    let db_name = schema.unwrap_or_else(|| params.database.primary());
    let pool = get_mysql_pool_for_database(params, Some(db_name)).await?;
    let rows = sqlx::query(
        r#"
        SELECT routine_name, routine_type, routine_definition
        FROM information_schema.routines
        WHERE routine_schema = ?
        ORDER BY routine_name
    "#,
    )
    .bind(db_name)
    .fetch_all(&pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(rows
        .iter()
        .map(|row| RoutineInfo {
            name: mysql_row_str(row, 0),
            routine_type: mysql_row_str(row, 1),
            definition: mysql_row_str_opt(row, 2),
        })
        .collect())
}

pub async fn get_routine_parameters(
    params: &ConnectionParams,
    routine_name: &str,
    schema: Option<&str>,
) -> Result<Vec<RoutineParameter>, String> {
    let db_name = schema.unwrap_or_else(|| params.database.primary());
    let pool = get_mysql_pool_for_database(params, Some(db_name)).await?;
    let routine_info = sqlx::query(
        r#"
        SELECT DATA_TYPE, ROUTINE_TYPE
        FROM information_schema.routines
        WHERE ROUTINE_SCHEMA = ? AND ROUTINE_NAME = ?
    "#,
    )
    .bind(db_name)
    .bind(routine_name)
    .fetch_optional(&pool)
    .await
    .map_err(|e| e.to_string())?;

    let mut parameters = Vec::new();
    if let Some(info) = routine_info {
        let data_type = mysql_row_str(&info, 0);
        let routine_type = mysql_row_str(&info, 1);
        if routine_type == "FUNCTION" && !data_type.is_empty() {
            parameters.push(RoutineParameter {
                name: String::new(),
                data_type,
                mode: "OUT".into(),
                ordinal_position: 0,
            });
        }
    }

    let rows = sqlx::query(
        r#"
        SELECT parameter_name, data_type, parameter_mode, ordinal_position
        FROM information_schema.parameters
        WHERE specific_schema = ? AND specific_name = ?
        ORDER BY ordinal_position
    "#,
    )
    .bind(db_name)
    .bind(routine_name)
    .fetch_all(&pool)
    .await
    .map_err(|e| e.to_string())?;

    parameters.extend(rows.iter().map(|row| RoutineParameter {
        name: mysql_row_str(row, 0),
        data_type: mysql_row_str(row, 1),
        mode: mysql_row_str(row, 2),
        ordinal_position: row.try_get(3).unwrap_or(0),
    }));

    Ok(parameters)
}

pub async fn get_routine_definition(
    params: &ConnectionParams,
    routine_name: &str,
    routine_type: &str,
) -> Result<String, String> {
    let pool = get_mysql_pool(params).await?;
    let query = format!("SHOW CREATE {} `{}`", routine_type, escape_identifier(routine_name));
    let row = sqlx::query(&query)
        .fetch_one(&pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(mysql_row_str(&row, 2))
}

pub async fn execute_query(
    params: &ConnectionParams,
    query: &str,
    limit: Option<u32>,
    page: u32,
    schema: Option<&str>,
) -> Result<QueryResult, String> {
    let is_select = query.trim_start().to_uppercase().starts_with("SELECT");
    let pool = if let Some(db_name) = schema {
        let mut effective_params = params.clone();
        effective_params.database = DatabaseSelection::Single(db_name.to_string());
        get_mysql_pool_for_database(&effective_params, Some(db_name)).await?
    } else {
        get_mysql_pool(params).await?
    };

    if !is_select {
        let result = sqlx::query(query).execute(&pool).await.map_err(|e| e.to_string())?;
        return Ok(QueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            affected_rows: result.rows_affected(),
            truncated: false,
            pagination: None,
        });
    }

    let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;
    let mut pagination: Option<Pagination> = None;
    let final_query: String;
    let mut manual_limit = limit;
    let mut truncated = false;

    if is_select && limit.is_some() {
        let page_size = limit.unwrap();
        let offset = (page - 1) * page_size;
        let order_by_clause = extract_order_by(query);
        if !order_by_clause.is_empty() {
            let query_without_order = remove_order_by(query);
            final_query = format!(
                "SELECT * FROM ({}) as data_wrapper {} LIMIT {} OFFSET {}",
                query_without_order,
                order_by_clause,
                page_size + 1,
                offset
            );
        } else {
            final_query = format!(
                "SELECT * FROM ({}) as data_wrapper LIMIT {} OFFSET {}",
                query,
                page_size + 1,
                offset
            );
        }
        pagination = Some(Pagination {
            page,
            page_size,
            total_rows: None,
            has_more: false,
        });
        manual_limit = None;
    } else {
        final_query = query.to_string();
    }

    let mut columns = Vec::new();
    let mut rows = Vec::new();
    {
        use futures::stream::StreamExt;
        let mut stream = sqlx::query(&final_query).fetch(&mut *conn);
        while let Some(result) = stream.next().await {
            match result {
                Ok(row) => {
                    if columns.is_empty() {
                        columns = row.columns().iter().map(|column| column.name().to_string()).collect();
                    }
                    if let Some(limit_value) = manual_limit {
                        if rows.len() >= limit_value as usize {
                            truncated = true;
                            break;
                        }
                    }
                    let mut json_row = Vec::new();
                    for (index, _) in row.columns().iter().enumerate() {
                        json_row.push(extract_value(&row, index, None));
                    }
                    rows.push(json_row);
                }
                Err(err) => return Err(err.to_string()),
            }
        }
    }

    if let Some(ref mut pagination_state) = pagination {
        let has_more = rows.len() > pagination_state.page_size as usize;
        if has_more {
            rows.truncate(pagination_state.page_size as usize);
        }
        pagination_state.has_more = has_more;
        truncated = has_more;
    }

    Ok(QueryResult {
        columns,
        rows,
        affected_rows: 0,
        truncated,
        pagination,
    })
}

pub fn get_create_table_sql(table_name: &str, columns: Vec<ColumnDefinition>) -> Result<Vec<String>, String> {
    let mut column_defs = Vec::new();
    let mut pk_columns = Vec::new();

    for column in &columns {
        let mut definition = format!("`{}` {}", escape_identifier(&column.name), column.data_type);
        if !column.is_nullable {
            definition.push_str(" NOT NULL");
        }
        if column.is_auto_increment {
            definition.push_str(" AUTO_INCREMENT");
        }
        if let Some(default_value) = &column.default_value {
            definition.push_str(&format!(" DEFAULT {}", default_value));
        }
        column_defs.push(definition);
        if column.is_pk {
            pk_columns.push(format!("`{}`", escape_identifier(&column.name)));
        }
    }

    if !pk_columns.is_empty() {
        column_defs.push(format!("PRIMARY KEY ({})", pk_columns.join(", ")));
    }

    Ok(vec![format!(
        "CREATE TABLE `{}` (\n  {}\n)",
        escape_identifier(table_name),
        column_defs.join(",\n  ")
    )])
}

pub fn get_add_column_sql(table: &str, column: ColumnDefinition) -> Result<Vec<String>, String> {
    let mut sql = format!(
        "ALTER TABLE `{}` ADD COLUMN `{}` {}",
        escape_identifier(table),
        escape_identifier(&column.name),
        column.data_type
    );
    if !column.is_nullable {
        sql.push_str(" NOT NULL");
    } else {
        sql.push_str(" NULL");
    }
    if column.is_auto_increment {
        sql.push_str(" AUTO_INCREMENT");
    }
    if let Some(default_value) = &column.default_value {
        sql.push_str(&format!(" DEFAULT {}", default_value));
    }
    if column.is_pk {
        sql.push_str(" PRIMARY KEY");
    }
    Ok(vec![sql])
}

pub fn get_alter_column_sql(
    table: &str,
    old_column: ColumnDefinition,
    new_column: ColumnDefinition,
) -> Result<Vec<String>, String> {
    let mut sql = if old_column.name != new_column.name {
        format!(
            "ALTER TABLE `{}` CHANGE `{}` `{}` {}",
            escape_identifier(table),
            escape_identifier(&old_column.name),
            escape_identifier(&new_column.name),
            new_column.data_type
        )
    } else {
        format!(
            "ALTER TABLE `{}` MODIFY COLUMN `{}` {}",
            escape_identifier(table),
            escape_identifier(&new_column.name),
            new_column.data_type
        )
    };
    if !new_column.is_nullable {
        sql.push_str(" NOT NULL");
    } else {
        sql.push_str(" NULL");
    }
    if new_column.is_auto_increment {
        sql.push_str(" AUTO_INCREMENT");
    }
    if let Some(default_value) = &new_column.default_value {
        sql.push_str(&format!(" DEFAULT {}", default_value));
    }
    Ok(vec![sql])
}

pub fn get_create_index_sql(
    table: &str,
    index_name: &str,
    columns: Vec<String>,
    is_unique: bool,
) -> Result<Vec<String>, String> {
    let unique = if is_unique { "UNIQUE " } else { "" };
    let quoted_columns: Vec<String> = columns
        .iter()
        .map(|column| format!("`{}`", escape_identifier(column)))
        .collect();
    Ok(vec![format!(
        "CREATE {}INDEX `{}` ON `{}` ({})",
        unique,
        escape_identifier(index_name),
        escape_identifier(table),
        quoted_columns.join(", ")
    )])
}

pub fn get_create_foreign_key_sql(
    table: &str,
    fk_name: &str,
    column: &str,
    ref_table: &str,
    ref_column: &str,
    on_delete: Option<&str>,
    on_update: Option<&str>,
) -> Result<Vec<String>, String> {
    let mut sql = format!(
        "ALTER TABLE `{}` ADD CONSTRAINT `{}` FOREIGN KEY (`{}`) REFERENCES `{}` (`{}`)",
        escape_identifier(table),
        escape_identifier(fk_name),
        escape_identifier(column),
        escape_identifier(ref_table),
        escape_identifier(ref_column)
    );
    if let Some(action) = on_delete {
        sql.push_str(&format!(" ON DELETE {action}"));
    }
    if let Some(action) = on_update {
        sql.push_str(&format!(" ON UPDATE {action}"));
    }
    Ok(vec![sql])
}

pub async fn drop_index(params: &ConnectionParams, table: &str, index_name: &str) -> Result<(), String> {
    let sql = format!(
        "DROP INDEX `{}` ON `{}`",
        escape_identifier(index_name),
        escape_identifier(table)
    );
    execute_query(params, &sql, None, 1, None).await?;
    Ok(())
}

pub async fn drop_foreign_key(params: &ConnectionParams, table: &str, fk_name: &str) -> Result<(), String> {
    let sql = format!(
        "ALTER TABLE `{}` DROP FOREIGN KEY `{}`",
        escape_identifier(table),
        escape_identifier(fk_name)
    );
    execute_query(params, &sql, None, 1, None).await?;
    Ok(())
}

pub async fn get_schema_snapshot(
    params: &ConnectionParams,
    schema: Option<&str>,
) -> Result<Vec<TableSchema>, String> {
    let tables = get_tables(params, schema).await?;
    let mut columns_map = get_all_columns_batch(params, schema).await?;
    let mut fks_map = get_all_foreign_keys_batch(params, schema).await?;
    Ok(tables
        .into_iter()
        .map(|table| TableSchema {
            name: table.name.clone(),
            columns: columns_map.remove(&table.name).unwrap_or_default(),
            foreign_keys: fks_map.remove(&table.name).unwrap_or_default(),
        })
        .collect())
}