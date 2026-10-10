//! sidekick-indexer: the Windows service that keeps the live drive index.
//!
//!   sidekick-indexer install --profile C:\Users\Ana   (admin) adds and starts it
//!   sidekick-indexer uninstall                        (admin) stops and removes it
//!   sidekick-indexer once                             runs in this console, for testing
//!   sidekick-indexer run                              what Windows starts

#[cfg(not(windows))]
fn main() {
    eprintln!("sidekick-indexer runs on Windows only");
    std::process::exit(1);
}

#[cfg(windows)]
fn main() {
    if let Err(e) = service::main() {
        eprintln!("sidekick-indexer: {e}");
        std::process::exit(1);
    }
}

#[cfg(windows)]
mod service {
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
    use std::time::{Duration, Instant};

    use sidekick_core::names::NameIndex;
    use sidekick_ntfs::volume::Volume;
    use sidekick_ntfs::{Change, Config, Tree};
    use windows_sys::Win32::System::Services::*;

    const NAME: &str = "SidekickIndexer";
    const DISPLAY: &str = "Sidekick drive index";
    const MARK_EVERY: Duration = Duration::from_secs(60);

    static STOP: AtomicBool = AtomicBool::new(false);
    static STATUS: AtomicPtr<core::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());

    type Res<T> = Result<T, Box<dyn std::error::Error>>;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    fn os_err() -> Box<dyn std::error::Error> {
        Box::new(std::io::Error::last_os_error())
    }

    /// C:\ProgramData\Sidekick, readable by the app, written by the service.
    pub fn dir() -> PathBuf {
        let base = std::env::var_os("ProgramData").unwrap_or_else(|| "C:\\ProgramData".into());
        PathBuf::from(base).join("Sidekick")
    }

    pub fn main() -> Res<()> {
        let args: Vec<String> = std::env::args().skip(1).collect();
        match args.first().map(String::as_str) {
            Some("install") => {
                let profile = args
                    .iter()
                    .position(|a| a == "--profile")
                    .and_then(|i| args.get(i + 1))
                    .cloned()
                    .unwrap_or_default();
                install(&profile)
            }
            Some("uninstall") => uninstall(),
            Some("once") => {
                work();
                Ok(())
            }
            Some("run") => dispatch(),
            _ => Err("usage: sidekick-indexer install --profile <home> | uninstall | once".into()),
        }
    }

    fn install(profile: &str) -> Res<()> {
        std::fs::create_dir_all(dir())?;
        let config = Config {
            profile: profile.to_string(),
        };
        std::fs::write(dir().join("indexer.json"), serde_json::to_vec(&config)?)?;
        let exe = std::env::current_exe()?;
        let command = wide(&format!("\"{}\" run", exe.display()));
        unsafe {
            let scm = OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_ALL_ACCESS);
            if scm.is_null() {
                return Err(os_err());
            }
            let mut svc = OpenServiceW(scm, wide(NAME).as_ptr(), SERVICE_ALL_ACCESS);
            if svc.is_null() {
                svc = CreateServiceW(
                    scm,
                    wide(NAME).as_ptr(),
                    wide(DISPLAY).as_ptr(),
                    SERVICE_ALL_ACCESS,
                    SERVICE_WIN32_OWN_PROCESS,
                    SERVICE_AUTO_START,
                    SERVICE_ERROR_NORMAL,
                    command.as_ptr(),
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                );
            }
            if svc.is_null() {
                let e = os_err();
                CloseServiceHandle(scm);
                return Err(e);
            }
            // Already running is fine: it rereads the config on next start.
            StartServiceW(svc, 0, std::ptr::null());
            CloseServiceHandle(svc);
            CloseServiceHandle(scm);
        }
        Ok(())
    }

    fn uninstall() -> Res<()> {
        unsafe {
            let scm = OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_ALL_ACCESS);
            if scm.is_null() {
                return Err(os_err());
            }
            let svc = OpenServiceW(scm, wide(NAME).as_ptr(), SERVICE_ALL_ACCESS);
            if !svc.is_null() {
                let mut status: SERVICE_STATUS = std::mem::zeroed();
                ControlService(svc, SERVICE_CONTROL_STOP, &mut status);
                DeleteService(svc);
                CloseServiceHandle(svc);
            }
            CloseServiceHandle(scm);
        }
        let _ = std::fs::remove_file(dir().join("names-live.db"));
        Ok(())
    }

    fn dispatch() -> Res<()> {
        let mut name = wide(NAME);
        let table = [
            SERVICE_TABLE_ENTRYW {
                lpServiceName: name.as_mut_ptr(),
                lpServiceProc: Some(service_main),
            },
            SERVICE_TABLE_ENTRYW {
                lpServiceName: std::ptr::null_mut(),
                lpServiceProc: None,
            },
        ];
        if unsafe { StartServiceCtrlDispatcherW(table.as_ptr()) } == 0 {
            return Err(os_err());
        }
        Ok(())
    }

    fn report(state: SERVICE_STATUS_CURRENT_STATE) {
        let handle = STATUS.load(Ordering::SeqCst);
        if handle.is_null() {
            return;
        }
        let status = SERVICE_STATUS {
            dwServiceType: SERVICE_WIN32_OWN_PROCESS,
            dwCurrentState: state,
            dwControlsAccepted: if state == SERVICE_RUNNING {
                SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN
            } else {
                0
            },
            dwWin32ExitCode: 0,
            dwServiceSpecificExitCode: 0,
            dwCheckPoint: 0,
            dwWaitHint: 0,
        };
        unsafe { SetServiceStatus(handle, &status) };
    }

    unsafe extern "system" fn handler(
        control: u32,
        _: u32,
        _: *mut core::ffi::c_void,
        _: *mut core::ffi::c_void,
    ) -> u32 {
        if control == SERVICE_CONTROL_STOP || control == SERVICE_CONTROL_SHUTDOWN {
            STOP.store(true, Ordering::SeqCst);
            report(SERVICE_STOP_PENDING);
        }
        0
    }

    unsafe extern "system" fn service_main(_: u32, _: *mut windows_sys::core::PWSTR) {
        let handle = unsafe {
            RegisterServiceCtrlHandlerExW(wide(NAME).as_ptr(), Some(handler), std::ptr::null())
        };
        STATUS.store(handle, Ordering::SeqCst);
        report(SERVICE_RUNNING);
        work();
        report(SERVICE_STOPPED);
    }

    struct Drive {
        root: String,
        vol: Volume,
        tree: Tree,
        next: i64,
    }

    /// Reads every NTFS drive in full, then follows their journals until
    /// asked to stop.
    fn work() {
        let config: Config = std::fs::read(dir().join("indexer.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let _ = std::fs::create_dir_all(dir());
        let Ok(mut index) = NameIndex::open_shared(&dir().join("names-live.db")) else {
            return;
        };
        let mut drives: HashMap<char, Drive> = HashMap::new();
        for letter in 'C'..='Z' {
            if let Some(d) = read_drive(letter, &config, &mut index) {
                drives.insert(letter, d);
            }
        }
        let _ = index.mark_live();
        let mut marked = Instant::now();
        while !STOP.load(Ordering::SeqCst) {
            if drives.is_empty() {
                std::thread::sleep(Duration::from_secs(2));
            }
            let mut lost = Vec::new();
            for (letter, d) in drives.iter_mut() {
                match d.vol.changes(d.next) {
                    Ok((records, next)) => {
                        d.next = next;
                        for r in &records {
                            match d.tree.apply(r) {
                                Some(Change::Upsert { path, dir }) if config.keeps(&path) => {
                                    let _ = index.upsert(&path, dir);
                                }
                                Some(Change::Remove { path }) => {
                                    let _ = index.remove(&path);
                                }
                                _ => {}
                            }
                        }
                    }
                    // The journal wrapped or was reset: read the drive again.
                    Err(e) => {
                        eprintln!("{}: {e}", d.root);
                        lost.push(*letter);
                    }
                }
            }
            for letter in lost {
                drives.remove(&letter);
                if let Some(d) = read_drive(letter, &config, &mut index) {
                    drives.insert(letter, d);
                }
            }
            if marked.elapsed() >= MARK_EVERY {
                let _ = index.mark_live();
                marked = Instant::now();
            }
        }
    }

    fn read_drive(letter: char, config: &Config, index: &mut NameIndex) -> Option<Drive> {
        let root = format!("{letter}:\\");
        let vol = Volume::open(letter).ok()?;
        let next = vol.next_usn().ok()?;
        let mut tree = Tree::new(&root);
        vol.enumerate(|r| tree.insert(&r)).ok()?;
        let entries: Vec<(String, bool)> =
            tree.entries().filter(|(p, _)| config.keeps(p)).collect();
        let n = index.replace_under(&root, entries).ok()?;
        eprintln!("{root}: {n} entries");
        Some(Drive {
            root,
            vol,
            tree,
            next,
        })
    }
}
