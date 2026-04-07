# Local MySQL TLS Test Harness

This folder gives you a controlled MySQL 8 server with TLS forced so you can test the plugin against a known-good setup.

On Windows with Docker Desktop, MySQL may ignore bind-mounted `.cnf` files as world-writable. This harness passes the TLS settings on the `mysqld` command line instead so the server always uses the generated certs.

## What it does

- Runs MySQL on `127.0.0.1:3306`
- Forces TLS with `require_secure_transport=ON`
- Generates a local CA, server cert, and client cert
- Requires an X.509 client certificate for `appuser`
- Seeds a small `appdb.widgets` table

## Quick Start

From this folder:

```powershell
.\generate-certs.ps1
docker compose up -d
docker compose ps
```

If you already started the container before changing the init SQL, recreate it so MySQL reruns the init script:

```powershell
docker compose down
docker compose up -d
```

## Test Connection Values

- Host: `127.0.0.1`
- Port: `3306`
- Username: `appuser`
- Password: `apppass`
- Database: `appdb`

## Plugin Config

Point your installed plugin config at these generated files:

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

If you copy the files into the live Tabularis plugin directory, keep the paths relative to that directory.

Without the client cert and key, `appuser` should now fail to connect. That is intentional and lets you verify whether the plugin is actually presenting the client certificate.

## Useful Commands

```powershell
docker compose logs -f mysql
docker compose down -v
```
