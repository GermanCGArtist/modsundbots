//! Hide, show and capture game windows (Windows only; every function is a harmless no-op elsewhere).
//!
//! A hidden window is *parked*: moved far outside every monitor, marked as a tool window (so it
//! leaves Alt-Tab) and its taskbar button removed through `ITaskbarList`. It is not minimized, so the
//! game keeps rendering and `PrintWindow(PW_RENDERFULLCONTENT)` still captures it for the Wall.
//! (DWM cloaking would be nicer, but `DWMWA_CLOAK` only works on a process's own windows.)
//! The original position is stored on the window itself (`SetProp`), so any mub process, also one
//! started after a crash, can put the window back.

/// Raw window handle (`HWND` as an integer so it can cross threads).
pub type Hwnd = isize;

/// A captured RGBA image.
#[derive(Debug, Clone)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Visible, non-empty top-level windows of `pid`, largest first (the game window before helpers).
pub fn find_windows(pid: u32) -> Vec<Hwnd> {
    imp::find_windows(pid)
}

/// The main (largest) window of `pid`.
pub fn main_window(pid: u32) -> Option<Hwnd> {
    find_windows(pid).into_iter().next()
}

/// Whether the window is hidden (parked) by mub.
pub fn is_hidden(hwnd: Hwnd) -> bool {
    imp::is_parked(hwnd)
}

/// Hide (`true`) or show a window: park it off-screen without taskbar button / Alt-Tab entry, or put
/// it back where it was. Returns false when nothing could be done.
pub fn set_hidden(hwnd: Hwnd, hidden: bool) -> bool {
    if hidden == imp::is_parked(hwnd) {
        return true;
    }
    let ok = imp::set_parked(hwnd, hidden);
    if ok {
        imp::set_taskbar_button(hwnd, !hidden);
    }
    ok
}

/// Hide every window of `pid`. Returns how many were hidden.
pub fn hide_pid(pid: u32) -> usize {
    find_windows(pid)
        .into_iter()
        .filter(|&h| set_hidden(h, true))
        .count()
}

/// Show every window of `pid`; with `focus` the main one is brought to the front. Returns how many.
pub fn show_pid(pid: u32, focus: bool) -> usize {
    let wins = find_windows(pid);
    let n = wins.iter().filter(|&&h| set_hidden(h, false)).count();
    if focus && let Some(&main) = wins.first() {
        imp::bring_to_front(main);
    }
    n
}

/// Whether the main window of `pid` is currently hidden.
pub fn pid_hidden(pid: u32) -> Option<bool> {
    main_window(pid).map(is_hidden)
}

/// Capture the client area of `hwnd`, scaled down so it is at most `max_width` wide.
pub fn capture(hwnd: Hwnd, max_width: u32) -> Option<Frame> {
    let full = imp::capture_client(hwnd)?;
    Some(downscale(full, max_width))
}

/// Show every window that mub hid and that belongs to a `java`/`javaw` process: the safety net when
/// mub-gui crashed while bots were hidden. Returns how many windows were shown.
pub fn unhide_all_java() -> usize {
    imp::all_top_windows()
        .into_iter()
        .filter(|&(h, pid)| {
            is_hidden(h)
                && process_exe_name(pid)
                    .map(|n| {
                        let n = n.to_ascii_lowercase();
                        n == "java.exe" || n == "javaw.exe"
                    })
                    .unwrap_or(false)
        })
        .filter(|&(h, _)| set_hidden(h, false))
        .count()
}

/// Whether the process is still running.
pub fn process_alive(pid: u32) -> bool {
    imp::process_alive(pid)
}

/// Process creation time (Windows FILETIME ticks), used to make sure a recorded PID was not reused.
pub fn process_start_time(pid: u32) -> Option<u64> {
    imp::process_start_time(pid)
}

/// File name of the process executable, e.g. `javaw.exe`.
pub fn process_exe_name(pid: u32) -> Option<String> {
    imp::process_exe_path(pid).and_then(|p| {
        std::path::Path::new(&p)
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
    })
}

/// Kill a process we have no `Child` handle for (an instance adopted after a mub restart).
pub fn terminate_process(pid: u32) -> bool {
    imp::terminate(pid)
}

/// Nearest-neighbour downscale; thumbnails do not need better filtering.
fn downscale(f: Frame, max_width: u32) -> Frame {
    if max_width == 0 || f.width <= max_width {
        return f;
    }
    let w = max_width;
    let h = ((f.height as u64 * w as u64) / f.width as u64).max(1) as u32;
    let mut out = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        let sy = (y as u64 * f.height as u64 / h as u64) as usize;
        for x in 0..w {
            let sx = (x as u64 * f.width as u64 / w as u64) as usize;
            let si = (sy * f.width as usize + sx) * 4;
            let di = ((y * w + x) * 4) as usize;
            out[di..di + 4].copy_from_slice(&f.rgba[si..si + 4]);
        }
    }
    Frame {
        width: w,
        height: h,
        rgba: out,
    }
}

#[cfg(windows)]
mod imp {
    use super::{Frame, Hwnd};
    use std::ffi::c_void;
    use std::mem::{size_of, zeroed};
    use windows_sys::Win32::Foundation::{BOOL, CloseHandle, FILETIME, HWND, LPARAM, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleBitmap, CreateCompatibleDC,
        DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC, SelectObject,
    };
    use windows_sys::Win32::Storage::Xps::PrintWindow;
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, GetProcessTimes, OpenProcess, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE, QueryFullProcessImageNameW,
        TerminateProcess,
    };
    use windows_sys::Win32::UI::HiDpi::{
        DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetThreadDpiAwarenessContext,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GWL_EXSTYLE, GetClientRect, GetPropW, GetSystemMetrics, GetWindowLongPtrW,
        GetWindowRect, GetWindowThreadProcessId, IsIconic, IsWindowVisible, RemovePropW,
        SM_CXVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SW_RESTORE, SWP_ASYNCWINDOWPOS,
        SW_HIDE, SW_SHOWNA, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SetForegroundWindow, SetPropW,
        SetWindowLongPtrW, SetWindowPos, ShowWindow, ShowWindowAsync, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
    };

    const PW_CLIENTONLY: u32 = 1;
    const PW_RENDERFULLCONTENT: u32 = 2;
    const STILL_ACTIVE: u32 = 259;

    fn hwnd(h: Hwnd) -> HWND {
        h as HWND
    }

    pub fn all_top_windows() -> Vec<(Hwnd, u32)> {
        unsafe extern "system" fn cb(h: HWND, lp: LPARAM) -> BOOL {
            let out = unsafe { &mut *(lp as *mut Vec<(Hwnd, u32)>) };
            let mut pid = 0u32;
            unsafe { GetWindowThreadProcessId(h, &mut pid) };
            out.push((h as Hwnd, pid));
            1
        }
        let mut out: Vec<(Hwnd, u32)> = Vec::new();
        unsafe { EnumWindows(Some(cb), &mut out as *mut _ as LPARAM) };
        out
    }

    pub fn find_windows(pid: u32) -> Vec<Hwnd> {
        let mut m: Vec<(Hwnd, i64)> = all_top_windows()
            .into_iter()
            .filter(|&(_, p)| p == pid)
            .filter_map(|(h, _)| unsafe {
                // A parked window is still visible (just off-screen), so it is found here too.
                if IsWindowVisible(hwnd(h)) == 0 {
                    return None;
                }
                let mut r: RECT = zeroed();
                if GetWindowRect(hwnd(h), &mut r) == 0 {
                    return None;
                }
                let a = (r.right - r.left) as i64 * (r.bottom - r.top) as i64;
                (a > 0).then_some((h, a))
            })
            .collect();
        m.sort_by_key(|&(_, a)| std::cmp::Reverse(a));
        m.into_iter().map(|(h, _)| h).collect()
    }

    /// Window properties that mark a parked window and remember where it was.
    fn prop_name(name: &str) -> Vec<u16> {
        name.encode_utf16().chain(std::iter::once(0)).collect()
    }
    const PROP_PARKED: &str = "ModsUndBots.Parked";
    const PROP_POS: &str = "ModsUndBots.ParkedPos";
    const PROP_EXSTYLE: &str = "ModsUndBots.ParkedExStyle";

    /// Run with per-monitor DPI awareness so positions mean the same in every mub process.
    fn dpi_aware<T>(f: impl FnOnce() -> T) -> T {
        unsafe {
            let old = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let r = f();
            if !old.is_null() {
                SetThreadDpiAwarenessContext(old);
            }
            r
        }
    }

    pub fn is_parked(h: Hwnd) -> bool {
        let name = prop_name(PROP_PARKED);
        unsafe { !GetPropW(hwnd(h), name.as_ptr()).is_null() }
    }

    pub fn set_parked(h: Hwnd, park: bool) -> bool {
        let (p_parked, p_pos, p_ex) = (prop_name(PROP_PARKED), prop_name(PROP_POS), prop_name(PROP_EXSTYLE));
        dpi_aware(|| unsafe {
            let w = hwnd(h);
            if park {
                let mut r: RECT = zeroed();
                if GetWindowRect(w, &mut r) == 0 {
                    return false;
                }
                let ex = GetWindowLongPtrW(w, GWL_EXSTYLE);
                // Remember position and style on the window itself (+1 so the value is never 0).
                let pos = (((r.left as u32 as u64) << 32) | r.top as u32 as u64) as isize;
                SetPropW(w, p_pos.as_ptr(), pos as *mut c_void);
                SetPropW(w, p_ex.as_ptr(), (ex + 1) as *mut c_void);
                SetPropW(w, p_parked.as_ptr(), std::ptr::without_provenance_mut(1));
                // Out of Alt-Tab: tool window, not app window.
                let new_ex = (ex | WS_EX_TOOLWINDOW as isize) & !(WS_EX_APPWINDOW as isize);
                SetWindowLongPtrW(w, GWL_EXSTYLE, new_ex);
                // Far right of the whole virtual desktop, so no monitor ever shows it.
                let x = GetSystemMetrics(SM_XVIRTUALSCREEN) + GetSystemMetrics(SM_CXVIRTUALSCREEN) + 4000;
                let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
                SetWindowPos(
                    w,
                    std::ptr::null_mut(),
                    x,
                    y,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS,
                ) != 0
            } else {
                let pos = GetPropW(w, p_pos.as_ptr()) as isize as u64;
                let ex = GetPropW(w, p_ex.as_ptr()) as isize;
                let (x, y) = ((pos >> 32) as u32 as i32, pos as u32 as i32);
                if ex != 0 {
                    SetWindowLongPtrW(w, GWL_EXSTYLE, ex - 1);
                } else {
                    let cur = GetWindowLongPtrW(w, GWL_EXSTYLE);
                    SetWindowLongPtrW(w, GWL_EXSTYLE, cur & !(WS_EX_TOOLWINDOW as isize));
                }
                RemovePropW(w, p_pos.as_ptr());
                RemovePropW(w, p_ex.as_ptr());
                RemovePropW(w, p_parked.as_ptr());
                let moved = SetWindowPos(
                    w,
                    std::ptr::null_mut(),
                    x,
                    y,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS,
                ) != 0;
                // The shell only rebuilds a taskbar button from the (restored) style when the window
                // is shown again; ITaskbarList::AddTab alone is not reliable for another process's
                // window. Queue a quick hide/show on the game's own thread.
                ShowWindowAsync(w, SW_HIDE);
                ShowWindowAsync(w, SW_SHOWNA);
                moved
            }
        })
    }

    pub fn bring_to_front(h: Hwnd) {
        unsafe {
            if IsIconic(hwnd(h)) != 0 {
                ShowWindow(hwnd(h), SW_RESTORE);
            }
            SetForegroundWindow(hwnd(h));
        }
    }

    /// Add or remove the taskbar button through ITaskbarList. Runs on its own short-lived STA thread
    /// so it works from any caller (GUI thread, tokio worker) without COM apartment surprises.
    pub fn set_taskbar_button(h: Hwnd, visible: bool) {
        let _ = std::thread::spawn(move || unsafe { taskbar_button(h, visible) }).join();
    }

    #[repr(C)]
    struct ITaskbarListVtbl {
        query_interface: usize,
        add_ref: usize,
        release: unsafe extern "system" fn(*mut c_void) -> u32,
        hr_init: unsafe extern "system" fn(*mut c_void) -> i32,
        add_tab: unsafe extern "system" fn(*mut c_void, HWND) -> i32,
        delete_tab: unsafe extern "system" fn(*mut c_void, HWND) -> i32,
    }

    unsafe fn taskbar_button(h: Hwnd, visible: bool) {
        use windows_sys::Win32::System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
            CoUninitialize,
        };
        use windows_sys::core::GUID;
        const CLSID_TASKBAR_LIST: GUID = GUID::from_u128(0x56fdf344_fd6d_11d0_958a_006097c9a090);
        const IID_ITASKBAR_LIST: GUID = GUID::from_u128(0x56fdf342_fd6d_11d0_958a_006097c9a090);
        unsafe {
            let init = CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32);
            let mut obj: *mut c_void = std::ptr::null_mut();
            let hr = CoCreateInstance(
                &CLSID_TASKBAR_LIST,
                std::ptr::null_mut(),
                CLSCTX_INPROC_SERVER,
                &IID_ITASKBAR_LIST,
                &mut obj,
            );
            if hr == 0 && !obj.is_null() {
                let vtbl = &**(obj as *mut *const ITaskbarListVtbl);
                if (vtbl.hr_init)(obj) == 0 {
                    if visible {
                        (vtbl.add_tab)(obj, hwnd(h));
                    } else {
                        (vtbl.delete_tab)(obj, hwnd(h));
                    }
                }
                (vtbl.release)(obj);
            }
            if init >= 0 {
                CoUninitialize();
            }
        }
    }

    pub fn capture_client(h: Hwnd) -> Option<Frame> {
        unsafe {
            let mut r: RECT = zeroed();
            if GetClientRect(hwnd(h), &mut r) == 0 {
                return None;
            }
            let (w, hgt) = (r.right - r.left, r.bottom - r.top);
            if w <= 0 || hgt <= 0 {
                return None;
            }
            let wdc = GetDC(hwnd(h));
            if wdc.is_null() {
                return None;
            }
            let mdc = CreateCompatibleDC(wdc);
            let bmp = CreateCompatibleBitmap(wdc, w, hgt);
            if mdc.is_null() || bmp.is_null() {
                if !bmp.is_null() {
                    DeleteObject(bmp);
                }
                if !mdc.is_null() {
                    DeleteDC(mdc);
                }
                ReleaseDC(hwnd(h), wdc);
                return None;
            }
            let old = SelectObject(mdc, bmp);
            let mut buf = vec![0u8; (w as usize) * (hgt as usize) * 4];
            let mut bmi: BITMAPINFO = zeroed();
            bmi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
            bmi.bmiHeader.biWidth = w;
            bmi.bmiHeader.biHeight = -hgt; // top-down
            bmi.bmiHeader.biPlanes = 1;
            bmi.bmiHeader.biBitCount = 32;
            bmi.bmiHeader.biCompression = BI_RGB;
            let mut ok = PrintWindow(hwnd(h), mdc, PW_CLIENTONLY | PW_RENDERFULLCONTENT) != 0
                && GetDIBits(mdc, bmp, 0, hgt as u32, buf.as_mut_ptr().cast(), &mut bmi, DIB_RGB_COLORS) != 0
                && buf.iter().any(|&b| b != 0);
            if !ok {
                ok = PrintWindow(hwnd(h), mdc, PW_CLIENTONLY) != 0
                    && GetDIBits(mdc, bmp, 0, hgt as u32, buf.as_mut_ptr().cast(), &mut bmi, DIB_RGB_COLORS) != 0;
            }
            SelectObject(mdc, old);
            DeleteObject(bmp);
            DeleteDC(mdc);
            ReleaseDC(hwnd(h), wdc);
            if !ok {
                return None;
            }
            for px in buf.as_chunks_mut::<4>().0 {
                px.swap(0, 2); // BGRA -> RGBA
                px[3] = 255;
            }
            Some(Frame {
                width: w as u32,
                height: hgt as u32,
                rgba: buf,
            })
        }
    }

    fn with_process<T>(pid: u32, access: u32, f: impl FnOnce(*mut c_void) -> Option<T>) -> Option<T> {
        unsafe {
            let h = OpenProcess(access, 0, pid);
            if h.is_null() {
                return None;
            }
            let r = f(h);
            CloseHandle(h);
            r
        }
    }

    pub fn process_alive(pid: u32) -> bool {
        with_process(pid, PROCESS_QUERY_LIMITED_INFORMATION, |h| unsafe {
            let mut code = 0u32;
            (GetExitCodeProcess(h, &mut code) != 0).then_some(code == STILL_ACTIVE)
        })
        .unwrap_or(false)
    }

    pub fn process_start_time(pid: u32) -> Option<u64> {
        with_process(pid, PROCESS_QUERY_LIMITED_INFORMATION, |h| unsafe {
            let (mut c, mut e, mut k, mut u): (FILETIME, FILETIME, FILETIME, FILETIME) =
                (zeroed(), zeroed(), zeroed(), zeroed());
            (GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u) != 0)
                .then_some(((c.dwHighDateTime as u64) << 32) | c.dwLowDateTime as u64)
        })
    }

    pub fn process_exe_path(pid: u32) -> Option<String> {
        with_process(pid, PROCESS_QUERY_LIMITED_INFORMATION, |h| unsafe {
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            (QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len) != 0)
                .then(|| String::from_utf16_lossy(&buf[..len as usize]))
        })
    }

    pub fn terminate(pid: u32) -> bool {
        with_process(pid, PROCESS_TERMINATE, |h| unsafe {
            Some(TerminateProcess(h, 1) != 0)
        })
        .unwrap_or(false)
    }
}

#[cfg(not(windows))]
mod imp {
    use super::{Frame, Hwnd};
    pub fn all_top_windows() -> Vec<(Hwnd, u32)> {
        Vec::new()
    }
    pub fn find_windows(_pid: u32) -> Vec<Hwnd> {
        Vec::new()
    }
    pub fn is_parked(_h: Hwnd) -> bool {
        false
    }
    pub fn set_parked(_h: Hwnd, _on: bool) -> bool {
        false
    }
    pub fn bring_to_front(_h: Hwnd) {}
    pub fn set_taskbar_button(_h: Hwnd, _visible: bool) {}
    pub fn capture_client(_h: Hwnd) -> Option<Frame> {
        None
    }
    pub fn process_alive(pid: u32) -> bool {
        std::path::Path::new(&format!("/proc/{pid}")).exists()
    }
    pub fn process_start_time(_pid: u32) -> Option<u64> {
        None
    }
    pub fn process_exe_path(_pid: u32) -> Option<String> {
        None
    }
    pub fn terminate(_pid: u32) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downscale_keeps_aspect_and_pixels() {
        let mut rgba = vec![0u8; 4 * 4 * 2];
        rgba[0] = 200; // top-left red
        let f = downscale(
            Frame {
                width: 4,
                height: 2,
                rgba,
            },
            2,
        );
        assert_eq!((f.width, f.height), (2, 1));
        assert_eq!(f.rgba[0], 200);
    }

    #[test]
    fn unknown_pid_is_harmless() {
        let pid = u32::MAX - 1;
        assert!(find_windows(pid).is_empty());
        assert!(!process_alive(pid));
        assert!(process_start_time(pid).is_none());
        assert_eq!(hide_pid(pid), 0);
    }

    #[cfg(windows)]
    #[test]
    fn own_process_has_start_time_and_name() {
        let pid = std::process::id();
        assert!(process_alive(pid));
        assert!(process_start_time(pid).is_some());
        assert!(process_exe_name(pid).is_some());
    }
}
