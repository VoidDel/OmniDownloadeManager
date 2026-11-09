//! Windows: the window's icons at the window's own DPI.
//!
//! Tauri gives a window only a small icon (`WM_SETICON`, `ICON_SMALL`), sized when the app
//! starts, before the process knows the display's scale: 32 px whatever the DPI. The taskbar
//! shows a window's large icon, or else enlarges the small one, so at 150 % or 175 % the taskbar
//! button was a 32 px image stretched (blurry). Here both icons are loaded from the executable's
//! icon resource at the sizes Windows asks for at the window's DPI, and again when the window
//! moves to a display with another scale.

use std::sync::Mutex;

use windows_sys::Win32::{
    Foundation::HWND,
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi},
        WindowsAndMessaging::{
            DestroyIcon, HICON, ICON_BIG, ICON_SMALL, IMAGE_ICON, LR_DEFAULTCOLOR, LoadImageW,
            SM_CXICON, SM_CXSMICON, SM_CYICON, SM_CYSMICON, SendMessageW, WM_SETICON,
        },
    },
};

/// The application icon the bundler embeds (`tauri_utils::platform::WINDOWS_APP_ICON_RESOURCE_ID`).
const APP_ICON: u16 = 32512;

/// Icons this module set, by window: destroyed when replaced.
static SET: Mutex<Vec<(isize, [isize; 2])>> = Mutex::new(Vec::new());

/// Sets the large and small icons of window `hwnd` (from `hwnd()` of a Tauri window) for its
/// current DPI.
pub fn apply(hwnd: isize) {
    let hwnd = hwnd as HWND;
    // SAFETY: plain Win32 calls on a live window handle; the icons loaded here are owned by
    // this module until replaced.
    unsafe {
        let dpi = GetDpiForWindow(hwnd);
        if dpi == 0 {
            return;
        }
        let module = GetModuleHandleW(std::ptr::null());
        let mut loaded = [0isize; 2];
        for (i, (kind, cx, cy)) in [
            (ICON_BIG, SM_CXICON, SM_CYICON),
            (ICON_SMALL, SM_CXSMICON, SM_CYSMICON),
        ]
        .into_iter()
        .enumerate()
        {
            let (w, h) = (
                GetSystemMetricsForDpi(cx, dpi),
                GetSystemMetricsForDpi(cy, dpi),
            );
            // MAKEINTRESOURCEW: the id in the pointer's low word.
            let icon = LoadImageW(
                module,
                APP_ICON as usize as *const u16,
                IMAGE_ICON,
                w,
                h,
                LR_DEFAULTCOLOR,
            );
            if icon.is_null() {
                tracing::warn!("loading the {w}x{h} window icon failed");
                continue;
            }
            SendMessageW(hwnd, WM_SETICON, kind as usize, icon as isize);
            loaded[i] = icon as isize;
        }
        let mut set = SET.lock().unwrap_or_else(|e| e.into_inner());
        let key = hwnd as isize;
        if let Some(pos) = set.iter().position(|(h, _)| *h == key) {
            let (_, old) = set.remove(pos);
            for (i, icon) in old.into_iter().enumerate() {
                if icon != 0 && loaded[i] != 0 {
                    DestroyIcon(icon as HICON);
                }
            }
        }
        set.push((key, loaded));
    }
}

/// The window is gone: frees the icons set on it.
pub fn forget(hwnd: isize) {
    let mut set = SET.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(pos) = set.iter().position(|(h, _)| *h == hwnd) {
        let (_, icons) = set.remove(pos);
        for icon in icons.into_iter().filter(|&i| i != 0) {
            // SAFETY: an icon this module loaded and no window uses any more.
            unsafe { DestroyIcon(icon as HICON) };
        }
    }
}
