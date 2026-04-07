# mysqlssl-plugin

External Tabularis driver for MySQL and MariaDB with PEM-based TLS support, including client certificate authentication.
Based heavily on the current MySQL driver.

## What It Adds

- MySQL and MariaDB connectivity through the Tabularis external plugin system
- TLS modes from `disabled` through `verify_identity`
- PEM CA, client certificate, and client key support
- Multi-database discovery and selection in Tabularis
- Table, view, routine, query, CRUD, and schema-management support similar to the built-in MySQL driver

## Distribution Status

This repository now supports two configuration paths:

1. Preferred: Tabularis plugin settings passed through the `initialize` RPC call
2. Fallback: a sidecar `mysqlssl-plugin.config.json` file placed next to the executable

For end users, the in-app plugin settings should be used first. The sidecar file remains useful for manual deployments and troubleshooting.

## Installation

### Package Contents

The distributable plugin folder must contain:

```text
mysqlssl/
├── manifest.json
├── mysqlssl-plugin.exe
├── README.md
└── mysqlssl-plugin.config.json.example
```

### Tabularis Plugin Folder

On Windows, the active plugin binary may live under one of these folders depending on the Tabularis build:

- `C:\Users\<you>\AppData\Roaming\tabularis\plugins\mysqlssl`
- `C:\Users\<you>\AppData\Roaming\debba\tabularis\data\plugins\mysqlssl`

If a plugin does not appear in the driver list, verify which root your installed Tabularis build is actually using.

### Enable The Plugin

Make sure `mysqlssl` is listed in Tabularis settings under `activeExternalDrivers`.

## Plugin Settings In Tabularis

After installing the plugin, open the plugin settings gear icon in Tabularis and configure these fields:

- `SSL Mode`: one of `disabled`, `preferred`, `required`, `verify_ca`, `verify_identity`
- `CA Certificate Path`: PEM CA certificate used to validate the server
- `Client Certificate Path`: PEM client certificate presented to MySQL
- `Client Key Path`: PEM private key matching the client certificate
- `Max Connections`: connection pool size inside the plugin process
- `Max Blob Size`: maximum BLOB size handled by blob helpers and grid operations

Path handling rules:

- Absolute paths are used as-is
- Relative paths are resolved relative to the plugin executable directory

## Fallback Sidecar Config

If you prefer file-based configuration, copy `mysqlssl-plugin.config.json.example` to `mysqlssl-plugin.config.json` next to the executable and edit it.

Example:

```json
{
  "ssl_ca": "./certs/ca.pem",
  "ssl_cert": "./certs/client-cert.pem",
  "ssl_key": "./certs/client-key.pem",
  "ssl_mode": "verify_ca",
  "max_connections": 5,
  "max_blob_size": 104857600
}
```

Runtime precedence is:

1. Tabularis plugin settings
2. Sidecar config file
3. Request-level `ssl_mode`
4. Built-in defaults

If `ssl_mode` is `verify_ca` or `verify_identity` and no CA path is configured, the plugin falls back to `required`.

## Building

### Requirements

- Rust toolchain
- Windows for the current packaging script

### Debug Build

```powershell
cargo build
```

### Release Build

```powershell
cargo build --release
```

## Packaging

Create the distributable folder with:

```powershell
.\package-plugin.ps1
```

This produces:

- `dist/mysqlssl` as the unpacked plugin folder
- `dist/mysqlssl-v0.2.0-win-x64.zip` as the Windows release asset

## Release Checklist

1. Run `cargo build --release`
2. Run `.\package-plugin.ps1`
3. Upload `dist\mysqlssl-v0.2.0-win-x64.zip` to GitHub Releases, or copy `dist\mysqlssl` into a `mysqlssl` folder in the Tabularis plugin directory
4. Open Tabularis and confirm `mysqlssl` is enabled in external drivers
5. Configure TLS paths in the plugin settings modal
6. Test `Test Connection`
7. Open the connection editor and verify `Load Databases` works with non-empty `host` and `username`

## Local TLS Test Database

The repository includes a reproducible mutual-TLS MySQL setup under `docker/mysql-tls`.

Quick start:

```powershell
Set-Location .\docker\mysql-tls
.\generate-certs.ps1
docker compose up -d
```

The database server setup:

- listens on `127.0.0.1:3306`
- requires encrypted transport
- configures `appuser` with `REQUIRE X509`
- uses the generated `client-cert.pem` and `client-key.pem` for client-auth testing

## Notes

- This plugin intentionally mirrors the built-in MySQL feature set closely while adding PEM TLS configuration.
