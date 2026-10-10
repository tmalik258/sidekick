//! Reads an NTFS drive's file table and change journal. Needs admin
//! rights, which is why this runs inside the Sidekick indexer service.

use std::ffi::c_void;
use std::io;

use windows_sys::Win32::Foundation::{CloseHandle, GENERIC_READ, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::IO::DeviceIoControl;
use windows_sys::Win32::System::Ioctl::{
    CREATE_USN_JOURNAL_DATA, FSCTL_CREATE_USN_JOURNAL, FSCTL_ENUM_USN_DATA,
    FSCTL_QUERY_USN_JOURNAL, FSCTL_READ_USN_JOURNAL, MFT_ENUM_DATA_V0, READ_USN_JOURNAL_DATA_V0,
    USN_JOURNAL_DATA_V0,
};

use crate::{Record, parse_records};

const BUF: usize = 1 << 20;
const ERROR_HANDLE_EOF: i32 = 38;

pub struct Volume {
    handle: HANDLE,
    journal: u64,
}

// The handle is only used from the thread that owns the Volume at a time.
unsafe impl Send for Volume {}

impl Drop for Volume {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.handle) };
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

impl Volume {
    /// Opens drive `letter` (like 'C') and makes sure it has a journal.
    /// Fails for drives that are not NTFS.
    pub fn open(letter: char) -> io::Result<Self> {
        let name = wide(&format!("\\\\.\\{letter}:"));
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_READ,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let mut vol = Self { handle, journal: 0 };
        let create = CREATE_USN_JOURNAL_DATA {
            MaximumSize: 32 << 20,
            AllocationDelta: 8 << 20,
        };
        vol.ioctl(FSCTL_CREATE_USN_JOURNAL, &create, &mut [])?;
        let mut data = [0u8; std::mem::size_of::<USN_JOURNAL_DATA_V0>()];
        vol.ioctl(FSCTL_QUERY_USN_JOURNAL, &(), &mut data)?;
        let info: USN_JOURNAL_DATA_V0 = unsafe { std::ptr::read_unaligned(data.as_ptr().cast()) };
        vol.journal = info.UsnJournalID;
        Ok(vol)
    }

    fn ioctl<T>(&self, code: u32, input: &T, out: &mut [u8]) -> io::Result<u32> {
        let mut got = 0u32;
        let size = std::mem::size_of::<T>() as u32;
        let ok = unsafe {
            DeviceIoControl(
                self.handle,
                code,
                if size == 0 {
                    std::ptr::null()
                } else {
                    (input as *const T).cast::<c_void>()
                },
                size,
                out.as_mut_ptr().cast(),
                out.len() as u32,
                &mut got,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(got)
    }

    /// The journal position to follow from after a full read.
    pub fn next_usn(&self) -> io::Result<i64> {
        let mut data = [0u8; std::mem::size_of::<USN_JOURNAL_DATA_V0>()];
        self.ioctl(FSCTL_QUERY_USN_JOURNAL, &(), &mut data)?;
        let info: USN_JOURNAL_DATA_V0 = unsafe { std::ptr::read_unaligned(data.as_ptr().cast()) };
        Ok(info.NextUsn)
    }

    /// Every file and folder on the drive, read straight from the file table.
    pub fn enumerate(&self, mut each: impl FnMut(Record)) -> io::Result<()> {
        let mut input = MFT_ENUM_DATA_V0 {
            StartFileReferenceNumber: 0,
            LowUsn: 0,
            HighUsn: i64::MAX,
        };
        let mut buf = vec![0u8; BUF];
        loop {
            let got = match self.ioctl(FSCTL_ENUM_USN_DATA, &input, &mut buf) {
                Ok(n) => n as usize,
                Err(e) if e.raw_os_error() == Some(ERROR_HANDLE_EOF) => return Ok(()),
                Err(e) => return Err(e),
            };
            if got <= 8 {
                return Ok(());
            }
            input.StartFileReferenceNumber =
                u64::from_le_bytes(buf[..8].try_into().unwrap_or_default());
            parse_records(&buf[8..got]).into_iter().for_each(&mut each);
        }
    }

    /// Changes since `from`, waiting up to a couple of seconds for new
    /// ones. Returns the records and where to read from next time.
    pub fn changes(&self, from: i64) -> io::Result<(Vec<Record>, i64)> {
        let input = READ_USN_JOURNAL_DATA_V0 {
            StartUsn: from,
            ReasonMask: crate::REASON_CREATE
                | crate::REASON_DELETE
                | crate::REASON_RENAME_OLD
                | crate::REASON_RENAME_NEW
                | crate::REASON_CLOSE,
            ReturnOnlyOnClose: 0,
            Timeout: 2,
            BytesToWaitFor: 1,
            UsnJournalID: self.journal,
        };
        let mut buf = vec![0u8; BUF];
        let got = self.ioctl(FSCTL_READ_USN_JOURNAL, &input, &mut buf)? as usize;
        if got < 8 {
            return Ok((Vec::new(), from));
        }
        let next = i64::from_le_bytes(buf[..8].try_into().unwrap_or_default());
        Ok((parse_records(&buf[8..got]), next))
    }
}
