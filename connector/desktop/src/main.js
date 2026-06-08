import { invoke } from "@tauri-apps/api/core";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";

const $ = (id) => document.getElementById(id);
let currentConfig = null;

// ── Tab switching ─────────────────────────────────────────────────────────────
document.querySelectorAll(".tab-btn").forEach((btn) => {
  btn.addEventListener("click", () => {
    document.querySelectorAll(".tab-btn").forEach((b) => b.classList.remove("active"));
    document.querySelectorAll(".tab-panel").forEach((p) => p.classList.remove("active"));
    btn.classList.add("active");
    document.querySelector(`.tab-panel[data-panel="${btn.dataset.tab}"]`).classList.add("active");
    if (btn.dataset.tab === "events") loadEvents();
  });
});

// ── Form read/write ───────────────────────────────────────────────────────────
function readForm() {
  return {
    printerType: $("printerType").value,
    printerIp: $("printerIpMain").value.trim() || $("printerIp").value.trim(),
    printerPort: Number($("printerPortMain").value || $("printerPort").value || 9100),
    drawerKickCommand: "1B700019FA",
    pairingToken: $("pairingToken").value.trim(),
    allowedOrigins: [$("allowedOrigin").value.trim()].filter(Boolean),
    devMode: $("devMode").checked,
    startAtLogin: $("startAtLogin").checked,
    connectorRunMode: $("connectorRunMode").value,
    setupComplete: Boolean(currentConfig?.setupComplete),
    hardwareVerified: Boolean(currentConfig?.hardwareVerified),
    lastHardwareVerifiedAt: currentConfig?.lastHardwareVerifiedAt ?? null,
    hardwareVerificationSource: currentConfig?.hardwareVerificationSource ?? null,
  };
}

function fillForm(config) {
  currentConfig = config;
  $("printerType").value = config.printerType || "network_escpos";
  $("printerIp").value = config.printerIp || "";
  $("printerIpMain").value = config.printerIp || "";
  $("printerPort").value = config.printerPort || 9100;
  $("printerPortMain").value = config.printerPort || 9100;
  $("pairingToken").value = config.pairingToken || "";
  $("allowedOrigin").value = (config.allowedOrigins && config.allowedOrigins[0]) || "https://franchisetech.ro";
  $("devMode").checked = Boolean(config.devMode);
  $("startAtLogin").checked = config.startAtLogin !== false;
  $("connectorRunMode").value = config.connectorRunMode || "simulation";
  updateBadges(config);
  if (config.setupComplete) {
    $("sum-paired").textContent   = config.pairingToken ? "Paired" : "Not paired";
    $("sum-printer").textContent  = config.printerIp || "Not configured";
    $("sum-hardware").textContent = config.hardwareVerified ? "Verified" : "Not verified";
  }
}

function updateBadges(config) {
  const mode = config.connectorRunMode || "simulation";
  $("pairing-status").textContent = config.pairingToken ? "Paired" : "Not configured";
  $("printer-status").textContent = config.printerIp ? config.printerIp : "Not configured";
  $("hw-status").textContent      = config.hardwareVerified ? "Verified" : "Not verified";
  $("mode-status").textContent    = mode.charAt(0).toUpperCase() + mode.slice(1);
  $("wizard-pairing").textContent = config.pairingToken ? "Paired" : "Waiting for franchisetech";
  // Update mode badge in header
  const badge = $("mode-badge");
  badge.textContent = mode;
  badge.className = `mode-badge ${mode}`;
}

function showStatusMsg(text, type = "info") {
  const el = $("status-message");
  el.textContent = text;
  el.className = `message ${type}`;
  el.classList.remove("hidden");
}

function show(text) {
  $("message").textContent = text;
  $("top-status").textContent = text;
}

// ── Save / Load ───────────────────────────────────────────────────────────────
async function saveConfig(extra = {}) {
  const config = { ...readForm(), ...extra };
  const saveBtn = $("save");
  if (saveBtn && !extra.silent) { saveBtn.disabled = true; saveBtn.textContent = "Saving…"; }
  try {
    await invoke("save_config", { config });
    currentConfig = config;
    updateBadges(config);
    if (!extra.silent) show("Settings saved.");
  } catch (error) {
    show(String(error || "Settings saved but autostart could not be enabled."));
  } finally {
    if (saveBtn && !extra.silent) { saveBtn.disabled = false; saveBtn.textContent = "Save settings"; }
  }
}

async function load() {
  const config = await invoke("get_config");
  fillForm(config);
}

// ── Wizard step navigation ────────────────────────────────────────────────────
function showStep(step) {
  document.querySelectorAll(".wizard-step").forEach((panel) => {
    panel.classList.toggle("hidden", panel.dataset.panel !== String(step));
  });
  document.querySelectorAll(".steps button").forEach((button) => {
    button.classList.toggle("active", button.dataset.step === String(step));
  });
}

function syncPrinterInputs(ip, port) {
  $("printerIp").value = ip;
  $("printerIpMain").value = ip;
  $("printerPort").value = port;
  $("printerPortMain").value = port;
}

// ── Printer discovery ─────────────────────────────────────────────────────────
async function findPrinters(targetId) {
  const target = $(targetId);
  target.innerHTML = "<p>Searching for network printers…</p>";
  const printers = await invoke("find_network_printers");
  if (!printers.length) {
    target.innerHTML = "<p>No network printers found. Enter the printer IP manually.</p>";
    return;
  }
  target.innerHTML = "";
  printers.forEach((printer) => {
    const row = document.createElement("div");
    row.className = "result";
    row.innerHTML = `<div><strong>${printer.name}</strong><span>${printer.ip}:${printer.port} · ${printer.status}</span></div>`;
    const button = document.createElement("button");
    button.textContent = "Use this printer";
    button.className = "secondary";
    button.addEventListener("click", async () => {
      await invoke("use_printer", { candidate: printer });
      syncPrinterInputs(printer.ip, printer.port);
      const config = await invoke("get_config");
      fillForm(config);
      show("Printer selected.");
    });
    row.appendChild(button);
    target.appendChild(row);
  });
}

// ── Pairing ───────────────────────────────────────────────────────────────────
async function checkPendingPairing() {
  const pending = await invoke("get_pending_pairing");
  $("pairing-request").classList.toggle("hidden", !pending);
  if (pending) {
    $("pairing-origin").textContent = `${pending.origin} wants to connect to this connector.`;
  }
}

async function approvePairing() {
  const token = await invoke("approve_pairing");
  $("pairingToken").value = token;
  const config = await invoke("get_config");
  fillForm(config);
  $("pairing-request").classList.add("hidden");
  show("Pairing approved.");
}

async function denyPairing() {
  await invoke("deny_pairing");
  $("pairing-request").classList.add("hidden");
  show("Pairing denied.");
}

// ── Printer test ──────────────────────────────────────────────────────────────
async function testConnection() {
  const btn = $("testConnection");
  const btnWiz = $("testConnectionWizard");
  if (btn) btn.disabled = true;
  if (btnWiz) btnWiz.disabled = true;
  try {
    await saveConfig();
    const result = await invoke("test_printer_connection");
    if (result === "reachable") show("Printer is reachable. Connection works.");
    else if (result === "printer_not_configured") show("Printer IP is missing. Enter the printer IP in the Setup tab.");
    else show("Printer is not reachable. Check IP, port, and that the printer is on the same network.");
  } catch {
    show("Invalid configuration.");
  } finally {
    if (btn) btn.disabled = false;
    if (btnWiz) btnWiz.disabled = false;
  }
}

async function testOpen() {
  const btn = $("testOpen");
  if (btn) btn.disabled = true;
  try {
    await saveConfig();
    const result = await invoke("send_test_open_command");
    const labels = {
      simulation_only: "Simulated",
      diagnostic_only: "Diagnostic",
      printer_not_configured: "Not configured",
      command_sent: "Command sent",
      failed: "Failed",
    };
    $("last-command").textContent = labels[result] ?? result;
    if (result === "simulation_only") show("Simulation mode is active — no drawer command was sent. Switch to Live mode to send a real command.");
    else if (result === "diagnostic_only") show("Diagnostic mode is active — no drawer command was sent. Switch to Live mode to send a real command.");
    else if (result === "printer_not_configured") show("Printer IP is missing. Enter the printer IP in the Setup tab.");
    else if (result === "command_sent") show("Open-drawer command sent.");
    else show("Could not send drawer command. Check printer IP, port, and network.");
  } catch (e) {
    $("last-command").textContent = "Failed";
    show("Could not send drawer command: " + e);
  } finally {
    if (btn) btn.disabled = false;
  }
}

// ── Status tab: simulation ────────────────────────────────────────────────────
async function runSimulationCheck() {
  showStatusMsg("Running simulation…", "info");
  try {
    // Call HTTP /simulate on localhost (no Tauri command needed — connector is running locally)
    const res = await fetch("http://127.0.0.1:17878/simulate", {
      method: "POST",
      headers: {
        "content-type": "application/json",
        "origin": "tauri://localhost",
        "x-connector-token": currentConfig?.pairingToken || "",
      },
      body: "{}",
    });
    const body = await res.json().catch(() => ({}));
    if (body.result === "simulation_success") {
      showStatusMsg(`Simulation passed in ${body.durationMs ?? "?"}ms. Web ↔ Connector communication works.`, "success");
    } else {
      showStatusMsg(body.message || "Simulation failed.", "error");
    }
  } catch (e) {
    showStatusMsg("Simulation failed: " + e.message, "error");
  }
}

// ── Status tab: diagnostics ───────────────────────────────────────────────────
async function runDiagnosticsCheck() {
  showStatusMsg("Running diagnostics…", "info");
  $("diag-results").classList.add("hidden");
  try {
    const res = await fetch("http://127.0.0.1:17878/diagnostics/run", {
      method: "POST",
      headers: {
        "content-type": "application/json",
        "origin": "tauri://localhost",
        "x-connector-token": currentConfig?.pairingToken || "",
      },
      body: "{}",
    });
    const body = await res.json().catch(() => ({}));
    const checks = Array.isArray(body.checks) ? body.checks : [];
    const overall = body.overall || "failed";

    const list = $("diag-check-list");
    list.innerHTML = "";
    checks.forEach((c) => {
      const li = document.createElement("li");
      li.className = "check-item";
      const icon = c.result === "passed" ? "✓" : c.result === "warning" ? "⚠" : "✗";
      li.innerHTML = `<span class="check-icon ${c.result}">${icon}</span>
        <span><strong>${c.name.replace(/_/g, " ")}</strong>: ${c.message}${c.suggestion ? ` <em>(${c.suggestion})</em>` : ""}</span>`;
      list.appendChild(li);
    });

    $("diag-results").classList.remove("hidden");
    const type = overall === "passed" ? "success" : overall === "warning" ? "info" : "error";
    showStatusMsg(`Diagnostics: ${overall} (${body.durationMs ?? "?"}ms)`, type);
  } catch (e) {
    showStatusMsg("Diagnostics failed: " + e.message, "error");
  }
}

async function checkStatus() {
  try {
    const res = await fetch("http://127.0.0.1:17878/status", {
      headers: {
        "origin": "tauri://localhost",
        "x-connector-token": currentConfig?.pairingToken || "",
      },
    });
    const body = await res.json().catch(() => ({}));
    $("mode-status").textContent    = (body.mode || "?").charAt(0).toUpperCase() + (body.mode || "?").slice(1);
    $("pairing-status").textContent = body.paired ? "Paired" : "Not paired";
    $("printer-status").textContent = body.printer?.ip || (body.printer?.configured ? "Configured" : "Not configured");
    $("hw-status").textContent      = body.hardware?.verified ? "Verified" : "Not verified";
    if (body.mode) {
      const badge = $("mode-badge");
      badge.textContent = body.mode;
      badge.className = `mode-badge ${body.mode}`;
    }
    showStatusMsg(`Status refreshed. Mode: ${body.mode || "?"}, Paired: ${body.paired ? "yes" : "no"}`, "success");
  } catch {
    showStatusMsg("Could not reach connector.", "error");
  }
}

// ── Events tab ────────────────────────────────────────────────────────────────
async function loadEvents() {
  const list = $("event-list");
  try {
    const res = await fetch("http://127.0.0.1:17878/events/recent", {
      headers: {
        "origin": "tauri://localhost",
        "x-connector-token": currentConfig?.pairingToken || "",
      },
    });
    const body = await res.json().catch(() => ({}));
    const events = Array.isArray(body.events) ? body.events : [];
    if (!events.length) {
      list.innerHTML = '<li style="color:#94a3b8;font-size:.88rem;padding:.75rem 0">No events yet.</li>';
      return;
    }
    list.innerHTML = "";
    events.forEach((e) => {
      const li = document.createElement("li");
      li.className = "event-item";
      const icons = { success: "✓", info: "ℹ", warning: "⚠", error: "✗" };
      const icon = icons[e.severity] || "•";
      const ts = e.createdAt ? new Date(e.createdAt * 1000).toLocaleTimeString() : "";
      li.innerHTML = `<span class="ev-icon ${e.severity}">${icon}</span>
        <span><strong>${(e.type || "").replace(/_/g, " ")}</strong>: ${e.message || ""}</span>
        <span class="ev-time">${ts}</span>`;
      list.appendChild(li);
    });
  } catch {
    list.innerHTML = '<li style="color:#dc2626;font-size:.88rem;padding:.75rem 0">Could not load events — connector not running.</li>';
  }
}

// ── Hardware verification wizard ──────────────────────────────────────────────
let hardwareVerifiedInSetup = false;

$("testOpenWizard").addEventListener("click", async () => {
  const btn = $("testOpenWizard");
  btn.disabled = true;
  try {
    const result = await invoke("send_test_open_command");
    $("last-command").textContent = result === "command_sent" ? "Command sent" : result;
    $("hw-retry").classList.add("hidden");
    if (result === "simulation_only") {
      show("Simulation mode is active — no drawer command was sent. Switch to Live mode to test hardware.");
    } else if (result === "diagnostic_only") {
      show("Diagnostic mode is active — no drawer command was sent. Switch to Live mode to test hardware.");
    } else if (result === "printer_not_configured") {
      show("Printer IP is missing. Go back to Setup and enter the printer IP.");
    } else if (result === "command_sent") {
      $("hw-confirm").classList.remove("hidden");
      show("Test command sent. Did the cash drawer open?");
    } else {
      show("Could not reach printer. Check IP and try again.");
    }
  } finally {
    btn.disabled = false;
  }
});

$("hwYes").addEventListener("click", () => {
  hardwareVerifiedInSetup = true;
  $("hw-confirm").classList.add("hidden");
  $("sum-paired").textContent   = currentConfig?.pairingToken ? "Paired" : "Not paired";
  $("sum-printer").textContent  = currentConfig?.printerIp || "Not configured";
  $("sum-hardware").textContent = "Verified";
  showStep(4);
});

$("hwNo").addEventListener("click", () => {
  hardwareVerifiedInSetup = false;
  $("hw-confirm").classList.add("hidden");
  $("hw-retry").classList.remove("hidden");
});

$("hwTryAgain").addEventListener("click", () => $("hw-retry").classList.add("hidden"));

$("hwSkip").addEventListener("click", () => {
  hardwareVerifiedInSetup = false;
  $("hw-retry").classList.add("hidden");
  $("sum-paired").textContent   = currentConfig?.pairingToken ? "Paired" : "Not paired";
  $("sum-printer").textContent  = currentConfig?.printerIp || "Not configured";
  $("sum-hardware").textContent = "Not verified";
  showStep(4);
});

$("finishSetup").addEventListener("click", async () => {
  await saveConfig({ setupComplete: true, hardwareVerified: hardwareVerifiedInSetup });
  await invoke("finish_setup", { hardwareVerified: hardwareVerifiedInSetup });
  show("franchisetech Connector is ready.");
});

// ── Event listeners ───────────────────────────────────────────────────────────
$("save").addEventListener("click", () => saveConfig());
$("generateToken").addEventListener("click", async () => {
  $("pairingToken").value = await invoke("generate_token");
  show("Pairing token generated. Copy it into the franchisetech web app.");
});
$("copyToken").addEventListener("click", async () => {
  const token = $("pairingToken").value;
  if (!token) { show("No pairing token to copy. Generate one first."); return; }
  await writeText(token);
  show("Pairing token copied.");
});
$("findPrinters").addEventListener("click", () => findPrinters("printer-results-main"));
$("findPrintersWizard").addEventListener("click", () => findPrinters("printer-results"));
$("approvePairing").addEventListener("click", approvePairing);
$("denyPairing").addEventListener("click", denyPairing);
$("testConnection").addEventListener("click", testConnection);
$("testConnectionWizard").addEventListener("click", testConnection);
$("testOpen").addEventListener("click", testOpen);
$("runSimulation").addEventListener("click", runSimulationCheck);
$("runDiagnostics").addEventListener("click", runDiagnosticsCheck);
$("checkStatus").addEventListener("click", checkStatus);
$("refreshEvents").addEventListener("click", loadEvents);
$("exportDiagnostics").addEventListener("click", async () => {
  try {
    const path = await invoke("export_diagnostics");
    show("Diagnostics exported to: " + path);
  } catch (e) {
    show("Export failed: " + e);
  }
});
$("openLogs").addEventListener("click", async () => {
  try { await invoke("open_logs"); }
  catch { show("No log file found yet. Use the connector for a while, then try again."); }
});

// Mode selector — auto-save when changed
$("connectorRunMode").addEventListener("change", () => saveConfig());

document.querySelectorAll("[data-step]").forEach((b) => b.addEventListener("click", () => showStep(b.dataset.step)));
document.querySelectorAll("[data-next]").forEach((b) => b.addEventListener("click", () => showStep(b.dataset.next)));

// ── Boot ──────────────────────────────────────────────────────────────────────
load().then(() => showStep(0)).catch(() => show("Configuration error."));
setInterval(() => checkPendingPairing().catch(() => {}), 1500);
