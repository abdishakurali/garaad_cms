# franchisetech Connector — Smoke Test

Quick curl checks to verify the connector is working correctly.
Run these from the Windows machine where the connector is installed.

## Prerequisites

- Connector is running (system tray icon visible)
- Pairing token is known (shown in connector Settings → Pairing Token)
- Replace `<TOKEN>` with your actual pairing token throughout

---

## 1. Health check (no auth required)

```bash
curl http://127.0.0.1:17878/health
```

**Expected:**
```json
{"ok":true,"name":"franchisetech-connector","version":"0.3.0","serverTime":1234567890,"mode":"simulation"}
```

**FAIL if:** `name` is missing or `version` is `0.2.x` (old connector — update required).

---

## 2. Status (requires token)

```bash
curl -H "x-connector-token: <TOKEN>" http://127.0.0.1:17878/status
```

**Expected:**
```json
{
  "ok": true,
  "name": "franchisetech-connector",
  "version": "0.3.0",
  "mode": "simulation",
  "paired": true,
  "capabilities": {
    "pairing": true,
    "simulation": true,
    "diagnostics": true,
    "events": true,
    "printerDiscovery": true,
    "liveEscposDrawerKick": true
  }
}
```

**FAIL if:** `capabilities` is missing or `paired` is `false`.

---

## 3. Simulation (simulation mode, no printer required)

```bash
curl -X POST http://127.0.0.1:17878/simulate   -H "content-type: application/json"   -H "x-connector-token: <TOKEN>"   -d "{}"
```

**Expected:**
```json
{"ok":true,"result":"simulation_success","durationMs":1}
```

---

## 4. Diagnostics (any mode)

```bash
curl -X POST http://127.0.0.1:17878/diagnostics/run   -H "content-type: application/json"   -H "x-connector-token: <TOKEN>"   -d "{}"
```

**Expected:** `overall` is `"passed"` or `"warning"`.  
**FAIL if:** HTTP 404 (old connector) or `overall` is `"failed"`.

---

## 5. Open drawer (LIVE MODE ONLY — sends real ESC/POS bytes)

> ⚠ Only run in **Live** mode with a real printer connected.

```bash
curl -X POST http://127.0.0.1:17878/drawer/open   -H "content-type: application/json"   -H "x-connector-token: <TOKEN>"   -d "{\"reason\":\"test\"}"
```

**Expected:** `{"ok":true,"result":"command_sent",...}`  
**FAIL if:** drawer does not open, or response contains `simulation_only` (wrong mode).

---

## 6. Rate limit check

Send the drawer open command twice in quick succession.  
**Expected:** second call returns `429 Too Many Requests` with `{"errorCode":"RATE_LIMITED",...}`.

---

## Gate: all smoke tests must pass before hardware testing

| # | Check | Pass condition |
|---|-------|----------------|
| 1 | Health | `name` = `franchisetech-connector`, version ≥ 0.3.0 |
| 2 | Status | `capabilities` present, `paired` = true |
| 3 | Simulation | `result` = `simulation_success` |
| 4 | Diagnostics | `overall` ≠ `failed` |
| 5 | Drawer (live) | `result` = `command_sent`, drawer opens |
| 6 | Rate limit | Second rapid call → 429 |
