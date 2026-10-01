use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, Rect};
use tauri_plugin_updater::UpdaterExt;
use uuid::Uuid;

mod database;
use database::Database;
mod automation_hooks;
#[cfg(test)]
mod cloud_e2e;
mod cloud_sync;
#[cfg(target_os = "windows")]
mod native_tray;
mod obsidian;
mod recurring;
mod reports;
mod seatable;
mod settings;
mod subjects;
mod supabase;
mod tasks;
mod time_tracking;
mod today_overview;
mod unassigned;

#[cfg(target_os = "windows")]
use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetCursorPos, GetWindowRect, GetWindowThreadProcessId, SetForegroundWindow,
    ShowWindow, SW_RESTORE,
};

const HOVER_SHOW_DELAY: Duration = Duration::from_millis(300);
const HOVER_HIDE_DELAY: Duration = Duration::from_secs(3);
const HOVER_POSITION_CHECK_INTERVAL: Duration = Duration::from_millis(150);
const HOVER_WINDOW_GAP: i32 = 8;
const TRAY_MENU_SUPPRESS: Duration = Duration::from_millis(1200);
const UPDATE_CHECK_MENU_LABEL: &str = "检查更新";
static HOVER_STATE: OnceLock<Arc<Mutex<HoverState>>> = OnceLock::new();
static UPDATE_CHECK_RUNNING: AtomicBool = AtomicBool::new(false);

#[derive(Debug)]
struct HoverState {
    generation: u64,
    pending: bool,
    shown: bool,
    suppress_until: Instant,
}

#[derive(Clone)]
struct TrayTimerMenu {
    start: MenuItem<tauri::Wry>,
    pause: MenuItem<tauri::Wry>,
    resume: MenuItem<tauri::Wry>,
    stop: MenuItem<tauri::Wry>,
}

struct AppTrayIcon {
    _icon: TrayIcon<tauri::Wry>,
}

#[derive(Clone, Copy)]
enum TrayTimerAction {
    Start,
    Pause,
    Resume,
    Stop,
}

#[derive(Clone, Copy, Debug)]
struct PhysicalBounds {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

impl PhysicalBounds {
    fn contains(self, point: PhysicalPosition<i32>) -> bool {
        let x = i64::from(point.x);
        let y = i64::from(point.y);
        let left = i64::from(self.x);
        let top = i64::from(self.y);
        x >= left
            && x < left + i64::from(self.width)
            && y >= top
            && y < top + i64::from(self.height)
    }
}

fn hover_window_position(
    tray: PhysicalBounds,
    window_size: tauri::PhysicalSize<u32>,
    work_area: PhysicalBounds,
) -> PhysicalPosition<i32> {
    let window_width = i64::from(window_size.width);
    let window_height = i64::from(window_size.height);
    let gap = i64::from(HOVER_WINDOW_GAP);
    let tray_left = i64::from(tray.x);
    let tray_top = i64::from(tray.y);
    let tray_right = tray_left + i64::from(tray.width);
    let tray_bottom = tray_top + i64::from(tray.height);
    let centered_x = tray_left + (i64::from(tray.width) - window_width) / 2;
    let centered_y = tray_top + (i64::from(tray.height) - window_height) / 2;

    let candidates = [
        (centered_x, tray_top - window_height - gap),
        (centered_x, tray_bottom + gap),
        (tray_left - window_width - gap, centered_y),
        (tray_right + gap, centered_y),
    ];
    let (x, y) = candidates
        .into_iter()
        .find(|&(x, y)| bounds_contain_window(work_area, x, y, window_width, window_height))
        .unwrap_or(candidates[0]);

    let min_x = i64::from(work_area.x);
    let min_y = i64::from(work_area.y);
    let max_x = min_x + i64::from(work_area.width) - window_width;
    let max_y = min_y + i64::from(work_area.height) - window_height;
    PhysicalPosition::new(
        clamp_coordinate(x, min_x, max_x),
        clamp_coordinate(y, min_y, max_y),
    )
}

fn bounds_contain_window(bounds: PhysicalBounds, x: i64, y: i64, width: i64, height: i64) -> bool {
    let left = i64::from(bounds.x);
    let top = i64::from(bounds.y);
    x >= left
        && y >= top
        && x + width <= left + i64::from(bounds.width)
        && y + height <= top + i64::from(bounds.height)
}

fn clamp_coordinate(value: i64, min: i64, max: i64) -> i32 {
    value.clamp(min, max.max(min)) as i32
}

fn window_size_for_scale(
    size: tauri::PhysicalSize<u32>,
    current_scale: f64,
    target_scale: f64,
) -> tauri::PhysicalSize<u32> {
    if current_scale <= 0.0 || target_scale <= 0.0 {
        return size;
    }
    tauri::PhysicalSize::new(
        (f64::from(size.width) / current_scale * target_scale).round() as u32,
        (f64::from(size.height) / current_scale * target_scale).round() as u32,
    )
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show(app, "main");
        }))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir()?;
            let database = initialize_database(&app_data_dir)?;
            automation_hooks::recover_interrupted_runs(&database).map_err(std::io::Error::other)?;
            cloud_sync::spawn_background_services(database.clone(), app.handle().clone());
            app.manage(database.clone());
            let state = Arc::new(Mutex::new(HoverState {
                generation: 0,
                pending: false,
                shown: false,
                suppress_until: Instant::now(),
            }));
            let _ = HOVER_STATE.set(Arc::clone(&state));
            let show_main = MenuItem::with_id(app, "show_main", "打开工作台", true, None::<&str>)?;
            let timer_menu = TrayTimerMenu {
                start: MenuItem::with_id(app, "start_timer", "开始计时", true, None::<&str>)?,
                pause: MenuItem::with_id(app, "pause_timer", "暂停计时", false, None::<&str>)?,
                resume: MenuItem::with_id(app, "resume_timer", "继续计时", false, None::<&str>)?,
                stop: MenuItem::with_id(app, "stop_timer", "结束本段", false, None::<&str>)?,
            };
            let show_today = MenuItem::with_id(app, "show_today", "查看今日", true, None::<&str>)?;
            let check_update =
                MenuItem::with_id(app, "check_update", UPDATE_CHECK_MENU_LABEL, true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(
                app,
                &[
                    &show_main,
                    &timer_menu.start,
                    &timer_menu.pause,
                    &timer_menu.resume,
                    &timer_menu.stop,
                    &show_today,
                    &check_update,
                    &quit,
                ],
            )?;
            refresh_tray_timer_menu(&database, &timer_menu);
            spawn_tray_timer_menu_refresh(database.clone(), timer_menu.clone());
            let tray_icon_image = app
                .default_window_icon()
                .cloned()
                .ok_or_else(|| std::io::Error::other("应用默认图标未配置"))?;
            let tray_icon = TrayIconBuilder::with_id("timegenie")
                .icon(tray_icon_image)
                .menu(&menu)
                .tooltip("时序")
                .show_menu_on_left_click(false)
                .on_menu_event({
                    let database = database.clone();
                    let timer_menu = timer_menu.clone();
                    let check_update = check_update.clone();
                    move |app, event| {
                        suppress_hover();
                        hide(app, "hover");
                        match event.id().as_ref() {
                            "show_main" | "show_today" => show(app, "main"),
                            "check_update" => {
                                run_tray_update_check(app.clone(), check_update.clone())
                            }
                            "start_timer" => run_tray_timer_action(
                                app.clone(),
                                database.clone(),
                                timer_menu.clone(),
                                TrayTimerAction::Start,
                            ),
                            "pause_timer" => run_tray_timer_action(
                                app.clone(),
                                database.clone(),
                                timer_menu.clone(),
                                TrayTimerAction::Pause,
                            ),
                            "resume_timer" => run_tray_timer_action(
                                app.clone(),
                                database.clone(),
                                timer_menu.clone(),
                                TrayTimerAction::Resume,
                            ),
                            "stop_timer" => run_tray_timer_action(
                                app.clone(),
                                database.clone(),
                                timer_menu.clone(),
                                TrayTimerAction::Stop,
                            ),
                            "quit" => app.exit(0),
                            _ => {}
                        }
                    }
                })
                .on_tray_icon_event({
                    let state = Arc::clone(&state);
                    move |tray, event| match event {
                        TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        } => {
                            cancel_hover(&state);
                            hide(tray.app_handle(), "hover");
                            show(tray.app_handle(), "main");
                        }
                        TrayIconEvent::Click {
                            button: MouseButton::Right,
                            ..
                        } => {
                            refresh_tray_timer_menu(
                                tray.app_handle().state::<Database>().inner(),
                                &timer_menu,
                            );
                            suppress_hover();
                            hide(tray.app_handle(), "hover");
                        }
                        TrayIconEvent::Enter { position, rect, .. } => {
                            schedule_hover(tray.app_handle().clone(), state.clone(), rect, position)
                        }
                        TrayIconEvent::Leave { .. } => cancel_pending_hover(&state),
                        _ => {}
                    }
                })
                .build(app)?;
            app.manage(AppTrayIcon { _icon: tray_icon });
            retry_tray_registration(app.handle().clone());
            #[cfg(target_os = "windows")]
            native_tray::start_if_tauri_tray_missing(app.handle().clone());
            verify_tray_registered(app.handle().clone());
            show(app.handle(), "main");
            show_main_after_startup(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            window_show,
            window_hide,
            window_close_main,
            window_start_dragging_main,
            window_minimize_main,
            window_toggle_maximize_main,
            window_is_main_maximized,
            window_open_main_overview,
            window_hide_hover_after_keyboard_close,
            database::database_status,
            automation_hooks::automation_hook_list,
            automation_hooks::automation_hook_create,
            automation_hooks::automation_hook_update,
            automation_hooks::automation_hook_set_enabled,
            automation_hooks::automation_hook_reorder,
            automation_hooks::automation_hook_delete,
            automation_hooks::automation_hook_run_list,
            automation_hooks::automation_hook_run_get,
            automation_hooks::automation_hook_test,
            settings::settings_get,
            settings::settings_update,
            settings::integration_config_update,
            settings::integration_secret_set,
            settings::integration_secret_clear,
            supabase::storage_mode_get,
            supabase::storage_mode_set_local,
            supabase::cloud_configure,
            supabase::cloud_sign_in_password,
            supabase::cloud_sign_out,
            supabase::cloud_session_get,
            supabase::cloud_workspace_bootstrap,
            supabase::cloud_device_register,
            supabase::storage_migration_preview,
            supabase::storage_migration_execute,
            cloud_sync::cloud_sync_status,
            cloud_sync::cloud_sync_pull,
            cloud_sync::cloud_sync_push,
            cloud_sync::cloud_sync_refresh,
            cloud_sync::cloud_sync_conflicts,
            cloud_sync::cloud_sync_resolve_conflict,
            cloud_sync::tracking_lease_acquire,
            cloud_sync::tracking_lease_renew,
            cloud_sync::tracking_lease_release,
            cloud_sync::tracking_lease_get,
            cloud_sync::cloud_timer_start,
            cloud_sync::cloud_timer_get_state,
            cloud_sync::cloud_time_entry_list,
            cloud_sync::cloud_timer_pause,
            cloud_sync::cloud_timer_resume,
            cloud_sync::cloud_timer_stop,
            cloud_sync::cloud_unassigned_get_state,
            cloud_sync::cloud_unassigned_resolve_work,
            cloud_sync::cloud_unassigned_resolve_break,
            cloud_sync::cloud_unassigned_discard,
            subjects::subject_list,
            subjects::subject_create,
            subjects::subject_rename,
            tasks::task_list,
            tasks::task_daily_estimate_set,
            tasks::task_create,
            tasks::task_create_quick,
            tasks::task_update,
            tasks::task_set_completed,
            tasks::task_delete_subtree,
            tasks::task_reorder_subtree,
            tasks::task_change_parent,
            tasks::task_duplicate_subtree,
            today_overview::today_work_overview_get,
            recurring::task_recurrence_save,
            recurring::task_recurrence_close,
            tasks::plan_import_preview,
            obsidian::obsidian_plan_import_preview,
            obsidian::obsidian_plan_import_confirm,
            obsidian::obsidian_report_write_preview,
            obsidian::obsidian_report_write_execute,
            time_tracking::timer_get_state,
            time_tracking::timer_start,
            time_tracking::timer_pause,
            time_tracking::timer_resume,
            time_tracking::timer_stop,
            time_tracking::time_entry_list,
            time_tracking::time_entry_create_manual,
            time_tracking::time_entry_update,
            time_tracking::time_allocation_replace,
            unassigned::unassigned_get_state,
            unassigned::unassigned_resolve_work,
            unassigned::unassigned_resolve_break,
            unassigned::unassigned_discard,
            reports::report_list,
            reports::report_task_suggestions,
            reports::report_create,
            reports::report_update_scope,
            reports::report_save_content,
            reports::report_regenerate,
            reports::report_delete,
            reports::report_public_text,
            reports::report_template_get,
            reports::report_template_save,
            seatable::seatable_connection_test,
            seatable::seatable_task_sync_preview,
            seatable::seatable_task_sync_execute,
            seatable::seatable_sync_retry_failed,
            seatable::seatable_reimbursements_query
        ])
        .run(tauri::generate_context!())
        .expect("error while running TimeGenie");
}

fn initialize_database(app_data_dir: &std::path::Path) -> Result<Database, std::io::Error> {
    if cfg!(debug_assertions) && std::env::var_os("TG_USE_APP_DATA_DIR").is_none() {
        let dev_dir = std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .join("target")
            .join("dev-data");
        return Database::initialize(&dev_dir).map_err(|error| {
            std::io::Error::other(format!(
                "数据库初始化失败: 开发目录 {} 失败: {error}",
                dev_dir.display()
            ))
        });
    }

    match Database::initialize(app_data_dir) {
        Ok(database) => Ok(database),
        Err(error) if cfg!(debug_assertions) => {
            let fallback_dir = std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."))
                .join("target")
                .join("dev-data");
            eprintln!(
                "数据库初始化失败，开发模式改用临时目录 {}。原错误: {error}",
                fallback_dir.display()
            );
            Database::initialize(&fallback_dir).map_err(|fallback_error| {
                std::io::Error::other(format!(
                    "数据库初始化失败: 默认目录 {} 失败: {error}; 开发临时目录 {} 也失败: {fallback_error}",
                    app_data_dir.display(),
                    fallback_dir.display()
                ))
            })
        }
        Err(error) => Err(std::io::Error::other(format!(
            "数据库初始化失败: 默认目录 {} 失败: {error}",
            app_data_dir.display()
        ))),
    }
}

#[tauri::command]
fn window_show(app: AppHandle, label: String) {
    show(&app, &label);
}

#[tauri::command]
fn window_hide(app: AppHandle, label: String) {
    hide(&app, &label);
}

#[tauri::command]
fn window_close_main(app: AppHandle) {
    hide(&app, "main");
}

#[tauri::command]
fn window_start_dragging_main(app: AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.start_dragging();
    }
}

#[tauri::command]
fn window_minimize_main(app: AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.minimize();
    }
}

#[tauri::command]
fn window_toggle_maximize_main(app: AppHandle) -> bool {
    let Some(window) = app.get_webview_window("main") else {
        return false;
    };
    if window.is_maximized().unwrap_or(false) {
        let _ = window.unmaximize();
    } else {
        let _ = window.maximize();
    }
    window.is_maximized().unwrap_or(false)
}

#[tauri::command]
fn window_is_main_maximized(app: AppHandle) -> bool {
    app.get_webview_window("main")
        .and_then(|window| window.is_maximized().ok())
        .unwrap_or(false)
}

#[tauri::command]
fn window_open_main_overview(app: AppHandle) {
    hide(&app, "hover");
    show(&app, "main");
}

#[tauri::command]
fn window_hide_hover_after_keyboard_close(app: AppHandle) {
    suppress_hover();
    hide(&app, "hover");
}

fn show_main_after_startup(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        for delay in [350, 850, 1_500, 2_400, 3_600] {
            tokio::time::sleep(Duration::from_millis(delay)).await;
            let app_for_show = app.clone();
            if let Err(error) = app.run_on_main_thread(move || show(&app_for_show, "main")) {
                eprintln!("调度主窗口显示失败: {error}");
            }
        }
    });
}

fn verify_tray_registered(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(600)).await;
        if app.tray_by_id("timegenie").is_none() {
            eprintln!("托盘图标创建后无法从应用中取回");
        }
    });
}

fn retry_tray_registration(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        for delay in [300_u64, 1_000, 2_000, 4_000] {
            tokio::time::sleep(Duration::from_millis(delay)).await;
            let Some(tray) = app.tray_by_id("timegenie") else {
                eprintln!("托盘对象不存在，无法重新注册");
                continue;
            };
            match tray.set_visible(true) {
                Ok(()) => eprintln!("托盘图标重新注册请求已发送 ({delay}ms)"),
                Err(error) => eprintln!("托盘图标重新注册失败 ({delay}ms): {error}"),
            }
        }
    });
}

fn run_tray_update_check(app: AppHandle, menu_item: MenuItem<tauri::Wry>) {
    if UPDATE_CHECK_RUNNING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }

    let _ = menu_item.set_enabled(false);
    let _ = menu_item.set_text("正在检查更新...");
    tauri::async_runtime::spawn(async move {
        let result = check_download_and_install_update(&app, &menu_item).await;
        UPDATE_CHECK_RUNNING.store(false, Ordering::SeqCst);

        match result {
            Ok(UpdateCheckResult::NoUpdate) => {
                set_update_menu_status(menu_item, "已是最新版本", true, true);
            }
            Ok(UpdateCheckResult::Installed { version }) => {
                let text = format!("更新 {version} 已安装，重启生效");
                set_update_menu_status(menu_item, text, true, false);
            }
            Err(error) => {
                eprintln!("检查更新失败: {error}");
                set_update_menu_status(menu_item, "检查更新失败", true, true);
            }
        }
    });
}

enum UpdateCheckResult {
    NoUpdate,
    Installed { version: String },
}

async fn check_download_and_install_update(
    app: &AppHandle,
    menu_item: &MenuItem<tauri::Wry>,
) -> Result<UpdateCheckResult, String> {
    let updater = app.updater().map_err(|error| error.to_string())?;
    let Some(update) = updater.check().await.map_err(|error| error.to_string())? else {
        return Ok(UpdateCheckResult::NoUpdate);
    };

    let version = update.version.clone();
    let _ = menu_item.set_text(format!("正在下载更新 {version}..."));
    update
        .download_and_install(
            |_, _| {},
            || {},
        )
        .await
        .map_err(|error| error.to_string())?;
    Ok(UpdateCheckResult::Installed { version })
}

fn set_update_menu_status(
    menu_item: MenuItem<tauri::Wry>,
    text: impl Into<String>,
    enabled: bool,
    reset_text_later: bool,
) {
    let _ = menu_item.set_enabled(enabled);
    let _ = menu_item.set_text(text.into());
    if reset_text_later {
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_secs(4)).await;
            let _ = menu_item.set_text(UPDATE_CHECK_MENU_LABEL);
        });
    }
}

fn spawn_tray_timer_menu_refresh(database: Database, menu: TrayTimerMenu) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;
            refresh_tray_timer_menu(&database, &menu);
        }
    });
}

fn refresh_tray_timer_menu(database: &Database, menu: &TrayTimerMenu) {
    let state = time_tracking::get_timer_state(database)
        .ok()
        .flatten()
        .map(|entry| entry.state);
    let running = state.as_deref() == Some("running");
    let paused = state.as_deref() == Some("paused");
    let _ = menu.start.set_enabled(state.is_none());
    let _ = menu.pause.set_enabled(running);
    let _ = menu.resume.set_enabled(paused);
    let _ = menu.stop.set_enabled(running || paused);
}

fn run_tray_timer_action(
    app: AppHandle,
    database: Database,
    menu: TrayTimerMenu,
    action: TrayTimerAction,
) {
    tauri::async_runtime::spawn_blocking(move || {
        let result = execute_tray_timer_action(&database, action);
        refresh_tray_timer_menu(&database, &menu);
        match result {
            Ok(timer) => {
                let _ = app.emit("timer-state-changed", timer);
                let _ = app.emit(
                    "work-data-changed",
                    serde_json::json!({
                        "revision": 0,
                        "domains": ["tasks", "time", "unassigned"],
                        "source": "tray"
                    }),
                );
            }
            Err(error) => {
                let _ = app.emit("tray-action-error", error);
            }
        }
    });
}

#[cfg(target_os = "windows")]
fn run_native_tray_timer_action(app: &AppHandle, action: TrayTimerAction) {
    let app = app.clone();
    let database = app.state::<Database>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        match execute_tray_timer_action(&database, action) {
            Ok(timer) => {
                let _ = app.emit("timer-state-changed", timer);
                let _ = app.emit(
                    "work-data-changed",
                    serde_json::json!({
                        "revision": 0,
                        "domains": ["tasks", "time", "unassigned"],
                        "source": "native-tray"
                    }),
                );
            }
            Err(error) => {
                let _ = app.emit("tray-action-error", error);
            }
        }
    });
}

fn execute_tray_timer_action(
    database: &Database,
    action: TrayTimerAction,
) -> Result<serde_json::Value, String> {
    let storage = supabase::storage_mode(database)?;
    if storage.mode == "cloud" {
        return execute_cloud_tray_timer_action(database, action);
    }
    execute_local_tray_timer_action(database, action)
}

fn execute_local_tray_timer_action(
    database: &Database,
    action: TrayTimerAction,
) -> Result<serde_json::Value, String> {
    let current = time_tracking::get_timer_state(database)?;
    let result = match action {
        TrayTimerAction::Start => time_tracking::start_timer_with_hooks(
            database,
            time_tracking::TimerStartRequest {
                task_id: None,
                note: None,
                client_request_id: Uuid::now_v7().to_string(),
            },
        )?,
        TrayTimerAction::Pause => {
            let entry =
                current.ok_or_else(|| "TIMER_NOT_RUNNING: 当前没有正在运行的计时".to_string())?;
            time_tracking::pause_timer(
                database,
                time_tracking::TimerVersionRequest {
                    entry_id: entry.id,
                    expected_version: entry.version,
                },
            )?
        }
        TrayTimerAction::Resume => {
            let entry =
                current.ok_or_else(|| "TIMER_NOT_PAUSED: 当前没有已暂停的计时".to_string())?;
            time_tracking::resume_timer(
                database,
                time_tracking::TimerResumeRequest {
                    entry_id: entry.id,
                    expected_version: entry.version,
                    operation_id: Uuid::now_v7().to_string(),
                },
            )?
        }
        TrayTimerAction::Stop => {
            let entry =
                current.ok_or_else(|| "TIMER_NOT_RUNNING: 当前没有可结束的计时".to_string())?;
            time_tracking::stop_timer_with_hooks(
                database,
                time_tracking::TimerStopRequest {
                    entry_id: entry.id,
                    expected_version: entry.version,
                    create_default_allocation: true,
                },
            )?
        }
    };
    serde_json::to_value(result).map_err(|error| error.to_string())
}

fn execute_cloud_tray_timer_action(
    database: &Database,
    action: TrayTimerAction,
) -> Result<serde_json::Value, String> {
    if matches!(action, TrayTimerAction::Start) {
        return cloud_sync::timer_start(
            database,
            cloud_sync::CloudTimerStartRequest {
                task_id: None,
                note: None,
                operation_id: Uuid::now_v7().to_string(),
            },
        );
    }
    let entry = cloud_sync::timer_get_state(database)?
        .ok_or_else(|| "TIMER_NOT_RUNNING: 当前没有可操作的云端计时".to_string())?;
    let entry_id = entry
        .get("id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "CLOUD_REQUEST_FAILED: 云端计时缺少 ID".to_string())?;
    let expected_version = entry
        .get("version")
        .and_then(serde_json::Value::as_i64)
        .ok_or_else(|| "CLOUD_REQUEST_FAILED: 云端计时缺少版本号".to_string())?;
    let rpc = match action {
        TrayTimerAction::Pause => "timer_pause",
        TrayTimerAction::Resume => "timer_resume",
        TrayTimerAction::Stop => "timer_stop",
        TrayTimerAction::Start => unreachable!(),
    };
    cloud_sync::timer_action(
        database,
        rpc,
        cloud_sync::CloudTimerVersionRequest {
            entry_id: entry_id.to_string(),
            expected_version,
            operation_id: Uuid::now_v7().to_string(),
        },
    )
}

fn show(app: &AppHandle, label: &str) {
    if let Some(window) = app.get_webview_window(label) {
        if let Err(error) = window.unminimize() {
            eprintln!("窗口 {label} 取消最小化失败: {error}");
        }
        if let Err(error) = window.show() {
            eprintln!("窗口 {label} 显示失败: {error}");
        }
        if label == "main" {
            force_show_main_window(&window);
            if let Err(error) = window.set_focus() {
                eprintln!("窗口 {label} 聚焦失败: {error}");
            }
        }
    } else {
        eprintln!("窗口 {label} 不存在，无法显示");
    }
}

#[cfg(target_os = "windows")]
fn force_show_main_window(_: &tauri::WebviewWindow) -> bool {
    struct MainWindowSearch {
        process_id: u32,
        hwnd: HWND,
        area: i64,
    }

    unsafe extern "system" fn collect_window(window: HWND, data: isize) -> i32 {
        let context = &mut *(data as *mut MainWindowSearch);
        let mut owner_process_id = 0_u32;
        GetWindowThreadProcessId(window, &mut owner_process_id);
        if owner_process_id != context.process_id {
            return 1;
        }

        let mut rect = RECT::default();
        if GetWindowRect(window, &mut rect) == 0 {
            return 1;
        }

        let width = i64::from(rect.right - rect.left);
        let height = i64::from(rect.bottom - rect.top);
        let area = width * height;
        if width > 300 && height > 300 && area > context.area {
            context.hwnd = window;
            context.area = area;
        }
        1
    }

    let mut context = MainWindowSearch {
        process_id: std::process::id(),
        hwnd: std::ptr::null_mut(),
        area: 0,
    };
    unsafe {
        EnumWindows(Some(collect_window), &mut context as *mut _ as isize);
        if !context.hwnd.is_null() {
            ShowWindow(context.hwnd, SW_RESTORE);
            SetForegroundWindow(context.hwnd);
            return true;
        }
    }
    false
}

#[cfg(not(target_os = "windows"))]
fn force_show_main_window(_: &tauri::WebviewWindow) -> bool {
    true
}

fn hide(app: &AppHandle, label: &str) {
    if label == "hover" {
        mark_hover_hidden();
    }
    if let Some(window) = app.get_webview_window(label) {
        let _ = window.hide();
    }
}

fn cancel_pending_hover(state: &Arc<Mutex<HoverState>>) {
    if let Ok(mut state) = state.lock() {
        if state.pending {
            state.generation = state.generation.wrapping_add(1);
            state.pending = false;
        }
    }
}

fn cancel_hover(state: &Arc<Mutex<HoverState>>) {
    if let Ok(mut state) = state.lock() {
        state.generation = state.generation.wrapping_add(1);
        state.pending = false;
        state.shown = false;
    }
}

fn mark_hover_hidden() {
    if let Some(state) = HOVER_STATE.get() {
        cancel_hover(state);
    }
}

fn suppress_hover() {
    if let Some(state) = HOVER_STATE.get() {
        if let Ok(mut state) = state.lock() {
            state.generation = state.generation.wrapping_add(1);
            state.pending = false;
            state.shown = false;
            state.suppress_until = Instant::now() + TRAY_MENU_SUPPRESS;
        }
    }
}

fn schedule_hover(
    app: AppHandle,
    state: Arc<Mutex<HoverState>>,
    rect: Rect,
    fallback: PhysicalPosition<f64>,
) {
    let generation = {
        let Ok(mut state) = state.lock() else {
            return;
        };
        if Instant::now() < state.suppress_until || state.pending || state.shown {
            return;
        }
        state.generation = state.generation.wrapping_add(1);
        state.pending = true;
        state.generation
    };
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(HOVER_SHOW_DELAY).await;
        if let Some(window) = app.get_webview_window("hover") {
            let scale_factor = window.scale_factor().unwrap_or(1.0);
            let anchor = rect.position.to_physical::<i32>(scale_factor);
            let anchor_size = rect.size.to_physical::<u32>(scale_factor);
            let tray_bounds = if anchor_size.width > 0 && anchor_size.height > 0 {
                PhysicalBounds {
                    x: anchor.x,
                    y: anchor.y,
                    width: anchor_size.width,
                    height: anchor_size.height,
                }
            } else {
                PhysicalBounds {
                    x: fallback.x as i32 - 16,
                    y: fallback.y as i32 - 16,
                    width: 32,
                    height: 32,
                }
            };

            if cursor_position().is_some_and(|position| !tray_bounds.contains(position)) {
                cancel_pending_hover(&state);
                return;
            }

            let current_window_size = window.outer_size().unwrap_or_else(|_| {
                tauri::PhysicalSize::new(
                    (380.0 * scale_factor).round() as u32,
                    (300.0 * scale_factor).round() as u32,
                )
            });
            let monitor = window
                .monitor_from_point(
                    f64::from(tray_bounds.x) + f64::from(tray_bounds.width) / 2.0,
                    f64::from(tray_bounds.y) + f64::from(tray_bounds.height) / 2.0,
                )
                .ok()
                .flatten()
                .or_else(|| window.primary_monitor().ok().flatten());
            let position = monitor
                .map(|monitor| {
                    let work_area = monitor.work_area();
                    let window_size = window_size_for_scale(
                        current_window_size,
                        scale_factor,
                        monitor.scale_factor(),
                    );
                    hover_window_position(
                        tray_bounds,
                        window_size,
                        PhysicalBounds {
                            x: work_area.position.x,
                            y: work_area.position.y,
                            width: work_area.size.width,
                            height: work_area.size.height,
                        },
                    )
                })
                .unwrap_or_else(|| {
                    PhysicalPosition::new(
                        tray_bounds.x - current_window_size.width as i32,
                        tray_bounds.y - current_window_size.height as i32 - HOVER_WINDOW_GAP,
                    )
                });
            let _ = window.set_position(position);

            let should_show = state
                .lock()
                .map(|mut state| {
                    let valid = state.generation == generation && state.pending;
                    state.pending = false;
                    state.shown = valid;
                    valid
                })
                .unwrap_or(false);
            if !should_show {
                return;
            }

            let _ = window.show();
            monitor_hover_position(app, state, generation, tray_bounds);
        }
    });
}

fn monitor_hover_position(
    app: AppHandle,
    state: Arc<Mutex<HoverState>>,
    generation: u64,
    tray_bounds: PhysicalBounds,
) {
    tauri::async_runtime::spawn(async move {
        let mut outside_since = None;
        loop {
            tokio::time::sleep(HOVER_POSITION_CHECK_INTERVAL).await;

            let active = state
                .lock()
                .map(|state| state.generation == generation && state.shown)
                .unwrap_or(false);
            if !active {
                return;
            }

            let Some(window) = app.get_webview_window("hover") else {
                mark_hover_hidden();
                return;
            };
            if !window.is_visible().unwrap_or(false) {
                mark_hover_hidden();
                return;
            }

            let Some(cursor_position) = cursor_position() else {
                outside_since = None;
                continue;
            };
            let pointer_is_inside = {
                let position = cursor_position;
                tray_bounds.contains(position)
                    || hover_window_bounds(&window).is_some_and(|bounds| bounds.contains(position))
            };

            if pointer_is_inside {
                outside_since = None;
                continue;
            }

            let started_at = outside_since.get_or_insert_with(Instant::now);
            if started_at.elapsed() < HOVER_HIDE_DELAY {
                continue;
            }

            let should_hide = state
                .lock()
                .map(|mut state| {
                    if state.generation != generation || !state.shown {
                        return false;
                    }
                    state.generation = state.generation.wrapping_add(1);
                    state.shown = false;
                    true
                })
                .unwrap_or(false);
            if should_hide {
                let _ = window.hide();
            }
            return;
        }
    });
}

fn hover_window_bounds(window: &tauri::WebviewWindow) -> Option<PhysicalBounds> {
    let position = window.outer_position().ok()?;
    let size = window.outer_size().ok()?;
    Some(PhysicalBounds {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
    })
}

#[cfg(target_os = "windows")]
fn cursor_position() -> Option<PhysicalPosition<i32>> {
    let mut point = POINT { x: 0, y: 0 };
    if unsafe { GetCursorPos(&mut point) } == 0 {
        return None;
    }
    Some(PhysicalPosition::new(point.x, point.y))
}

#[cfg(not(target_os = "windows"))]
fn cursor_position() -> Option<PhysicalPosition<i32>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn physical_bounds_use_exclusive_bottom_and_right_edges() {
        let bounds = PhysicalBounds {
            x: 10,
            y: 20,
            width: 30,
            height: 40,
        };
        assert!(bounds.contains(PhysicalPosition::new(10, 20)));
        assert!(bounds.contains(PhysicalPosition::new(39, 59)));
        assert!(!bounds.contains(PhysicalPosition::new(40, 59)));
        assert!(!bounds.contains(PhysicalPosition::new(39, 60)));
    }

    #[test]
    fn hover_window_is_placed_above_bottom_right_tray() {
        let position = hover_window_position(
            PhysicalBounds {
                x: 1880,
                y: 1040,
                width: 24,
                height: 24,
            },
            tauri::PhysicalSize::new(380, 300),
            PhysicalBounds {
                x: 0,
                y: 0,
                width: 1920,
                height: 1040,
            },
        );
        assert_eq!(position, PhysicalPosition::new(1540, 732));
    }

    #[test]
    fn hover_window_stays_inside_negative_coordinate_monitor() {
        let work_area = PhysicalBounds {
            x: -1920,
            y: 0,
            width: 1920,
            height: 1040,
        };
        let position = hover_window_position(
            PhysicalBounds {
                x: -40,
                y: 1040,
                width: 24,
                height: 24,
            },
            tauri::PhysicalSize::new(380, 300),
            work_area,
        );
        assert!(bounds_contain_window(
            work_area,
            i64::from(position.x),
            i64::from(position.y),
            380,
            300,
        ));
    }

    #[test]
    fn window_size_is_adjusted_for_target_monitor_scale() {
        assert_eq!(
            window_size_for_scale(tauri::PhysicalSize::new(380, 300), 1.0, 1.5),
            tauri::PhysicalSize::new(570, 450),
        );
    }

    #[test]
    fn tray_timer_actions_cover_the_full_local_timer_lifecycle() {
        let directory = tempfile::tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("tray-timer.sqlite3")).unwrap();

        let started = execute_tray_timer_action(&database, TrayTimerAction::Start).unwrap();
        assert_eq!(started["state"], "running");

        let paused = execute_tray_timer_action(&database, TrayTimerAction::Pause).unwrap();
        assert_eq!(paused["state"], "paused");

        let resumed = execute_tray_timer_action(&database, TrayTimerAction::Resume).unwrap();
        assert_eq!(resumed["state"], "running");

        let stopped = execute_tray_timer_action(&database, TrayTimerAction::Stop).unwrap();
        assert_eq!(stopped["state"], "ended");
        assert!(time_tracking::get_timer_state(&database).unwrap().is_none());
    }
}
