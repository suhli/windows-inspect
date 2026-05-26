#[cfg(target_os = "windows")]
mod win {
    use std::ffi::c_void;

    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR,
        DWMWA_USE_IMMERSIVE_DARK_MODE,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, GetWindowLongPtrW, SendMessageW, SetLayeredWindowAttributes,
        SetWindowLongPtrW, GWL_EXSTYLE, HTCAPTION, LWA_ALPHA, WM_NCLBUTTONDOWN, WS_EX_LAYERED,
    };

    pub fn find_hwnd(window_title: &str) -> Option<HWND> {
        let mut title: Vec<u16> = window_title.encode_utf16().collect();
        title.push(0);
        unsafe { FindWindowW(PCWSTR::null(), PCWSTR(title.as_ptr())) }.ok()
    }

    pub fn apply_native_style(window_title: &str) {
        let Some(hwnd) = find_hwnd(window_title) else {
            return;
        };

        unsafe {
            let dark_mode: i32 = 1;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                (&dark_mode as *const i32).cast::<c_void>(),
                std::mem::size_of::<i32>() as u32,
            );

            // COLORREF is 0x00BBGGRR.
            let caption_color: u32 = 0x00170602; // #020617
            let text_color: u32 = 0x00FAF8F8; // close to #f8fafc
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_CAPTION_COLOR,
                (&caption_color as *const u32).cast::<c_void>(),
                std::mem::size_of::<u32>() as u32,
            );
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_TEXT_COLOR,
                (&text_color as *const u32).cast::<c_void>(),
                std::mem::size_of::<u32>() as u32,
            );
        }
    }

    pub fn set_window_opacity(hwnd: HWND, opacity_percent: u8) {
        let alpha = ((opacity_percent.clamp(10, 100) as u32) * 255 / 100) as u8;
        unsafe {
            let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
            if ex_style & WS_EX_LAYERED.0 == 0 {
                let _ = SetWindowLongPtrW(
                    hwnd,
                    GWL_EXSTYLE,
                    (ex_style | WS_EX_LAYERED.0) as isize,
                );
            }
            let _ = SetLayeredWindowAttributes(hwnd, None, alpha, LWA_ALPHA);
        }
    }

    pub fn clear_window_opacity(hwnd: HWND) {
        unsafe {
            let _ = SetLayeredWindowAttributes(hwnd, None, 255, LWA_ALPHA);
            let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
            if ex_style & WS_EX_LAYERED.0 != 0 {
                let _ = SetWindowLongPtrW(
                    hwnd,
                    GWL_EXSTYLE,
                    (ex_style & !WS_EX_LAYERED.0) as isize,
                );
            }
        }
    }

    pub fn start_window_drag(hwnd: HWND) {
        unsafe {
            let _ = ReleaseCapture();
            let _ = SendMessageW(
                hwnd,
                WM_NCLBUTTONDOWN,
                WPARAM(HTCAPTION as _),
                LPARAM(0),
            );
        }
    }
}

#[cfg(target_os = "windows")]
pub use win::*;

#[cfg(not(target_os = "windows"))]
pub fn find_hwnd(_window_title: &str) -> Option<()> {
    None
}

#[cfg(not(target_os = "windows"))]
pub fn apply_native_style(_window_title: &str) {}

#[cfg(not(target_os = "windows"))]
pub fn set_window_opacity(_hwnd: (), _opacity_percent: u8) {}

#[cfg(not(target_os = "windows"))]
pub fn clear_window_opacity(_hwnd: ()) {}

#[cfg(not(target_os = "windows"))]
pub fn start_window_drag(_hwnd: ()) {}
