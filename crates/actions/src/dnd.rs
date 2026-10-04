//! Do Not Disturb, switched quietly with no window shown.
//!
//! Windows has no public API that sets Do Not Disturb (the Focus session
//! API needs a token Microsoft issues per app). So this does what admin
//! scripts and taskbar-focus (github.com/yoelrosenthal/taskbar-focus, MIT)
//! do:
//!
//! 1. Swap the active quiet-hours profile name inside the CloudStore blob in
//!    the registry: `...Unrestricted` is off, `...PriorityOnly` is on (what
//!    Windows calls Do Not Disturb). Both names are 40 characters, so the
//!    blob never changes length and nothing else in it is touched. Its
//!    timestamp is refreshed so every copy agrees.
//! 2. Restart the per-user notification service (`WpnUserService_*`, no
//!    admin needed), which caches the setting and only then picks it up.
//! 3. Read the state in effect back (a WNF value) to confirm it applied.
//!
//! Undocumented by nature: if a Windows update moves things, this reports
//! a failure instead of claiming success. The bell by the clock may lag
//! until Explorer refreshes; notifications are muted either way.

/// Do Not Disturb off.
pub const UNRESTRICTED: &str = "Microsoft.QuietHoursProfile.Unrestricted";
/// Do Not Disturb on; the user's priority apps and contacts still get through.
pub const PRIORITY_ONLY: &str = "Microsoft.QuietHoursProfile.PriorityOnly";
const PREFIX: &str = "Microsoft.QuietHoursProfile.";

fn utf16le(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

/// Where the profile name sits in the blob: (byte offset, characters). The
/// byte before the name holds its length.
fn locate(blob: &[u8]) -> Option<(usize, usize)> {
    let needle = utf16le(PREFIX);
    let at = blob.windows(needle.len()).position(|w| w == needle)?;
    let count = usize::from(*blob.get(at.checked_sub(1)?)?);
    (at + count * 2 <= blob.len()).then_some((at, count))
}

/// The profile name stored in `blob`.
pub fn profile(blob: &[u8]) -> Option<String> {
    let (at, count) = locate(blob)?;
    let units: Vec<u16> = blob[at..at + count * 2]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u16::from_le_bytes(*b))
        .collect();
    Some(String::from_utf16_lossy(&units))
}

/// The blob's "changed at" field: tag `2a 06` at byte 8, then five bytes of
/// LEB128 Unix seconds (enough until 2106, so the length never changes).
const STAMP_TAG: usize = 8;
const STAMP_AT: usize = 10;
const STAMP_LEN: usize = 5;

fn stamp(out: &mut [u8], unix: u64) {
    if out.len() < STAMP_AT + STAMP_LEN || out[STAMP_TAG..STAMP_TAG + 2] != [0x2a, 0x06] {
        return;
    }
    let mut v = unix;
    for i in 0..STAMP_LEN {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        out[STAMP_AT + i] = if i < STAMP_LEN - 1 { byte | 0x80 } else { byte };
    }
}

/// A copy of `blob` with the profile set to `name` and the stamp to `unix`.
/// Refused when the name would change the blob's length.
pub fn with_profile(blob: &[u8], name: &str, unix: u64) -> Option<Vec<u8>> {
    let (at, count) = locate(blob)?;
    if name.encode_utf16().count() != count {
        return None;
    }
    let mut out = blob.to_vec();
    out[at..at + count * 2].copy_from_slice(&utf16le(name));
    stamp(&mut out, unix);
    Some(out)
}

/// Whether Do Not Disturb is in effect right now, from Windows itself.
/// None when it cannot be read (another OS, or a build that moved it).
pub fn state() -> Option<bool> {
    sys::effective()
}

/// What switching did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Switched {
    /// Changed, and Windows confirms it.
    Done,
    /// It was already that way.
    Already,
    /// Written and the service restarted, but Windows did not report the
    /// new state in time.
    Unconfirmed,
}

/// Turns Do Not Disturb on or off.
pub fn set(on: bool) -> Result<Switched, String> {
    let want = if on { PRIORITY_ONLY } else { UNRESTRICTED };
    let targets = sys::targets();
    if targets.is_empty() {
        return Err("the Do Not Disturb setting was not found in the registry".into());
    }
    // Stored and in effect both agree already: nothing to do.
    let stored = targets
        .iter()
        .all(|(_, b)| profile(b).as_deref() == Some(want));
    if stored && state() == Some(on) {
        return Ok(Switched::Already);
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let wrote = targets
        .iter()
        .filter_map(|(path, blob)| Some((path, with_profile(blob, want, now)?)))
        .filter(|(path, blob)| sys::write(path, blob))
        .count();
    if wrote == 0 {
        return Err("could not write the Do Not Disturb setting".into());
    }
    if !sys::restart_notifications() {
        return Err("could not restart the Windows notification service".into());
    }
    // Right after the restart the old value still shows for a moment.
    let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while std::time::Instant::now() < until {
        match state() {
            Some(now_on) if now_on == on => return Ok(Switched::Done),
            None => return Ok(Switched::Unconfirmed),
            _ => std::thread::sleep(std::time::Duration::from_millis(200)),
        }
    }
    Ok(Switched::Unconfirmed)
}

#[cfg(windows)]
mod sys {
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, REG_BINARY, RegCloseKey, RegEnumKeyExW,
        RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
    };
    use windows_sys::Win32::System::Services::{
        CloseServiceHandle, ControlService, ENUM_SERVICE_STATUS_PROCESSW, EnumServicesStatusExW,
        OpenSCManagerW, OpenServiceW, QueryServiceStatus, SC_ENUM_PROCESS_INFO, SC_HANDLE,
        SC_MANAGER_CONNECT, SC_MANAGER_ENUMERATE_SERVICE, SERVICE_CONTROL_STOP,
        SERVICE_QUERY_STATUS, SERVICE_RUNNING, SERVICE_START, SERVICE_STATE_ALL, SERVICE_STATUS,
        SERVICE_STOP, SERVICE_STOPPED, StartServiceW,
    };

    const BASE: &str =
        r"Software\Microsoft\Windows\CurrentVersion\CloudStore\Store\DefaultAccount\Current";
    /// `WNF_SHEL_QUIETHOURS_ACTIVE_PROFILE_CHANGED`: 0 off, 1 priority only,
    /// 2 alarms only.
    const WNF_QUIET_HOURS: u64 = 0x0D83_063E_A3BF_1C75;
    /// Per-user service instances.
    const USER_SERVICES: u32 = 0xF0;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    struct Key(HKEY);

    impl Key {
        fn open(path: &str, access: u32) -> Option<Key> {
            let w = wide(path);
            let mut h: HKEY = null_mut();
            let rc = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, w.as_ptr(), 0, access, &mut h) };
            (rc == ERROR_SUCCESS).then_some(Key(h))
        }

        fn subkeys(&self) -> Vec<String> {
            let mut out = Vec::new();
            for i in 0.. {
                let mut buf = [0u16; 512];
                let mut len = buf.len() as u32;
                let rc = unsafe {
                    RegEnumKeyExW(
                        self.0,
                        i,
                        buf.as_mut_ptr(),
                        &mut len,
                        null(),
                        null_mut(),
                        null_mut(),
                        null_mut(),
                    )
                };
                if rc != ERROR_SUCCESS {
                    break;
                }
                out.push(String::from_utf16_lossy(&buf[..len as usize]));
            }
            out
        }

        fn binary(&self, name: &str) -> Option<Vec<u8>> {
            let w = wide(name);
            let mut size = 0u32;
            let rc = unsafe {
                RegQueryValueExW(
                    self.0,
                    w.as_ptr(),
                    null(),
                    null_mut(),
                    null_mut(),
                    &mut size,
                )
            };
            if rc != ERROR_SUCCESS || size == 0 {
                return None;
            }
            let mut buf = vec![0u8; size as usize];
            let rc = unsafe {
                RegQueryValueExW(
                    self.0,
                    w.as_ptr(),
                    null(),
                    null_mut(),
                    buf.as_mut_ptr(),
                    &mut size,
                )
            };
            (rc == ERROR_SUCCESS).then(|| {
                buf.truncate(size as usize);
                buf
            })
        }
    }

    impl Drop for Key {
        fn drop(&mut self) {
            unsafe { RegCloseKey(self.0) };
        }
    }

    /// Every quiet-hours blob: there is a default one and one per account,
    /// and the container's name changed between Windows versions.
    pub fn targets() -> Vec<(String, Vec<u8>)> {
        let Some(base) = Key::open(BASE, KEY_READ) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for container in base.subkeys() {
            if !container
                .to_ascii_lowercase()
                .contains("quiethourssettings")
            {
                continue;
            }
            let path = format!(r"{BASE}\{container}");
            let Some(ck) = Key::open(&path, KEY_READ) else {
                continue;
            };
            for child in ck.subkeys() {
                let leaf = format!(r"{path}\{child}");
                if let Some(blob) = Key::open(&leaf, KEY_READ).and_then(|k| k.binary("Data"))
                    && super::profile(&blob).is_some()
                {
                    out.push((leaf, blob));
                }
            }
        }
        out
    }

    pub fn write(path: &str, blob: &[u8]) -> bool {
        let Some(k) = Key::open(path, KEY_READ | KEY_SET_VALUE) else {
            return false;
        };
        let name = wide("Data");
        let rc = unsafe {
            RegSetValueExW(
                k.0,
                name.as_ptr(),
                0,
                REG_BINARY,
                blob.as_ptr(),
                blob.len() as u32,
            )
        };
        rc == ERROR_SUCCESS
    }

    type QueryWnf = unsafe extern "system" fn(
        *const u64,
        *const u8,
        *const u8,
        *mut u32,
        *mut u8,
        *mut u32,
    ) -> i32;

    pub fn effective() -> Option<bool> {
        unsafe {
            let ntdll = GetModuleHandleA(c"ntdll.dll".as_ptr().cast());
            if ntdll.is_null() {
                return None;
            }
            let f = GetProcAddress(ntdll, c"NtQueryWnfStateData".as_ptr().cast())?;
            let f: QueryWnf = std::mem::transmute(f);
            let mut buf = [0u8; 16];
            let mut size = buf.len() as u32;
            let mut stamp = 0u32;
            let status = f(
                &WNF_QUIET_HOURS,
                null(),
                null(),
                &mut stamp,
                buf.as_mut_ptr(),
                &mut size,
            );
            if status < 0 || size < 4 {
                return None;
            }
            Some(u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) != 0)
        }
    }

    fn state_of(svc: SC_HANDLE) -> Option<u32> {
        let mut st: SERVICE_STATUS = unsafe { std::mem::zeroed() };
        (unsafe { QueryServiceStatus(svc, &mut st) } != 0).then_some(st.dwCurrentState)
    }

    fn wait_for(svc: SC_HANDLE, want: u32) -> bool {
        for _ in 0..50 {
            match state_of(svc) {
                Some(s) if s == want => return true,
                None => return false,
                _ => std::thread::sleep(std::time::Duration::from_millis(100)),
            }
        }
        false
    }

    fn user_services(scm: SC_HANDLE) -> Vec<Vec<u16>> {
        let mut out = Vec::new();
        let (mut needed, mut count, mut resume) = (0u32, 0u32, 0u32);
        unsafe {
            EnumServicesStatusExW(
                scm,
                SC_ENUM_PROCESS_INFO,
                USER_SERVICES,
                SERVICE_STATE_ALL,
                null_mut(),
                0,
                &mut needed,
                &mut count,
                &mut resume,
                null(),
            );
            if needed == 0 {
                return out;
            }
            // u64 words keep the entries aligned.
            let mut buf = vec![0u64; (needed as usize).div_ceil(8)];
            resume = 0;
            let ok = EnumServicesStatusExW(
                scm,
                SC_ENUM_PROCESS_INFO,
                USER_SERVICES,
                SERVICE_STATE_ALL,
                buf.as_mut_ptr().cast(),
                (buf.len() * 8) as u32,
                &mut needed,
                &mut count,
                &mut resume,
                null(),
            );
            if ok == 0 {
                return out;
            }
            let entries = std::slice::from_raw_parts(
                buf.as_ptr().cast::<ENUM_SERVICE_STATUS_PROCESSW>(),
                count as usize,
            );
            for e in entries {
                let mut len = 0;
                while *e.lpServiceName.add(len) != 0 {
                    len += 1;
                }
                let name = std::slice::from_raw_parts(e.lpServiceName, len);
                if String::from_utf16_lossy(name).starts_with("WpnUserService_") {
                    out.push(name.iter().copied().chain(std::iter::once(0)).collect());
                }
            }
        }
        out
    }

    /// Stops and starts every `WpnUserService_*`; true when one came back.
    pub fn restart_notifications() -> bool {
        unsafe {
            let scm = OpenSCManagerW(
                null(),
                null(),
                SC_MANAGER_CONNECT | SC_MANAGER_ENUMERATE_SERVICE,
            );
            if scm.is_null() {
                return false;
            }
            let mut restarted = false;
            for name in user_services(scm) {
                let svc = OpenServiceW(
                    scm,
                    name.as_ptr(),
                    SERVICE_START | SERVICE_STOP | SERVICE_QUERY_STATUS,
                );
                if svc.is_null() {
                    continue;
                }
                let mut st: SERVICE_STATUS = std::mem::zeroed();
                ControlService(svc, SERVICE_CONTROL_STOP, &mut st);
                wait_for(svc, SERVICE_STOPPED);
                // Never leave notifications stopped: keep trying to start.
                for _ in 0..20 {
                    if StartServiceW(svc, 0, null()) != 0 || state_of(svc) == Some(SERVICE_RUNNING)
                    {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                restarted |= wait_for(svc, SERVICE_RUNNING);
                CloseServiceHandle(svc);
            }
            CloseServiceHandle(scm);
            restarted
        }
    }
}

#[cfg(not(windows))]
mod sys {
    pub fn targets() -> Vec<(String, Vec<u8>)> {
        Vec::new()
    }
    pub fn write(_path: &str, _blob: &[u8]) -> bool {
        false
    }
    pub fn effective() -> Option<bool> {
        None
    }
    pub fn restart_notifications() -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `windows.data.donotdisturb.quiethourssettings\...\Data` as captured on
    /// Windows 11 Pro 26200 with Do Not Disturb off (from taskbar-focus).
    const BLOB: [u8; 116] = [
        0x43, 0x42, 0x01, 0x00, 0x0a, 0x02, 0x01, 0x00, 0x2a, 0x06, 0x8c, 0x95, 0xfd, 0xc5, 0x06,
        0x2a, 0x2b, 0x0e, 0x5e, 0x43, 0x42, 0x01, 0x00, 0xc2, 0x0a, 0x01, 0xd2, 0x14, 0x28, 0x4d,
        0x00, 0x69, 0x00, 0x63, 0x00, 0x72, 0x00, 0x6f, 0x00, 0x73, 0x00, 0x6f, 0x00, 0x66, 0x00,
        0x74, 0x00, 0x2e, 0x00, 0x51, 0x00, 0x75, 0x00, 0x69, 0x00, 0x65, 0x00, 0x74, 0x00, 0x48,
        0x00, 0x6f, 0x00, 0x75, 0x00, 0x72, 0x00, 0x73, 0x00, 0x50, 0x00, 0x72, 0x00, 0x6f, 0x00,
        0x66, 0x00, 0x69, 0x00, 0x6c, 0x00, 0x65, 0x00, 0x2e, 0x00, 0x55, 0x00, 0x6e, 0x00, 0x72,
        0x00, 0x65, 0x00, 0x73, 0x00, 0x74, 0x00, 0x72, 0x00, 0x69, 0x00, 0x63, 0x00, 0x74, 0x00,
        0x65, 0x00, 0x64, 0x00, 0xca, 0x28, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    const STAMPED: u64 = 1_757_366_924;

    #[test]
    fn reads_and_swaps_the_profile_in_place() {
        assert_eq!(profile(&BLOB).as_deref(), Some(UNRESTRICTED));
        let on = with_profile(&BLOB, PRIORITY_ONLY, STAMPED).unwrap();
        assert_eq!(on.len(), BLOB.len());
        assert_eq!(profile(&on).as_deref(), Some(PRIORITY_ONLY));
        // Back again, with the same stamp, gives the original bytes.
        assert_eq!(with_profile(&on, UNRESTRICTED, STAMPED).unwrap(), BLOB);
    }

    #[test]
    fn refreshes_the_stamp_without_resizing() {
        let out = with_profile(&BLOB, PRIORITY_ONLY, 1_785_520_746).unwrap();
        assert_eq!(out.len(), BLOB.len());
        assert_ne!(
            out[STAMP_AT..STAMP_AT + STAMP_LEN],
            BLOB[STAMP_AT..STAMP_AT + STAMP_LEN]
        );
        assert_eq!(out[..STAMP_AT], BLOB[..STAMP_AT]);
    }

    #[test]
    fn refuses_names_that_change_the_length_and_bad_blobs() {
        assert!(with_profile(&BLOB, "Microsoft.QuietHoursProfile.AlarmsOnly", 0).is_none());
        for cut in 0..BLOB.len() {
            let _ = profile(&BLOB[..cut]);
            let _ = with_profile(&BLOB[..cut], PRIORITY_ONLY, 0);
        }
        assert!(set(true).is_err() || cfg!(windows));
    }
}
