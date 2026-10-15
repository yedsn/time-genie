use std::mem::size_of;
use std::ptr;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::tray_presentation::TrayPresentation;
use tauri::{AppHandle, Emitter};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIcon, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyIcon,
    DestroyMenu, DispatchMessageW, GetCursorPos, GetMessageW, LoadImageW, PostMessageW,
    PostQuitMessage, RegisterClassW, SetForegroundWindow, TrackPopupMenu, TranslateMessage,
    CW_USEDEFAULT, HICON, IMAGE_ICON, LR_DEFAULTSIZE, LR_LOADFROMFILE, MF_SEPARATOR, MF_STRING,
    MSG, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_APP, WM_DESTROY, WM_LBUTTONUP, WM_RBUTTONUP, WNDCLASSW,
    WS_EX_TOOLWINDOW, WS_POPUP,
};

const TRAY_ID: u32 = 1;
const TRAY_CALLBACK: u32 = WM_APP + 41;
const TRAY_UPDATE: u32 = WM_APP + 42;
const MENU_OPEN: usize = 1;
const MENU_START: usize = 2;
const MENU_PAUSE: usize = 3;
const MENU_RESUME: usize = 4;
const MENU_STOP: usize = 5;
const MENU_CHECK_UPDATE: usize = 6;
const MENU_QUIT: usize = 7;
static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();
static NATIVE_PRESENTATION: OnceLock<Mutex<NativePresentation>> = OnceLock::new();
static NATIVE_WINDOW: AtomicIsize = AtomicIsize::new(0);
static CURRENT_ICON: AtomicIsize = AtomicIsize::new(0);

#[derive(Clone)]
struct NativePresentation {
    rgba: Option<Vec<u8>>,
    tooltip: String,
}

impl Default for NativePresentation {
    fn default() -> Self {
        Self {
            rgba: None,
            tooltip: "TimeGenie".to_string(),
        }
    }
}

pub fn update_presentation(presentation: &TrayPresentation) {
    let state = NATIVE_PRESENTATION.get_or_init(|| Mutex::new(NativePresentation::default()));
    if let Ok(mut state) = state.lock() {
        state.rgba = presentation.rgba();
        state.tooltip = presentation.tooltip.clone();
    }
    let window = NATIVE_WINDOW.load(Ordering::SeqCst) as HWND;
    if !window.is_null() {
        unsafe {
            PostMessageW(window, TRAY_UPDATE, 0, 0);
        }
    }
}

pub fn start_if_tauri_tray_missing(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(8)).await;
        let tauri_tray_visible = app
            .tray_by_id("timegenie")
            .and_then(|tray| tray.rect().ok().flatten())
            .is_some();
        let force_native =
            cfg!(debug_assertions) && std::env::var_os("TG_DEV_FORCE_NATIVE_TRAY").is_some();
        if tauri_tray_visible && !force_native {
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
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: class_name.as_ptr(),
            ..Default::default()
        });
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
        NATIVE_WINDOW.store(window as isize, Ordering::SeqCst);
        let presentation = current_presentation();
        let icon = icon_for_presentation(&presentation);
        let tray = tray_data(window, icon, &presentation.tooltip);
        if Shell_NotifyIconW(NIM_ADD, &tray) == 0 {
            eprintln!("原生托盘注册失败: {}", std::io::Error::last_os_error());
            destroy_icon(icon);
            NATIVE_WINDOW.store(0, Ordering::SeqCst);
            windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow(window);
            return;
        }
        CURRENT_ICON.store(icon as isize, Ordering::SeqCst);
        eprintln!("原生托盘图标注册成功");
        let mut message = MSG::default();
        while GetMessageW(&mut message, ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        Shell_NotifyIconW(NIM_DELETE, &tray);
        destroy_current_icon();
        NATIVE_WINDOW.store(0, Ordering::SeqCst);
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
        TRAY_UPDATE => {
            apply_pending_presentation(window);
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
    append_item(menu, MENU_CHECK_UPDATE, "检查更新");
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
        MENU_CHECK_UPDATE => {
            super::show(app, "main");
            let _ = app.emit(super::TRAY_CHECK_UPDATE_EVENT, serde_json::json!({}));
        }
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

unsafe fn tray_data(window: HWND, icon: HICON, tooltip: &str) -> NOTIFYICONDATAW {
    let mut data = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: window,
        uID: TRAY_ID,
        uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
        uCallbackMessage: TRAY_CALLBACK,
        hIcon: icon,
        ..Default::default()
    };
    write_tooltip(&mut data, tooltip);
    data
}

fn current_presentation() -> NativePresentation {
    NATIVE_PRESENTATION
        .get_or_init(|| Mutex::new(NativePresentation::default()))
        .lock()
        .map(|state| state.clone())
        .unwrap_or_default()
}

unsafe fn icon_for_presentation(presentation: &NativePresentation) -> HICON {
    if let Some(rgba) = presentation.rgba.as_deref() {
        let icon = create_icon_from_rgba(rgba);
        if !icon.is_null() {
            return icon;
        }
    }
    load_icon()
}

unsafe fn apply_pending_presentation(window: HWND) {
    let presentation = current_presentation();
    let new_icon = icon_for_presentation(&presentation);
    if new_icon.is_null() {
        return;
    }
    let mut tray = tray_data(window, new_icon, &presentation.tooltip);
    tray.uFlags = NIF_ICON | NIF_TIP;
    if Shell_NotifyIconW(NIM_MODIFY, &tray) == 0 {
        destroy_icon(new_icon);
        return;
    }
    let old_icon = CURRENT_ICON.swap(new_icon as isize, Ordering::SeqCst) as HICON;
    destroy_icon(old_icon);
}

unsafe fn destroy_current_icon() {
    let icon = CURRENT_ICON.swap(0, Ordering::SeqCst) as HICON;
    destroy_icon(icon);
}

unsafe fn destroy_icon(icon: HICON) {
    if !icon.is_null() {
        DestroyIcon(icon);
    }
}

unsafe fn create_icon_from_rgba(rgba: &[u8]) -> HICON {
    if rgba.len() != 32 * 32 * 4 {
        return ptr::null_mut();
    }
    let and_mask = vec![0u8; 32 * 32 / 8];
    let mut bgra = vec![0u8; rgba.len()];
    for y in 0..32usize {
        for x in 0..32usize {
            let source = (y * 32 + x) * 4;
            let target = ((31 - y) * 32 + x) * 4;
            bgra[target] = rgba[source + 2];
            bgra[target + 1] = rgba[source + 1];
            bgra[target + 2] = rgba[source];
            bgra[target + 3] = rgba[source + 3];
        }
    }
    CreateIcon(
        ptr::null_mut(),
        32,
        32,
        1,
        32,
        and_mask.as_ptr(),
        bgra.as_ptr(),
    )
}

fn write_tooltip(data: &mut NOTIFYICONDATAW, tooltip: &str) {
    data.szTip.fill(0);
    let encoded = tooltip.encode_utf16().collect::<Vec<_>>();
    let length = encoded.len().min(data.szTip.len().saturating_sub(1));
    data.szTip[..length].copy_from_slice(&encoded[..length]);
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{DrawIconEx, DI_NORMAL};

    fn render_icon_at_size(
        rgba: &[u8],
        size: usize,
        background: (u8, u8, u8),
    ) -> Vec<(u8, u8, u8)> {
        unsafe {
            let icon = create_icon_from_rgba(rgba);
            assert!(!icon.is_null());

            let dc = CreateCompatibleDC(ptr::null_mut());
            assert!(!dc.is_null());
            let mut bitmap_info = BITMAPINFO::default();
            bitmap_info.bmiHeader = BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size as i32,
                biHeight: -(size as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                ..Default::default()
            };
            let mut bits = ptr::null_mut();
            let bitmap = CreateDIBSection(
                dc,
                &bitmap_info,
                DIB_RGB_COLORS,
                &mut bits,
                ptr::null_mut(),
                0,
            );
            assert!(!bitmap.is_null());
            assert!(!bits.is_null());

            let old_bitmap = SelectObject(dc, bitmap);
            let pixels = std::slice::from_raw_parts_mut(bits.cast::<u8>(), size * size * 4);
            for pixel in pixels.chunks_exact_mut(4) {
                pixel.copy_from_slice(&[background.2, background.1, background.0, 255]);
            }
            assert_ne!(
                DrawIconEx(
                    dc,
                    0,
                    0,
                    icon,
                    size as i32,
                    size as i32,
                    0,
                    ptr::null_mut(),
                    DI_NORMAL,
                ),
                0
            );
            let rendered = pixels
                .chunks_exact(4)
                .map(|pixel| (pixel[2], pixel[1], pixel[0]))
                .collect();

            SelectObject(dc, old_bitmap);
            DeleteObject(bitmap);
            DeleteDC(dc);
            DestroyIcon(icon);
            rendered
        }
    }

    fn presentation(state: crate::tray_presentation::TrayTimerState) -> TrayPresentation {
        TrayPresentation {
            style: crate::tray_presentation::TrayIconStyle::StatusOrb,
            state,
            tooltip: String::new(),
            running: state == crate::tray_presentation::TrayTimerState::Running,
            paused: state == crate::tray_presentation::TrayTimerState::Paused,
        }
    }

    #[test]
    fn native_tooltip_is_truncated_and_null_terminated() {
        let mut data = NOTIFYICONDATAW::default();
        write_tooltip(&mut data, &"事项".repeat(200));
        assert_eq!(data.szTip[data.szTip.len() - 1], 0);
        assert!(data.szTip[..data.szTip.len() - 1]
            .iter()
            .any(|value| *value != 0));
    }

    #[test]
    fn presentation_can_be_updated_before_native_window_starts() {
        let presentation = TrayPresentation {
            style: crate::tray_presentation::TrayIconStyle::Minimal,
            state: crate::tray_presentation::TrayTimerState::Paused,
            tooltip: "已暂停 · 测试 · 01:00".to_string(),
            running: false,
            paused: true,
        };
        update_presentation(&presentation);
        let current = current_presentation();
        assert_eq!(current.tooltip, presentation.tooltip);
        assert_eq!(current.rgba, presentation.rgba());

        let next = TrayPresentation {
            style: crate::tray_presentation::TrayIconStyle::StatusOrb,
            state: crate::tray_presentation::TrayTimerState::Running,
            tooltip: "执行中 · 第二次更新 · 02:00".to_string(),
            running: true,
            paused: false,
        };
        update_presentation(&next);
        let current = current_presentation();
        assert_eq!(current.tooltip, next.tooltip);
        assert_eq!(current.rgba, next.rgba());
    }

    #[test]
    fn windows_icon_scaling_preserves_three_states_on_light_and_dark_taskbars() {
        use crate::tray_presentation::TrayTimerState::{Idle, Paused, Running};

        let backgrounds = [(243, 243, 243), (32, 32, 32)];
        for size in [16usize, 20, 24] {
            for background in backgrounds {
                let rendered = [Running, Paused, Idle].map(|state| {
                    render_icon_at_size(&presentation(state).rgba().unwrap(), size, background)
                });

                for (state_index, pixels) in rendered.iter().enumerate() {
                    let visible = pixels
                        .iter()
                        .filter(|pixel| **pixel != background)
                        .collect::<Vec<_>>();
                    let minimum_visible = if state_index == 2 {
                        size * size / 4
                    } else {
                        size * size / 2
                    };
                    assert!(visible.len() >= minimum_visible);
                    let luminance = visible
                        .iter()
                        .map(|pixel| {
                            (u16::from(pixel.0) * 3 + u16::from(pixel.1) * 6 + u16::from(pixel.2))
                                / 10
                        })
                        .collect::<Vec<_>>();
                    assert!(
                        luminance.iter().max().unwrap() - luminance.iter().min().unwrap() >= 35
                    );
                }

                assert_ne!(rendered[0], rendered[1]);
                assert_ne!(rendered[1], rendered[2]);
                assert_ne!(rendered[0], rendered[2]);

                let average = rendered.each_ref().map(|pixels| {
                    let visible = pixels
                        .iter()
                        .filter(|pixel| **pixel != background)
                        .collect::<Vec<_>>();
                    let sum = visible.iter().fold((0u32, 0u32, 0u32), |sum, pixel| {
                        (
                            sum.0 + u32::from(pixel.0),
                            sum.1 + u32::from(pixel.1),
                            sum.2 + u32::from(pixel.2),
                        )
                    });
                    let count = visible.len() as u32;
                    (sum.0 / count, sum.1 / count, sum.2 / count)
                });
                assert!(
                    u32::from(average[0].1) > u32::from(average[0].0) + 25
                        && u32::from(average[0].1) > u32::from(average[0].2) + 10
                );
                assert!(
                    u32::from(average[1].0) > u32::from(average[1].2) + 50
                        && u32::from(average[1].1) > u32::from(average[1].2) + 45
                );
                let idle_pixels = &rendered[2];
                assert!(idle_pixels.iter().any(|pixel| {
                    u32::from(pixel.0) > u32::from(pixel.1) + 70
                        && u32::from(pixel.0) > u32::from(pixel.2) + 60
                }));
            }
        }
    }
}
