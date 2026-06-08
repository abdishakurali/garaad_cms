# franchisetech Connector

Local connector for franchisetech cash drawer commands.

The connector must run on the customer till computer. The DigitalOcean server is not the real runtime because it cannot reach the local receipt printer or cash drawer.

## Current Beta Scope

- LAN/network ESC/POS receipt printer only
- Cash drawer connected to the printer drawer kick port
- No USB, Bluetooth, Android native, iOS native, or receipt printing yet

## Windows App

franchisetech Connector is packaged as a Windows Tauri app for customers. Version `0.2.0` adds:

- launch at login
- guided pairing from franchisetech Settings
- network printer discovery
- setup wizard
- diagnostics export

Customers should install `franchisetechConnectorSetup.exe`; they should not install Rust or edit JSON for normal setup.

## Configure

Advanced users can still edit `connector-config.json`:

```json
{
  "printerType": "network_escpos",
  "printerIp": "192.168.1.50",
  "printerPort": 9100,
  "drawerKickCommand": "1B700019FA",
  "pairingToken": "choose-a-long-random-token",
  "allowedOrigins": ["https://franchisetech.ro"],
  "devMode": false,
  "startAtLogin": true,
  "setupComplete": false
}
```

Use `devMode: true` only for local testing. In development you may add `http://localhost:3000` to `allowedOrigins`.

## Run Locally

```bash
cd connector
cargo run
```

The connector binds only to `127.0.0.1:17878`.

## Health Test

`/health` is public:

```bash
curl http://127.0.0.1:17878/health
```

Expected:

```json
{"ok":true,"name":"franchisetech-connector","version":"0.2.0"}
```

## Status Test

Protected endpoints require the pairing token and an allowed Origin:

```bash
curl http://127.0.0.1:17878/status \
  -H 'Origin: https://franchisetech.ro' \
  -H 'Authorization: Bearer choose-a-long-random-token'
```

## Open Drawer Command Test

```bash
curl -X POST http://127.0.0.1:17878/open-drawer \
  -H 'Origin: https://franchisetech.ro' \
  -H 'Authorization: Bearer choose-a-long-random-token' \
  -H 'Content-Type: application/json' \
  -d '{"reason":"test","source":"franchisetech-web"}'
```

Success means the ESC/POS open-drawer command was sent to the printer. It does not prove the drawer physically opened.

## Build Release Binary

On the build machine:

```bash
cargo build --release
```

The binary will be at:

```bash
target/release/franchisetech-connector
```

Copy the binary and `connector-config.json` to the till computer.

## Windows Beta Packaging Path

For beta customers, build a Windows release binary on Windows:

```powershell
cargo build --release
```

Ship:

- `target\release\franchisetech-connector.exe`
- `connector-config.json`
- this README

The customer should not need Rust or Cargo installed. Future production releases should be code signed.

## Hardware Test Checklist

1. Till computer can reach the printer IP.
2. Receipt printer is LAN/network ESC/POS.
3. Printer raw TCP port is `9100`.
4. Cash drawer cable plugs into the printer drawer port.
5. `connector-config.json` has the printer IP.
6. `/health` returns ok.
7. `/status` shows `configured: true`.
8. `/open-drawer` returns `command_sent`.
9. Drawer physically opens during the beta hardware test.

## Run Tests

```bash
cargo test
```
