/// Maximum size in bytes for BLOB data to include as base64 preview.
pub const MAX_BLOB_PREVIEW_SIZE: usize = 4096;

/// Default maximum size in bytes for a BLOB file that can be uploaded/loaded into memory.
pub const DEFAULT_MAX_BLOB_SIZE: u64 = 100 * 1024 * 1024;

pub fn encode_blob(data: &[u8]) -> String {
    let total_size = data.len();
    let preview = if total_size > MAX_BLOB_PREVIEW_SIZE {
        &data[..MAX_BLOB_PREVIEW_SIZE]
    } else {
        data
    };

    let mime_type = infer::get(preview)
        .map(|kind| kind.mime_type())
        .unwrap_or("application/octet-stream");

    let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, preview);
    format!("BLOB:{total_size}:{mime_type}:{b64}")
}

pub fn encode_blob_full(data: &[u8]) -> String {
    let total_size = data.len();
    let mime_type = infer::get(data)
        .map(|kind| kind.mime_type())
        .unwrap_or("application/octet-stream");
    let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, data);
    format!("BLOB:{total_size}:{mime_type}:{b64}")
}

pub fn resolve_blob_file_ref(value: &str, max_size: u64) -> Result<Vec<u8>, String> {
    let rest = value
        .strip_prefix("BLOB_FILE_REF:")
        .ok_or_else(|| "Not a BLOB_FILE_REF".to_string())?;

    let parts: Vec<&str> = rest.splitn(3, ':').collect();
    if parts.len() != 3 {
        return Err("Invalid BLOB_FILE_REF format".to_string());
    }

    let file_size: u64 = parts[0]
        .parse()
        .map_err(|_| "Invalid file size in BLOB_FILE_REF".to_string())?;
    if file_size > max_size {
        return Err(format!(
            "File size ({file_size} bytes) exceeds maximum allowed size ({max_size} bytes / {}MB)",
            max_size / (1024 * 1024)
        ));
    }

    std::fs::read(parts[2]).map_err(|e| format!("Failed to read BLOB file: {e}"))
}

pub fn decode_blob_wire_format(value: &str, max_size: u64) -> Option<Vec<u8>> {
    if value.starts_with("BLOB_FILE_REF:") {
        return resolve_blob_file_ref(value, max_size).ok();
    }

    let rest = value.strip_prefix("BLOB:")?;
    let after_size = rest.splitn(2, ':').nth(1)?;
    let base64_data = after_size.splitn(2, ':').nth(1)?;
    base64::Engine::decode(&base64::engine::general_purpose::STANDARD, base64_data).ok()
}