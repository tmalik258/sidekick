//! Window layouts: where each app's windows sit, saved per
//! monitor setup, so plugging a monitor back in puts things back.

use serde::{Deserialize, Serialize};

/// Where one window was, matched later by program and title.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Placement {
    pub exe: String,
    pub title: String,
    /// The restored (not maximized) rectangle: left, top, right, bottom.
    pub rect: [i32; 4],
    pub maximized: bool,
    pub minimized: bool,
}

/// A live window that can be moved.
#[derive(Debug, Clone)]
pub struct Window {
    pub handle: isize,
    pub exe: String,
    pub title: String,
}

/// Pairs saved placements with live windows: same program and title first,
/// then the same program when only one window of it is left.
pub fn plan(saved: &[Placement], live: &[Window]) -> Vec<(isize, Placement)> {
    let mut used = vec![false; live.len()];
    let mut out = Vec::new();
    let mut leftovers = Vec::new();
    for p in saved {
        match live
            .iter()
            .enumerate()
            .find(|(i, w)| !used[*i] && w.exe.eq_ignore_ascii_case(&p.exe) && w.title == p.title)
        {
            Some((i, w)) => {
                used[i] = true;
                out.push((w.handle, p.clone()));
            }
            None => leftovers.push(p),
        }
    }
    for p in leftovers {
        let candidates: Vec<usize> = live
            .iter()
            .enumerate()
            .filter(|(i, w)| !used[*i] && w.exe.eq_ignore_ascii_case(&p.exe))
            .map(|(i, _)| i)
            .collect();
        if let [i] = candidates[..] {
            used[i] = true;
            out.push((live[i].handle, p.clone()));
        }
    }
    out
}

#[cfg(windows)]
mod win {
    use super::{Placement, Window};
    use windows_sys::Win32::Foundation::{CloseHandle, HWND, LPARAM, POINT, RECT};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        QueryFullProcessImageNameW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GW_OWNER, GWL_EXSTYLE, GetWindow, GetWindowLongW, GetWindowPlacement,
        GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
        SW_SHOWMAXIMIZED, SW_SHOWMINIMIZED, SW_SHOWNORMAL, SetWindowPlacement, WINDOWPLACEMENT,
        WS_EX_TOOLWINDOW,
    };

    fn exe_of(hwnd: HWND) -> Option<String> {
        let mut pid = 0u32;
        // SAFETY: plain Win32 calls with valid out pointers; the process
        // handle is closed before returning.
        unsafe {
            GetWindowThreadProcessId(hwnd, &mut pid);
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if process.is_null() {
                return None;
            }
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let ok =
                QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len);
            CloseHandle(process);
            if ok == 0 {
                return None;
            }
            let path = String::from_utf16_lossy(&buf[..len as usize]);
            path.rsplit(['\\', '/']).next().map(str::to_ascii_lowercase)
        }
    }

    fn title_of(hwnd: HWND) -> String {
        // SAFETY: the buffer is sized from GetWindowTextLengthW.
        unsafe {
            let len = GetWindowTextLengthW(hwnd);
            if len <= 0 {
                return String::new();
            }
            let mut buf = vec![0u16; len as usize + 1];
            let n = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
            String::from_utf16_lossy(&buf[..n.max(0) as usize])
        }
    }

    unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> i32 {
        // SAFETY: lparam is the Vec passed by `windows` below.
        let list = unsafe { &mut *(lparam as *mut Vec<HWND>) };
        let visible = unsafe { IsWindowVisible(hwnd) } != 0;
        let owned = !unsafe { GetWindow(hwnd, GW_OWNER) }.is_null();
        let tool = unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) } as u32 & WS_EX_TOOLWINDOW != 0;
        if visible && !owned && !tool {
            list.push(hwnd);
        }
        1
    }

    fn handles() -> Vec<HWND> {
        let mut list: Vec<HWND> = Vec::new();
        // SAFETY: the callback only runs during this call.
        unsafe { EnumWindows(Some(collect), &mut list as *mut Vec<HWND> as LPARAM) };
        list
    }

    /// Normal app windows: visible, titled, not tool windows.
    pub fn windows() -> Vec<Window> {
        handles()
            .into_iter()
            .filter_map(|h| {
                let title = title_of(h);
                if title.is_empty() {
                    return None;
                }
                Some(Window {
                    handle: h as isize,
                    exe: exe_of(h)?,
                    title,
                })
            })
            .collect()
    }

    pub fn capture() -> Vec<Placement> {
        windows()
            .into_iter()
            .filter_map(|w| {
                let mut wp: WINDOWPLACEMENT = unsafe { std::mem::zeroed() };
                wp.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;
                // SAFETY: wp is initialized with its length as the API requires.
                if unsafe { GetWindowPlacement(w.handle as HWND, &mut wp) } == 0 {
                    return None;
                }
                let r = wp.rcNormalPosition;
                Some(Placement {
                    exe: w.exe,
                    title: w.title,
                    rect: [r.left, r.top, r.right, r.bottom],
                    maximized: wp.showCmd == SW_SHOWMAXIMIZED as u32,
                    minimized: wp.showCmd == SW_SHOWMINIMIZED as u32,
                })
            })
            .collect()
    }

    pub fn apply(handle: isize, p: &Placement) -> bool {
        let mut wp: WINDOWPLACEMENT = unsafe { std::mem::zeroed() };
        wp.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;
        wp.showCmd = if p.minimized {
            SW_SHOWMINIMIZED
        } else if p.maximized {
            SW_SHOWMAXIMIZED
        } else {
            SW_SHOWNORMAL
        } as u32;
        wp.ptMinPosition = POINT { x: -1, y: -1 };
        wp.ptMaxPosition = POINT { x: -1, y: -1 };
        wp.rcNormalPosition = RECT {
            left: p.rect[0],
            top: p.rect[1],
            right: p.rect[2],
            bottom: p.rect[3],
        };
        // SAFETY: a fully initialized placement for a live window handle.
        unsafe { SetWindowPlacement(handle as HWND, &wp) != 0 }
    }
}

/// Where every normal window is now.
pub fn capture() -> Vec<Placement> {
    #[cfg(windows)]
    {
        win::capture()
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

/// Puts windows back; returns how many moved.
pub fn restore(saved: &[Placement]) -> usize {
    #[cfg(windows)]
    {
        plan(saved, &win::windows())
            .iter()
            .filter(|(h, p)| win::apply(*h, p))
            .count()
    }
    #[cfg(not(windows))]
    {
        let _ = saved;
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(exe: &str, title: &str) -> Placement {
        Placement {
            exe: exe.into(),
            title: title.into(),
            rect: [0, 0, 100, 100],
            maximized: false,
            minimized: false,
        }
    }

    fn w(h: isize, exe: &str, title: &str) -> Window {
        Window {
            handle: h,
            exe: exe.into(),
            title: title.into(),
        }
    }

    #[test]
    fn matches_by_title_then_by_program() {
        let saved = [
            p("code.exe", "a - Visual Studio Code"),
            p("slack.exe", "Slack | old"),
            p("chrome.exe", "x"),
        ];
        let live = [
            w(1, "code.exe", "b - Visual Studio Code"),
            w(2, "Code.exe", "a - Visual Studio Code"),
            w(3, "slack.exe", "Slack | new"),
            w(4, "chrome.exe", "y"),
            w(5, "chrome.exe", "z"),
        ];
        let got: Vec<(isize, String)> = plan(&saved, &live)
            .into_iter()
            .map(|(h, p)| (h, p.title))
            .collect();
        assert_eq!(
            got,
            vec![
                (2, "a - Visual Studio Code".to_owned()),
                (3, "Slack | old".to_owned())
            ],
            "two Chrome windows are ambiguous, so neither moves"
        );
    }
}
