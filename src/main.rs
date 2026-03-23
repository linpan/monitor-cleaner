#![windows_subsystem = "windows"]

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use windows::{
    core::*, Win32::Foundation::*, Win32::Graphics::Gdi::*,
    Win32::System::LibraryLoader::GetModuleHandleA, Win32::UI::Input::KeyboardAndMouse::*,
    Win32::UI::Shell::*, Win32::UI::WindowsAndMessaging::*,
};

// 托盘图标和菜单常量
const WM_TRAYICON: u32 = WM_USER + 1;
const ID_TRAY_ICON: u32 = 1001;
const ID_MENU_EXIT: u32 = 1002;

// 全局状态：存储每个显示器上被最小化的窗口句柄（存储为 usize 解决 Send 问题）
static MINIMIZED_WINDOWS: LazyLock<Mutex<HashMap<u32, Vec<usize>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

// 获取所有显示器的边界矩形
fn get_monitors() -> Vec<RECT> {
    let mut monitors = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(monitor_enum_proc),
            LPARAM(&mut monitors as *mut _ as isize),
        );
    }
    monitors
}

extern "system" fn monitor_enum_proc(
    _hmonitor: HMONITOR,
    _: HDC,
    rect: *mut RECT,
    lparam: LPARAM,
) -> BOOL {
    let monitors = unsafe { &mut *(lparam.0 as *mut Vec<RECT>) };
    let rect = unsafe { *rect };
    monitors.push(rect);
    TRUE
}

// 根据屏幕坐标获取显示器索引（1-based）
fn get_monitor_at_point(x: i32, y: i32) -> Option<u32> {
    let monitors = get_monitors();
    for (i, rect) in monitors.iter().enumerate() {
        if x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom {
            return Some((i + 1) as u32);
        }
    }
    None
}

// 判断窗口是否应该被处理（排除系统窗口）
fn should_handle_window(hwnd: HWND) -> bool {
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() {
            return false;
        }
        let mut class_name = [0u16; 256];
        let len = GetClassNameW(hwnd, &mut class_name);
        let class = String::from_utf16_lossy(&class_name[..len as usize]);
        // 排除桌面、任务栏、开始菜单等系统窗口
        if class == "Progman"
            || class == "WorkerW"
            || class == "Shell_TrayWnd"
            || class == "Shell_SecondaryTrayWnd"
            || class == "Start"
        {
            return false;
        }
        true
    }
}

// 获取窗口中心点坐标
fn get_window_center(hwnd: HWND) -> Option<(i32, i32)> {
    unsafe {
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_ok() {
            let width = rect.right - rect.left;
            let height = rect.bottom - rect.top;
            if width > 0 && height > 0 {
                let center_x = rect.left + width / 2;
                let center_y = rect.top + height / 2;
                return Some((center_x, center_y));
            }
        }
        None
    }
}

// 清空指定显示器上的所有窗口
fn clean_monitor(monitor_index: u32) {
    let mut minimized = Vec::new();
    unsafe {
        let _ = EnumWindows(
            Some(enum_window_clean),
            LPARAM(&mut (monitor_index, &mut minimized) as *mut _ as isize),
        );
    }
    // 将 HWND 转换为 usize 存储
    let minimized_usize: Vec<usize> = minimized
        .into_iter()
        .map(|hwnd: HWND| hwnd.0 as usize)
        .collect();
    let mut map = MINIMIZED_WINDOWS.lock().unwrap();
    map.insert(monitor_index, minimized_usize);
}

extern "system" fn enum_window_clean(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let (monitor_index, minimized) = unsafe { &mut *(lparam.0 as *mut (u32, &mut Vec<HWND>)) };
    if should_handle_window(hwnd) {
        if let Some((cx, cy)) = get_window_center(hwnd) {
            if let Some(mon) = get_monitor_at_point(cx, cy) {
                if mon == *monitor_index {
                    unsafe {
                        let _ = ShowWindow(hwnd, SW_MINIMIZE);
                        minimized.push(hwnd);
                    }
                }
            }
        }
    }
    TRUE
}

// 恢复指定显示器上的所有窗口（倒序恢复以保持原始 Z-Order）
fn restore_monitor(monitor_index: u32) {
    let mut map = MINIMIZED_WINDOWS.lock().unwrap();
    if let Some(list) = map.remove(&monitor_index) {
        for &hwnd_usize in list.iter().rev() {
            let hwnd = HWND(hwnd_usize as *mut _);
            unsafe {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            }
        }
    }
}

// 切换当前鼠标所在显示器的状态（清空或恢复）
fn toggle_current_monitor() {
    unsafe {
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        if let Some(monitor) = get_monitor_at_point(pt.x, pt.y) {
            let map = MINIMIZED_WINDOWS.lock().unwrap();
            let exists = map.contains_key(&monitor);
            drop(map);
            if exists {
                restore_monitor(monitor);
            } else {
                clean_monitor(monitor);
            }
        }
    }
}

// 注册全局热键
fn register_hotkey(hwnd: HWND, id: i32, modifiers: HOT_KEY_MODIFIERS, vk: u32) -> bool {
    unsafe { RegisterHotKey(hwnd, id, modifiers, vk).is_ok() }
}

// 添加系统托盘图标
fn add_tray_icon(hwnd: HWND) {
    unsafe {
        // 从嵌入资源加载自定义图标（资源 ID = 1）
        let handle = LoadImageA(
            GetModuleHandleA(None).unwrap_or_default(),
            PCSTR(1u16 as *const u8),
            IMAGE_ICON,
            16,
            16,
            LR_DEFAULTCOLOR,
        )
        .unwrap_or_default();

        let hicon = if !handle.is_invalid() {
            HICON(handle.0)
        } else {
            LoadIconW(None, IDI_APPLICATION).unwrap_or_default()
        };

        let mut nid = NOTIFYICONDATAA {
            cbSize: std::mem::size_of::<NOTIFYICONDATAA>() as u32,
            hWnd: hwnd,
            uID: ID_TRAY_ICON,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: WM_TRAYICON,
            hIcon: hicon,
            ..Default::default()
        };
        // 设置提示文字
        let tip = b"Monitor Cleaner\nCtrl+Q: Toggle\0";
        let tip_i8: &[i8] = std::mem::transmute(tip.as_slice());
        nid.szTip[..tip_i8.len()].copy_from_slice(tip_i8);
        let _ = Shell_NotifyIconA(NIM_ADD, &nid);
    }
}

// 移除系统托盘图标
fn remove_tray_icon() {
    unsafe {
        let nid = NOTIFYICONDATAA {
            cbSize: std::mem::size_of::<NOTIFYICONDATAA>() as u32,
            uID: ID_TRAY_ICON,
            ..Default::default()
        };
        let _ = Shell_NotifyIconA(NIM_DELETE, &nid);
    }
}

// 显示托盘右键菜单
fn show_tray_menu(hwnd: HWND) {
    unsafe {
        let menu = CreatePopupMenu().unwrap();
        // 添加菜单项 - 退出
        let exit_text = c"Exit";
        let _ = AppendMenuA(menu, MF_STRING, ID_MENU_EXIT as usize, PCSTR(exit_text.as_ptr() as *const u8));

        // 获取鼠标位置
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);

        // 确保窗口在前台，否则菜单可能无法正确关闭
        let _ = SetForegroundWindow(hwnd);

        // 显示菜单
        let _ = TrackPopupMenu(
            menu,
            TPM_RIGHTBUTTON,
            pt.x,
            pt.y,
            0, // nReserved
            hwnd,
            None,
        );

        // 销毁菜单
        let _ = DestroyMenu(menu);
    }
}

// 消息循环（隐藏窗口，处理热键）
fn message_loop() -> Result<()> {
    let class_name = "MonitorCleanerClass";
    let instance = unsafe { GetModuleHandleA(None)? };

    let wc = WNDCLASSA {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(wndproc),
        hInstance: instance.into(),
        lpszClassName: PCSTR(class_name.as_ptr()),
        ..Default::default()
    };
    unsafe {
        RegisterClassA(&wc);
        let hwnd = CreateWindowExA(
            WINDOW_EX_STYLE::default(),
            PCSTR(class_name.as_ptr()),
            PCSTR("MonitorCleaner\0".as_ptr()),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            400,
            200,
            None,
            None,
            instance,
            None,
        )?;
        if hwnd.0.is_null() {
            return Err(Error::from_win32());
        }
        // 注册热键 Ctrl+Q
        let vk = 0x51; // Q
        if !register_hotkey(hwnd, 1, MOD_CONTROL, vk) {
            return Err(Error::from_win32());
        }
        // 添加系统托盘图标
        add_tray_icon(hwnd);

        // 隐藏窗口（仅用于消息处理）
        let _ = ShowWindow(hwnd, SW_HIDE);

        let mut msg = MSG::default();
        while GetMessageA(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageA(&msg);
        }

        // 清理托盘图标
        remove_tray_icon();
        Ok(())
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_HOTKEY => {
            if wparam.0 == 1 {
                toggle_current_monitor();
            }
            LRESULT(0)
        }
        WM_TRAYICON => {
            match lparam.0 as u32 {
                WM_RBUTTONUP => {
                    // 右键点击显示菜单
                    show_tray_menu(hwnd);
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            let menu_id = (wparam.0 & 0xFFFF) as u32;
            match menu_id {
                ID_MENU_EXIT => {
                    unsafe {
                        PostQuitMessage(0);
                    }
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe {
                PostQuitMessage(0);
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcA(hwnd, msg, wparam, lparam) },
    }
}

fn main() -> Result<()> {
    message_loop()
}
