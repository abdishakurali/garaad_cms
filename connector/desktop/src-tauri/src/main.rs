use franchisetech_connector::{
    approve_pairing_request, default_config, deny_pairing_request, discover_network_printers,
    finish_setup_state, generate_pairing_token, is_configured, load_config_from_path, new_state,
    pending_pairing_request, save_config_to_path, send_network_escpos, test_network_connection,
    try_run_server_with_state, AppState, ConnectorConfig, ConnectorRunMode, PendingPairingView,
    PrinterCandidate, VERSION,
};
use std::{fs, path::PathBuf, sync::Arc};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tokio::sync::RwLock;

struct DesktopState {
    config_path: PathBuf,
    log_path: PathBuf,
    config: Arc<RwLock<ConnectorConfig>>,
    connector_state: AppState,
    server_status: Arc<RwLock<String>>,
}

fn app_data_paths(app: &tauri::AppHandle) -> Result<(PathBuf, PathBuf), String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok((dir.join("connector-config.json"), dir.join("connector.log")))
}

fn write_log(path: &PathBuf, message: &str) {
    let line = format!("{} {}\n", chrono_like_timestamp(), message);
    let _ = fs::OpenOptions::new().create(true).append(true).open(path).and_then(|mut f| {
        use std::io::Write;
        f.write_all(line.as_bytes())
    });
}

fn read_last_log_lines(path: &PathBuf, count: usize) -> Vec<String> {
    let Ok(raw) = fs::read_to_string(path) else { return Vec::new() };
    let lines: Vec<String> = raw.lines().map(|line| line.to_string()).collect();
    lines.into_iter().rev().take(count).collect::<Vec<_>>().into_iter().rev().collect()
}

fn configure_autostart(app: &AppHandle, enabled: bool) -> Result<bool, String> {
    let autostart = app.autolaunch();
    if enabled {
        autostart.enable().map_err(|e| e.to_string())?;
    } else {
        autostart.disable().map_err(|e| e.to_string())?;
    }
    autostart.is_enabled().map_err(|e| e.to_string())
}

fn chrono_like_timestamp() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string())
}

#[tauri::command]
async fn get_config(state: State<'_, DesktopState>) -> Result<ConnectorConfig, String> {
    Ok(state.config.read().await.clone())
}

#[tauri::command]
async fn save_config(config: ConnectorConfig, state: State<'_, DesktopState>, app: AppHandle) -> Result<(), String> {
    let autostart_message = match configure_autostart(&app, config.start_at_login) {
        Ok(_) => None,
        Err(error) => Some(format!("Connector installed, but automatic startup could not be enabled. Open franchisetech Connector manually. {error}")),
    };
    save_config_to_path(&state.config_path, &config)?;
    *state.config.write().await = config;
    write_log(&state.log_path, "config saved");
    autostart_message.map_or(Ok(()), Err)
}

#[tauri::command]
fn generate_token() -> String {
    generate_pairing_token()
}

#[tauri::command]
async fn get_pending_pairing(state: State<'_, DesktopState>) -> Result<Option<PendingPairingView>, String> {
    Ok(pending_pairing_request(&state.connector_state).await)
}

#[tauri::command]
async fn approve_pairing(state: State<'_, DesktopState>) -> Result<String, String> {
    let token = approve_pairing_request(&state.connector_state).await?;
    let config = state.config.read().await.clone();
    save_config_to_path(&state.config_path, &config)?;
    write_log(&state.log_path, "pairing approved");
    Ok(token)
}

#[tauri::command]
async fn deny_pairing(state: State<'_, DesktopState>) -> Result<(), String> {
    deny_pairing_request(&state.connector_state).await?;
    write_log(&state.log_path, "pairing denied");
    Ok(())
}

#[tauri::command]
async fn test_printer_connection(state: State<'_, DesktopState>) -> Result<String, String> {
    let config = state.config.read().await.clone();
    if !is_configured(&config) {
        write_log(&state.log_path, "test failed PRINTER_NOT_CONFIGURED");
        return Ok("printer_not_configured".to_string());
    }
    let ok = test_network_connection(&config.printer_ip, config.printer_port).await.is_ok();
    write_log(&state.log_path, if ok { "printer connection ok" } else { "printer connection failed" });
    Ok(if ok { "reachable".to_string() } else { "unreachable".to_string() })
}

#[tauri::command]
async fn send_test_open_command(state: State<'_, DesktopState>) -> Result<String, String> {
    let config = state.config.read().await.clone();
    match config.connector_run_mode {
        ConnectorRunMode::Simulation => {
            write_log(&state.log_path, "send_test_open_command: simulation mode, no bytes sent");
            return Ok("simulation_only".to_string());
        }
        ConnectorRunMode::Diagnostic => {
            write_log(&state.log_path, "send_test_open_command: diagnostic mode, no bytes sent");
            return Ok("diagnostic_only".to_string());
        }
        ConnectorRunMode::Live => {}
    }
    if !is_configured(&config) {
        write_log(&state.log_path, "open command failed PRINTER_NOT_CONFIGURED");
        return Ok("printer_not_configured".to_string());
    }
    let ok = send_network_escpos(&config.printer_ip, config.printer_port).await.is_ok();
    write_log(&state.log_path, if ok { "open command sent" } else { "open command failed" });
    Ok(if ok { "command_sent".to_string() } else { "failed".to_string() })
}

#[tauri::command]
async fn find_network_printers() -> Result<Vec<PrinterCandidate>, String> {
    Ok(discover_network_printers().await)
}

#[tauri::command]
async fn use_printer(candidate: PrinterCandidate, state: State<'_, DesktopState>) -> Result<(), String> {
    let mut config = state.config.write().await;
    config.printer_type = "network_escpos".to_string();
    config.printer_ip = candidate.ip;
    config.printer_port = candidate.port;
    config.drawer_kick_command = "1B700019FA".to_string();
    save_config_to_path(&state.config_path, &config)?;
    write_log(&state.log_path, "printer selected");
    Ok(())
}

#[tauri::command]
async fn finish_setup(hardware_verified: bool, state: State<'_, DesktopState>) -> Result<(), String> {
    finish_setup_state(&state.connector_state, hardware_verified).await;
    let config = state.config.read().await.clone();
    save_config_to_path(&state.config_path, &config)?;
    write_log(&state.log_path, &format!("setup complete hw_verified={}", hardware_verified));
    Ok(())
}

#[tauri::command]
async fn export_diagnostics(state: State<'_, DesktopState>) -> Result<String, String> {
    let config = state.config.read().await.clone();
    let diagnostics_path = state.log_path.with_file_name("franchisetech-connector-diagnostics.json");
    let body = serde_json::json!({
        "name": "franchisetech Connector",
        "version": VERSION,
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "serverBindStatus": state.server_status.read().await.clone(),
        "config": {
            "printerType": config.printer_type,
            "printerIp": config.printer_ip,
            "printerPort": config.printer_port,
            "drawerKickCommand": config.drawer_kick_command,
            "allowedOrigins": config.allowed_origins,
            "devMode": config.dev_mode,
            "startAtLogin": config.start_at_login,
            "setupComplete": config.setup_complete,
            "pairingTokenConfigured": !config.pairing_token.is_empty()
        },
        "lastLogLines": read_last_log_lines(&state.log_path, 50)
    });
    fs::write(&diagnostics_path, serde_json::to_string_pretty(&body).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    write_log(&state.log_path, "diagnostics exported");
    Ok(diagnostics_path.to_string_lossy().to_string())
}

#[tauri::command]
fn open_logs(state: State<'_, DesktopState>) -> Result<(), String> {
    tauri_plugin_opener::open_path(state.log_path.to_string_lossy().to_string(), None::<String>).map_err(|e| e.to_string())
}

fn main() {
    tracing_subscriber::fmt::init();
    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec![])))
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let (config_path, log_path) = app_data_paths(app.handle())?;
            let config = if config_path.exists() {
                load_config_from_path(config_path.to_str().unwrap_or_default())
            } else {
                let cfg = default_config();
                save_config_to_path(&config_path, &cfg)?;
                cfg
            };
            if let Err(error) = configure_autostart(app.handle(), config.start_at_login) {
                write_log(&log_path, &format!("autostart error {error}"));
            }
            write_log(&log_path, "app started");
            let connector_state = new_state(config.clone());
            let live_config = connector_state.config.clone();
            let managed_connector_state = connector_state.clone();
            let server_log_path = log_path.clone();
            let server_status = Arc::new(RwLock::new("Starting".to_string()));
            let server_status_for_task = server_status.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = try_run_server_with_state(connector_state).await {
                    write_log(&server_log_path, &format!("server error {error}"));
                    *server_status_for_task.write().await = format!("Error: {error}");
                }
            });
            let server_status_ready = server_status.clone();
            tauri::async_runtime::spawn(async move {
                *server_status_ready.write().await = "Running on 127.0.0.1:17878".to_string();
            });
            write_log(&log_path, &format!("server started version {}", VERSION));
            app.manage(DesktopState {
                config_path,
                log_path,
                config: live_config,
                connector_state: managed_connector_state,
                server_status,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            generate_token,
            get_pending_pairing,
            approve_pairing,
            deny_pairing,
            test_printer_connection,
            send_test_open_command,
            find_network_printers,
            use_printer,
            finish_setup,
            export_diagnostics,
            open_logs
        ])
        .run(tauri::generate_context!())
        .expect("error while running franchisetech Connector");
}
