$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $MyInvocation.MyCommand.Path
$certDir = Join-Path $root 'certs'
$opensslConfig = Join-Path $root 'openssl.cnf'

New-Item -ItemType Directory -Force -Path $certDir | Out-Null

$containerCommand = @(
	'set -e'
	'apk add --no-cache openssl >/dev/null'
	'cd /work'
	'openssl genrsa -out certs/ca-key.pem 2048'
	"openssl req -x509 -new -nodes -key certs/ca-key.pem -sha256 -days 3650 -out certs/ca.pem -subj '/C=IS/ST=Reykjavik/L=Reykjavik/O=mysqlssl-plugin/OU=local-test/CN=mysqlssl-plugin-test-ca'"
	'openssl genrsa -out certs/server-key.pem 2048'
	'openssl req -new -key certs/server-key.pem -out certs/server.csr -config openssl.cnf'
	'openssl x509 -req -in certs/server.csr -CA certs/ca.pem -CAkey certs/ca-key.pem -CAcreateserial -out certs/server-cert.pem -days 3650 -sha256 -extensions req_ext -extfile openssl.cnf'
	'openssl genrsa -out certs/client-key.pem 2048'
	"openssl req -new -key certs/client-key.pem -out certs/client.csr -subj '/C=IS/ST=Reykjavik/L=Reykjavik/O=mysqlssl-plugin/OU=local-test/CN=mysqlssl-client'"
	'openssl x509 -req -in certs/client.csr -CA certs/ca.pem -CAkey certs/ca-key.pem -CAcreateserial -out certs/client-cert.pem -days 3650 -sha256'
	'chmod 644 certs/*.pem'
	'rm -f certs/*.csr certs/*.srl'
) -join "`n"

docker run --rm -v "${root}:/work" alpine:3.20 sh -lc $containerCommand

Write-Output "Certificates written to $certDir"
