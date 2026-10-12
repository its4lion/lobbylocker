//! Windows RunAs readback: local IPC only, bounded, and checked against the
//! helper PID returned by Start-Process. Never accept a privileged file path.
use crate::{inspection::MAX_READBACK_BYTES, Result};
use std::{
    io::Write,
    os::windows::fs::OpenOptionsExt,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{
        GetLastError, ERROR_IO_PENDING, ERROR_PIPE_CONNECTED, INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
        WAIT_TIMEOUT,
    },
    Storage::FileSystem::{
        FlushFileBuffers, ReadFile, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED,
        PIPE_ACCESS_INBOUND, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT,
    },
    System::{
        Pipes::{
            ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId, PIPE_READMODE_BYTE,
            PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
        },
        Threading::{CreateEventW, ResetEvent, WaitForSingleObject},
        IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED},
    },
};

const PREFIX: &str = r"\\.\pipe\LobbyLocker.readback.";
type Reader = JoinHandle<Result<(u32, Vec<u8>)>>;

pub fn validate_name(name: &str) -> Result<()> {
    let token = name.strip_prefix(PREFIX).ok_or("Invalid readback pipe.")?;
    if token.len() != 32 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Invalid readback pipe.".into());
    }
    Ok(())
}

pub fn send(name: &str, bytes: &[u8]) -> Result<()> {
    validate_name(name)?;
    if bytes.len() > MAX_READBACK_BYTES {
        return Err("Firewall readback is too large.".into());
    }
    let mut pipe = std::fs::OpenOptions::new()
        .write(true)
        .share_mode(0)
        // A user-owned pipe server may read the result and client PID, but must
        // never be able to impersonate the elevated helper's administrator token.
        .security_qos_flags(SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION)
        .open(name)
        .map_err(|e| e.to_string())?;
    pipe.write_all(&(bytes.len() as u32).to_le_bytes())
        .and_then(|()| pipe.write_all(bytes))
        .map_err(|e| e.to_string())?;
    // Keep the client connected until the receiver has consumed the report and
    // captured its process ID, including for small responses.
    if unsafe { FlushFileBuffers(pipe.as_raw_handle()) } == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(())
}

pub struct Receiver {
    name: String,
    finished: Arc<AtomicBool>,
    reader: Option<Reader>,
}

impl Receiver {
    pub fn new() -> Result<Self> {
        let name = format!("{PREFIX}{}", uuid::Uuid::new_v4().simple());
        let wide: Vec<_> = name.encode_utf16().chain(Some(0)).collect();
        // OwnedHandle closes exactly once and keeps handles alive throughout all
        // outstanding overlapped I/O. They are not inherited by child processes.
        let handle = unsafe {
            CreateNamedPipeW(
                wide.as_ptr(),
                PIPE_ACCESS_INBOUND | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                0,
                65_536,
                0,
                std::ptr::null(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let pipe = unsafe { OwnedHandle::from_raw_handle(handle) };
        let event = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
        if event.is_null() {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let event = unsafe { OwnedHandle::from_raw_handle(event) };
        let finished = Arc::new(AtomicBool::new(false));
        let done = Arc::clone(&finished);
        let reader = thread::spawn(move || receive(pipe, event, &done));
        Ok(Self {
            name,
            finished,
            reader: Some(reader),
        })
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn finish(mut self) -> Result<(u32, Vec<u8>)> {
        self.finished.store(true, Ordering::Release);
        self.reader
            .take()
            .ok_or("Missing firewall readback reader.")?
            .join()
            .map_err(|_| "Firewall readback reader failed.")?
    }
}

impl Drop for Receiver {
    fn drop(&mut self) {
        self.finished.store(true, Ordering::Release);
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

fn wait(
    pipe: &OwnedHandle,
    overlapped: &mut OVERLAPPED,
    finished: &AtomicBool,
    deadline: Option<Instant>,
) -> Result<u32> {
    loop {
        let result = unsafe { WaitForSingleObject(overlapped.hEvent, 50) };
        if result == WAIT_OBJECT_0 {
            let mut bytes = 0;
            if unsafe { GetOverlappedResult(pipe.as_raw_handle(), overlapped, &mut bytes, 0) } == 0
            {
                return Err(std::io::Error::last_os_error().to_string());
            }
            return Ok(bytes);
        }
        if result != WAIT_TIMEOUT
            || finished.load(Ordering::Acquire)
            || deadline.is_some_and(|time| Instant::now() >= time)
        {
            // Cancel and wait before the OVERLAPPED memory or read buffer goes
            // out of scope. Authentication itself has no artificial time limit.
            unsafe {
                CancelIoEx(pipe.as_raw_handle(), overlapped);
                let mut bytes = 0;
                GetOverlappedResult(pipe.as_raw_handle(), overlapped, &mut bytes, 1);
            }
            return Err("Firewall helper did not return a complete readback.".into());
        }
    }
}

fn receive(pipe: OwnedHandle, event: OwnedHandle, finished: &AtomicBool) -> Result<(u32, Vec<u8>)> {
    let mut overlapped = OVERLAPPED {
        hEvent: event.as_raw_handle(),
        ..unsafe { std::mem::zeroed() }
    };
    if unsafe { ConnectNamedPipe(pipe.as_raw_handle(), &mut overlapped) } == 0 {
        match unsafe { GetLastError() } {
            ERROR_PIPE_CONNECTED => {}
            ERROR_IO_PENDING => {
                wait(&pipe, &mut overlapped, finished, None)?;
            }
            _ => return Err(std::io::Error::last_os_error().to_string()),
        }
    }
    let mut pid = 0;
    if unsafe { GetNamedPipeClientProcessId(pipe.as_raw_handle(), &mut pid) } == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut length = [0; 4];
    read_exact(&pipe, &event, finished, deadline, &mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    if length == 0 || length > MAX_READBACK_BYTES {
        return Err("Invalid firewall readback size.".into());
    }
    let mut bytes = vec![0; length];
    read_exact(&pipe, &event, finished, deadline, &mut bytes)?;
    Ok((pid, bytes))
}

fn read_exact(
    pipe: &OwnedHandle,
    event: &OwnedHandle,
    finished: &AtomicBool,
    deadline: Instant,
    mut buffer: &mut [u8],
) -> Result<()> {
    while !buffer.is_empty() {
        unsafe {
            ResetEvent(event.as_raw_handle());
        }
        let mut overlapped = OVERLAPPED {
            hEvent: event.as_raw_handle(),
            ..unsafe { std::mem::zeroed() }
        };
        let mut bytes = 0;
        let immediate = unsafe {
            ReadFile(
                pipe.as_raw_handle(),
                buffer.as_mut_ptr(),
                buffer.len().min(65_536) as u32,
                &mut bytes,
                &mut overlapped,
            )
        };
        if immediate == 0 {
            if unsafe { GetLastError() } != ERROR_IO_PENDING {
                return Err(std::io::Error::last_os_error().to_string());
            }
            bytes = wait(pipe, &mut overlapped, finished, Some(deadline))?;
        } else if unsafe { GetOverlappedResult(pipe.as_raw_handle(), &overlapped, &mut bytes, 0) }
            == 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
        if bytes == 0 {
            return Err("Incomplete firewall readback.".into());
        }
        buffer = &mut buffer[bytes as usize..];
    }
    Ok(())
}

#[cfg(test)]
#[path = "../tests/unit/readback.rs"]
mod tests;
