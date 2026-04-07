use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use rust_decimal::Decimal;
use sqlx::Row;
use uuid::Uuid;

use crate::common::encode_blob;

pub fn extract_value(
    row: &sqlx::mysql::MySqlRow,
    index: usize,
    known_type: Option<&str>,
) -> serde_json::Value {
    use sqlx::{Column, TypeInfo, ValueRef};

    let col = row.columns().get(index);
    let col_name = col.map(|c| c.name()).unwrap_or("unknown");
    let col_type = col.map(|c| c.type_info().name()).unwrap_or("unknown");
    let effective_type = known_type
        .map(|value| value.to_uppercase())
        .unwrap_or_else(|| col_type.to_uppercase());

    if let Some(value_ref) = row.try_get_raw(index).ok() {
        if value_ref.is_null() {
            return serde_json::Value::Null;
        }
    }

    if effective_type == "DECIMAL" || effective_type == "NEWDECIMAL" || effective_type == "NUMERIC" {
        if let Ok(value) = row.try_get::<Decimal, _>(index) {
            return serde_json::Value::String(value.to_string());
        }
        if let Ok(value) = row.try_get::<String, _>(index) {
            return serde_json::Value::String(value);
        }
    }

    if effective_type == "TIMESTAMP" || effective_type == "DATETIME" {
        if let Ok(value) = row.try_get::<NaiveDateTime, _>(index) {
            return serde_json::Value::String(value.format("%Y-%m-%d %H:%M:%S").to_string());
        }
        if let Ok(value) = row.try_get::<DateTime<Utc>, _>(index) {
            return serde_json::Value::String(value.format("%Y-%m-%d %H:%M:%S").to_string());
        }
        if let Ok(value) = row.try_get::<String, _>(index) {
            if let Ok(dt) = NaiveDateTime::parse_from_str(&value, "%Y-%m-%dT%H:%M:%S%.f") {
                return serde_json::Value::String(dt.format("%Y-%m-%d %H:%M:%S").to_string());
            }
            if let Ok(dt) = NaiveDateTime::parse_from_str(&value, "%Y-%m-%dT%H:%M:%S") {
                return serde_json::Value::String(dt.format("%Y-%m-%d %H:%M:%S").to_string());
            }
            return serde_json::Value::String(value);
        }
        if let Ok(value) = row.try_get::<i64, _>(index) {
            return serde_json::Value::from(value);
        }
    }

    if effective_type.contains("BLOB") || effective_type.contains("BINARY") {
        if let Ok(value) = row.try_get::<Vec<u8>, _>(index) {
            let is_binary_string = effective_type.contains("VARBINARY") || effective_type == "BINARY";
            if is_binary_string && value.len() <= 65_535 {
                if let Ok(text) = String::from_utf8(value.clone()) {
                    return serde_json::Value::String(text);
                }
            }
            if known_type.is_none() {
                if let Ok(text) = String::from_utf8(value.clone()) {
                    return serde_json::Value::String(text);
                }
            }
            return serde_json::Value::String(encode_blob(&value));
        }
        if let Ok(value) = row.try_get::<String, _>(index) {
            return serde_json::Value::String(encode_blob(value.as_bytes()));
        }
    }

    if effective_type.contains("TEXT") {
        if let Ok(value) = row.try_get::<String, _>(index) {
            return serde_json::Value::String(value);
        }
        if let Ok(value) = row.try_get::<Vec<u8>, _>(index) {
            if let Ok(text) = String::from_utf8(value.clone()) {
                return serde_json::Value::String(text);
            }
            return serde_json::Value::String(base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD,
                value,
            ));
        }
    }

    if effective_type == "JSON" {
        if let Ok(value) = row.try_get::<serde_json::Value, _>(index) {
            return value;
        }
        if let Ok(raw_value) = row.try_get_raw(index) {
            use sqlx::ValueRef;
            if !raw_value.is_null() {
                if let Ok(bytes) = <Vec<u8> as sqlx::Decode<sqlx::MySql>>::decode(raw_value) {
                    if let Ok(text) = String::from_utf8(bytes) {
                        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&text) {
                            return parsed;
                        }
                        return serde_json::Value::String(text);
                    }
                }
            }
        }
    }

    if effective_type == "GEOMETRY"
        || effective_type.contains("POINT")
        || effective_type.contains("LINESTRING")
        || effective_type.contains("POLYGON")
        || effective_type.contains("COLLECTION")
    {
        if let Ok(raw_value) = row.try_get_raw(index) {
            use sqlx::ValueRef;
            if !raw_value.is_null() {
                if let Ok(value) = <Vec<u8> as sqlx::Decode<sqlx::MySql>>::decode(raw_value) {
                    let hex = value.iter().map(|byte| format!("{byte:02X}")).collect::<String>();
                    return serde_json::Value::String(format!("0x{hex}"));
                }
            }
        }
    }

    if let Ok(value) = row.try_get::<NaiveDateTime, _>(index) {
        return serde_json::Value::String(value.format("%Y-%m-%d %H:%M:%S").to_string());
    }
    if let Ok(value) = row.try_get::<NaiveDate, _>(index) {
        return serde_json::Value::String(value.to_string());
    }
    if let Ok(value) = row.try_get::<NaiveTime, _>(index) {
        return serde_json::Value::String(value.to_string());
    }

    if let Ok(value) = row.try_get::<u64, _>(index) {
        return serde_json::Value::from(value);
    }
    if let Ok(value) = row.try_get::<u32, _>(index) {
        return serde_json::Value::from(value);
    }
    if let Ok(value) = row.try_get::<u16, _>(index) {
        return serde_json::Value::from(value);
    }
    if let Ok(value) = row.try_get::<u8, _>(index) {
        return serde_json::Value::from(value);
    }
    if let Ok(value) = row.try_get::<i64, _>(index) {
        return serde_json::Value::from(value);
    }
    if let Ok(value) = row.try_get::<i32, _>(index) {
        return serde_json::Value::from(value);
    }
    if let Ok(value) = row.try_get::<i16, _>(index) {
        return serde_json::Value::from(value);
    }
    if let Ok(value) = row.try_get::<i8, _>(index) {
        return serde_json::Value::from(value);
    }
    if let Ok(value) = row.try_get::<Decimal, _>(index) {
        return serde_json::Value::String(value.to_string());
    }
    if let Ok(value) = row.try_get::<f64, _>(index) {
        return serde_json::Number::from_f64(value)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null);
    }
    if let Ok(value) = row.try_get::<f32, _>(index) {
        return serde_json::Number::from_f64(value as f64)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null);
    }
    if let Ok(value) = row.try_get::<bool, _>(index) {
        return serde_json::Value::from(value);
    }
    if let Ok(value) = row.try_get::<String, _>(index) {
        return serde_json::Value::from(value);
    }
    if let Ok(value) = row.try_get::<Uuid, _>(index) {
        return serde_json::Value::String(value.to_string());
    }
    if let Ok(value) = row.try_get::<Vec<u8>, _>(index) {
        return serde_json::Value::String(encode_blob(&value));
    }

    eprintln!(
        "[WARNING] Column '{}' [{}] type '{}' could not be extracted",
        col_name, index, col_type
    );
    serde_json::Value::Null
}