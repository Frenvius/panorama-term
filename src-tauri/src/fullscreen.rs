#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    fn intersects(&self, other: &Rect) -> bool {
        self.x < other.x + other.w
            && other.x < self.x + self.w
            && self.y < other.y + other.h
            && other.y < self.y + self.h
    }

    fn covers(&self, other: &Rect) -> bool {
        self.x <= other.x
            && self.y <= other.y
            && self.x + self.w >= other.x + other.w
            && self.y + self.h >= other.y + other.h
    }
}

#[derive(Clone, Copy, Debug)]
struct Candidate {
    rect: Rect,
    own: bool,
    desktop: bool,
    pinned: bool,
}

// Windows arrive front to back. Always-on-top windows that do not cover the display (mini players,
// overlays) are looked through; the first ordinary window on the display decides.
fn foreign_window_covers(stack: &[Candidate], display: &Rect) -> bool {
    for window in stack {
        if !window.rect.intersects(display) {
            continue;
        }
        if window.rect.covers(display) {
            return !(window.own || window.desktop);
        }
        if !window.pinned {
            return false;
        }
    }
    false
}

pub fn other_app_fullscreen_on(monitor: &tauri::Monitor) -> bool {
    let Some((stack, display)) = platform::scan(monitor) else {
        return false;
    };
    foreign_window_covers(&stack, &display)
}

fn physical_rect(monitor: &tauri::Monitor) -> Rect {
    let pos = monitor.position();
    let size = monitor.size();
    Rect {
        x: pos.x as f64,
        y: pos.y as f64,
        w: size.width as f64,
        h: size.height as f64,
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::{physical_rect, Candidate, Rect};
    use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, RECT};
    use windows_sys::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId, IsIconic,
        IsWindowVisible, GWL_EXSTYLE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    };

    const DESKTOP_CLASSES: [&str; 2] = ["Progman", "WorkerW"];

    pub fn scan(monitor: &tauri::Monitor) -> Option<(Vec<Candidate>, Rect)> {
        let mut stack: Vec<Candidate> = Vec::new();
        unsafe {
            EnumWindows(Some(visit), &mut stack as *mut Vec<Candidate> as LPARAM);
        }
        Some((stack, physical_rect(monitor)))
    }

    unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let stack = &mut *(lparam as *mut Vec<Candidate>);
        if let Some(candidate) = candidate(hwnd) {
            stack.push(candidate);
        }
        1
    }

    unsafe fn candidate(hwnd: HWND) -> Option<Candidate> {
        if IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 || is_cloaked(hwnd) {
            return None;
        }
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        if ex & WS_EX_TOOLWINDOW != 0 {
            return None;
        }
        let mut r: RECT = std::mem::zeroed();
        if GetWindowRect(hwnd, &mut r) == 0 || r.right <= r.left || r.bottom <= r.top {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        Some(Candidate {
            rect: Rect {
                x: r.left as f64,
                y: r.top as f64,
                w: (r.right - r.left) as f64,
                h: (r.bottom - r.top) as f64,
            },
            own: pid == std::process::id(),
            desktop: DESKTOP_CLASSES.contains(&class_name(hwnd).as_str()),
            pinned: ex & WS_EX_TOPMOST != 0,
        })
    }

    unsafe fn is_cloaked(hwnd: HWND) -> bool {
        let mut cloaked = 0u32;
        let hr = DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED as _,
            &mut cloaked as *mut u32 as *mut _,
            std::mem::size_of::<u32>() as u32,
        );
        hr == 0 && cloaked != 0
    }

    unsafe fn class_name(hwnd: HWND) -> String {
        let mut buf = [0u16; 64];
        let len = GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
        String::from_utf16_lossy(&buf[..len.max(0) as usize])
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::{Candidate, Rect};
    use core_foundation::base::{CFType, TCFType};
    use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
    use core_foundation::number::CFNumber;
    use core_foundation::string::CFString;
    use core_graphics::geometry::CGRect;
    use core_graphics::window::{
        copy_window_info, kCGNullWindowID, kCGWindowBounds, kCGWindowLayer, kCGWindowListExcludeDesktopElements,
        kCGWindowListOptionOnScreenOnly, kCGWindowOwnerPID,
    };

    pub fn scan(monitor: &tauri::Monitor) -> Option<(Vec<Candidate>, Rect)> {
        let windows = copy_window_info(
            kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
            kCGNullWindowID,
        )?;
        let own = std::process::id() as i64;
        let stack = windows
            .get_all_values()
            .into_iter()
            .filter_map(|raw| {
                let info = unsafe { CFDictionary::<CFString, CFType>::wrap_under_get_rule(raw as CFDictionaryRef) };
                if number(&info, unsafe { kCGWindowLayer })? != 0 {
                    return None;
                }
                let bounds = info
                    .find(unsafe { CFString::wrap_under_get_rule(kCGWindowBounds) })?
                    .downcast::<CFDictionary>()?;
                let r = CGRect::from_dict_representation(&bounds)?;
                Some(Candidate {
                    rect: Rect {
                        x: r.origin.x,
                        y: r.origin.y,
                        w: r.size.width,
                        h: r.size.height,
                    },
                    own: number(&info, unsafe { kCGWindowOwnerPID }) == Some(own),
                    desktop: false,
                    pinned: false,
                })
            })
            .collect();
        Some((stack, points_rect(monitor)))
    }

    fn number(
        info: &CFDictionary<CFString, CFType>,
        key: core_foundation::string::CFStringRef,
    ) -> Option<i64> {
        info.find(unsafe { CFString::wrap_under_get_rule(key) })?
            .downcast::<CFNumber>()?
            .to_i64()
    }

    fn points_rect(monitor: &tauri::Monitor) -> Rect {
        let scale = monitor.scale_factor();
        let r = super::physical_rect(monitor);
        Rect {
            x: r.x / scale,
            y: r.y / scale,
            w: r.w / scale,
            h: r.h / scale,
        }
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod platform {
    use super::{physical_rect, Candidate, Rect};
    use std::os::raw::{c_int, c_uchar, c_uint, c_ulong};
    use x11::xlib;

    struct Atoms {
        stacking: c_ulong,
        state: c_ulong,
        fullscreen: c_ulong,
        hidden: c_ulong,
        above: c_ulong,
        pid: c_ulong,
    }

    // A Wayland session gives no view of other clients' windows; toasts there cannot be raised over
    // fullscreen windows anyway, so detection is skipped.
    pub fn scan(monitor: &tauri::Monitor) -> Option<(Vec<Candidate>, Rect)> {
        if std::env::var("XDG_SESSION_TYPE").is_ok_and(|s| s == "wayland") {
            return None;
        }
        unsafe {
            let dpy = xlib::XOpenDisplay(std::ptr::null());
            if dpy.is_null() {
                return None;
            }
            let stack = stacking_candidates(dpy);
            xlib::XCloseDisplay(dpy);
            Some((stack, physical_rect(monitor)))
        }
    }

    unsafe fn stacking_candidates(dpy: *mut xlib::Display) -> Vec<Candidate> {
        let root = xlib::XDefaultRootWindow(dpy);
        let atoms = Atoms {
            stacking: atom(dpy, c"_NET_CLIENT_LIST_STACKING"),
            state: atom(dpy, c"_NET_WM_STATE"),
            fullscreen: atom(dpy, c"_NET_WM_STATE_FULLSCREEN"),
            hidden: atom(dpy, c"_NET_WM_STATE_HIDDEN"),
            above: atom(dpy, c"_NET_WM_STATE_ABOVE"),
            pid: atom(dpy, c"_NET_WM_PID"),
        };
        let own = std::process::id() as c_ulong;
        property(dpy, root, atoms.stacking)
            .into_iter()
            .rev()
            .filter_map(|win| candidate(dpy, root, win, &atoms, own))
            .collect()
    }

    unsafe fn candidate(
        dpy: *mut xlib::Display,
        root: c_ulong,
        win: c_ulong,
        atoms: &Atoms,
        own: c_ulong,
    ) -> Option<Candidate> {
        let states = property(dpy, win, atoms.state);
        if states.contains(&atoms.hidden) {
            return None;
        }
        let rect = root_rect(dpy, root, win)?;
        let fullscreen = states.contains(&atoms.fullscreen);
        Some(Candidate {
            rect,
            own: property(dpy, win, atoms.pid).first() == Some(&own),
            desktop: false,
            pinned: states.contains(&atoms.above) && !fullscreen,
        })
    }

    unsafe fn root_rect(dpy: *mut xlib::Display, root: c_ulong, win: c_ulong) -> Option<Rect> {
        let (mut parent, mut x, mut y) = (0, 0, 0);
        let (mut w, mut h, mut border, mut depth): (c_uint, c_uint, c_uint, c_uint) = (0, 0, 0, 0);
        let (mut gx, mut gy): (c_int, c_int) = (0, 0);
        if xlib::XGetGeometry(dpy, win, &mut parent, &mut gx, &mut gy, &mut w, &mut h, &mut border, &mut depth) == 0 {
            return None;
        }
        let mut child = 0;
        if xlib::XTranslateCoordinates(dpy, win, root, 0, 0, &mut x, &mut y, &mut child) == 0 || w == 0 || h == 0 {
            return None;
        }
        Some(Rect {
            x: x as f64,
            y: y as f64,
            w: w as f64,
            h: h as f64,
        })
    }

    unsafe fn atom(dpy: *mut xlib::Display, name: &std::ffi::CStr) -> c_ulong {
        xlib::XInternAtom(dpy, name.as_ptr(), xlib::False)
    }

    unsafe fn property(dpy: *mut xlib::Display, win: c_ulong, prop: c_ulong) -> Vec<c_ulong> {
        let (mut actual_type, mut format, mut count, mut remaining) = (0, 0, 0, 0);
        let mut data: *mut c_uchar = std::ptr::null_mut();
        let status = xlib::XGetWindowProperty(
            dpy,
            win,
            prop,
            0,
            4096,
            xlib::False,
            xlib::AnyPropertyType as c_ulong,
            &mut actual_type,
            &mut format,
            &mut count,
            &mut remaining,
            &mut data,
        );
        if status != xlib::Success as c_int || data.is_null() {
            return Vec::new();
        }
        let values = if format == 32 {
            std::slice::from_raw_parts(data as *const c_ulong, count as usize).to_vec()
        } else {
            Vec::new()
        };
        xlib::XFree(data as *mut _);
        values
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DISPLAY: Rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 1920.0,
        h: 1080.0,
    };
    const FULL: Rect = DISPLAY;
    const MAXIMIZED: Rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 1920.0,
        h: 1032.0,
    };
    const SMALL: Rect = Rect {
        x: 100.0,
        y: 100.0,
        w: 400.0,
        h: 300.0,
    };
    const OTHER_DISPLAY: Rect = Rect {
        x: 1920.0,
        y: 0.0,
        w: 1920.0,
        h: 1080.0,
    };

    fn window(rect: Rect) -> Candidate {
        Candidate {
            rect,
            own: false,
            desktop: false,
            pinned: false,
        }
    }

    #[test]
    fn another_app_covering_the_display_is_fullscreen() {
        assert!(foreign_window_covers(&[window(FULL)], &DISPLAY));
    }

    #[test]
    fn own_fullscreen_window_does_not_count() {
        let own = Candidate { own: true, ..window(FULL) };
        assert!(!foreign_window_covers(&[own, window(FULL)], &DISPLAY));
    }

    #[test]
    fn desktop_window_does_not_count() {
        let desktop = Candidate { desktop: true, ..window(FULL) };
        assert!(!foreign_window_covers(&[desktop], &DISPLAY));
    }

    #[test]
    fn maximized_window_leaves_the_taskbar_visible() {
        assert!(!foreign_window_covers(&[window(MAXIMIZED), window(FULL)], &DISPLAY));
    }

    #[test]
    fn pinned_small_window_is_looked_through() {
        let pinned = Candidate { pinned: true, ..window(SMALL) };
        assert!(foreign_window_covers(&[pinned, window(FULL)], &DISPLAY));
    }

    #[test]
    fn fullscreen_on_another_display_is_ignored() {
        let elsewhere = window(OTHER_DISPLAY);
        assert!(!foreign_window_covers(&[elsewhere, window(SMALL)], &DISPLAY));
    }
}
