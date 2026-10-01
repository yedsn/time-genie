use std::mem::size_of;
use std::ptr;
use std::sync::OnceLock;

use tauri::AppHandle;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyMenu,
    DispatchMessageW, GetCursorPos, GetMessageW, LoadImageW, PostQuitMessage, RegisterClassW,
    SetForegroundWindow, TrackPopupMenu, TranslateMessage, CW_USEDEFAULT, HICON, IMAGE_ICON,
    LR_DEFAULTSIZE, LR_LOADFROMFILE, MF_SEPARATOR, MF_STRING, MSG, TPM_RETURNCMD, TPM_RIGHTBUTTON,
    WM_APP, WM_DESTROY, WM_LBUTTONUP, WM_RBUTTONUP, WNDCLASSW, WS_EX_TOOLWINDOW, WS_POPUP,
};

const TRAY_ID: u32 = 1;
const TRAY_CALLBACK: u32 = WM_APP + 41;
const MENU_OPEN: usize = 1;
const MENU_START: usize = 2;
const MENU_PAUSE: usize = 3;
const MENU_RESUME: usize = 4;
const MENU_STOP: usize = 5;
const MENU_QUIT: usize = 6;
static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();

pub fn start_if_tauri_tray_missing(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(8)).await;
        let tauri_tray_visible = app
            .tray_by_id("timegenie")
            .and_then(|tray| tray.rect().ok().flatten())
            .is_some();
        if tauri_tray_visible {
            return;
        }

        let _ = APP_HANDLE.set(app);
        std::thread::Builder::new()
            .name("native-tray".to_string())
            .spawn(run_native_tray)
            .unwrap_or_else(|error| panic!("无法启动原生托盘线程: {error}"));
    });
}

fn run_native_tray() {
    unsafe {
        let instance = GetModuleHandleW(ptr::null());
        let class_name = wide("timegenie_native_tray");
        let window_class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: class_name.as_ptr(),
            ..Default::default()
        };
        RegisterClassW(&window_class);

        let window = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            class_name.as_ptr(),
            class_name.as_ptr(),
            WS_POPUP,
            CW_USEDEFAULT,
            0,
            CW_USEDEFAULT,
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            instance,
            ptr::null(),
        );
        if window.is_null() {
            eprintln!("原生托盘窗口创建失败: {}", std::io::Error::last_os_error());
            return;
        }

        let icon = load_icon();
        let tray = tray_data(window, icon);
        if Shell_NotifyIconW(NIM_ADD, &tray) == 0 {
            eprintln!("原生托盘注册失败: {}", std::io::Error::last_os_error());
            if !icon.is_null() {
                DestroyIcon(icon);
            }
            windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow(window);
            return;
        }
        eprintln!("原生托盘图标注册成功");

        let mut message = MSG::default();
        while GetMessageW(&mut message, ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }

        Shell_NotifyIconW(NIM_DELETE, &tray);
        if !icon.is_null() {
            DestroyIcon(icon);
        }
    }
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        TRAY_CALLBACK if lparam as u32 == WM_LBUTTONUP => {
            if let Some(app) = APP_HANDLE.get() {
                super::show(app, "main");
            }
            0
        }
        TRAY_CALLBACK if lparam as u32 == WM_RBUTTONUP => {
            show_menu(window);
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(window, message, wparam, lparam),
    }
}

unsafe fn show_menu(window: HWND) {
    let menu = CreatePopupMenu();
    if menu.is_null() {
        return;
    }
    append_item(menu, MENU_OPEN, "打开工作台");
    AppendMenuW(menu, MF_SEPARATOR, 0, ptr::null());
    append_item(menu, MENU_START, "开始计时");
    append_item(menu, MENU_PAUSE, "暂停计时");
    append_item(menu, MENU_RESUME, "继续计时");
    append_item(menu, MENU_STOP, "结束本段");
    AppendMenuW(menu, MF_SEPARATOR, 0, ptr::null());
    append_item(menu, MENU_QUIT, "退出");

    let mut cursor = POINT::default();
    GetCursorPos(&mut cursor);
    SetForegroundWindow(window);
    let selected = TrackPopupMenu(
        menu,
        TPM_RETURNCMD | TPM_RIGHTBUTTON,
        cursor.x,
        cursor.y,
        0,
        window,
        ptr::null(),
    );
    DestroyMenu(menu);

    let Some(app) = APP_HANDLE.get() else {
        return;
    };
    match selected as usize {
        MENU_OPEN => super::show(app, "main"),
        MENU_START => super::run_native_tray_timer_action(app, super::TrayTimerAction::Start),
        MENU_PAUSE => super::run_native_tray_timer_action(app, super::TrayTimerAction::Pause),
        MENU_RESUME => super::run_native_tray_timer_action(app, super::TrayTimerAction::Resume),
        MENU_STOP => super::run_native_tray_timer_action(app, super::TrayTimerAction::Stop),
        MENU_QUIT => app.exit(0),
        _ => {}
    }
}

unsafe fn append_item(menu: *mut core::ffi::c_void, id: usize, label: &str) {
    let label = wide(label);
    AppendMenuW(menu, MF_STRING, id, label.as_ptr());
}

unsafe fn load_icon() -> HICON {
    let path = wide(concat!(env!("CARGO_MANIFEST_DIR"), "/icons/icon.ico"));
    LoadImageW(
        ptr::null_mut(),
        path.as_ptr(),
        IMAGE_ICON,
        32,
        32,
        LR_DEFAULTSIZE | LR_LOADFROMFILE,
    ) as HICON
}

unsafe fn tray_data(window: HWND, icon: HICON) -> NOTIFYICONDATAW {
    let mut data = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: window,
        uID: TRAY_ID,
        uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
        uCallbackMessage: TRAY_CALLBACK,
        hIcon: icon,
        ..Default::default()
    };
    let tooltip = wide("时序");
    let length = tooltip.len().min(data.szTip.len());
    data.szTip[..length].copy_from_slice(&tooltip[..length]);
    data
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
