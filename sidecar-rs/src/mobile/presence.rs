use std::time::Duration;

const AWAY_AFTER: Duration = Duration::from_secs(120);

pub(super) fn desktop_in_use() -> bool {
    input_idle().is_some_and(|idle| idle < AWAY_AFTER)
}

#[cfg(windows)]
fn input_idle() -> Option<Duration> {
    #[repr(C)]
    struct LastInputInfo {
        cb_size: u32,
        dw_time: u32,
    }
    #[link(name = "user32")]
    extern "system" {
        fn GetLastInputInfo(info: *mut LastInputInfo) -> i32;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetTickCount() -> u32;
    }
    let mut info = LastInputInfo { cb_size: std::mem::size_of::<LastInputInfo>() as u32, dw_time: 0 };
    let ok = unsafe { GetLastInputInfo(&mut info) } != 0;
    ok.then(|| Duration::from_millis(u64::from(unsafe { GetTickCount() }.wrapping_sub(info.dw_time))))
}

#[cfg(target_os = "macos")]
fn input_idle() -> Option<Duration> {
    let out = std::process::Command::new("ioreg").args(["-c", "IOHIDSystem", "-d", "4"]).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().find(|l| l.contains("\"HIDIdleTime\""))?;
    let nanos: u64 = line.rsplit('=').next()?.trim().parse().ok()?;
    Some(Duration::from_nanos(nanos))
}

#[cfg(not(any(windows, target_os = "macos")))]
fn input_idle() -> Option<Duration> {
    None
}

#[cfg(all(test, any(windows, target_os = "macos")))]
mod tests {
    use super::*;

    #[test]
    fn reads_the_input_idle_time() {
        assert!(input_idle().is_some_and(|idle| idle < Duration::from_secs(86_400 * 30)));
    }
}
