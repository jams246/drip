#![expect(
    unsafe_code,
    reason = "Windows named-pipe ACLs require raw security attributes"
)]

use std::{ffi::c_void, io, ptr, time::Duration};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::windows::named_pipe::{NamedPipeServer, ServerOptions},
    sync::watch,
    time::{Instant, timeout},
};
use windows::{
    Win32::{
        Foundation::{
            ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_PIPE_BUSY, HLOCAL, LocalFree,
        },
        Security::Authorization::{
            ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
        },
        Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES},
    },
    core::PCWSTR,
};

const OPEN: u8 = 1;
const ACCEPTED: u8 = 1;
const PIPE_TIMEOUT: Duration = Duration::from_millis(500);
const REQUEST_TIMEOUT: Duration = Duration::from_millis(750);
const RETRY_DELAY: Duration = Duration::from_millis(20);

/// Cancellable same-user Open request listener retained by the GUI process.
pub struct OpenListener {
    pipe_name: String,
    server: NamedPipeServer,
}

pub(super) enum OpenRequestError {
    Unavailable,
    Rejected,
    Timeout,
    Transport(io::Error),
}

pub(super) fn create_listener(pipe_name: &str) -> io::Result<OpenListener> {
    Ok(OpenListener {
        pipe_name: pipe_name.to_owned(),
        server: create_server(pipe_name, true)?,
    })
}

pub(super) async fn request_open(pipe_name: &str) -> Result<(), OpenRequestError> {
    let deadline = Instant::now() + REQUEST_TIMEOUT;
    loop {
        match tokio::net::windows::named_pipe::ClientOptions::new()
            .read(true)
            .write(true)
            .open(pipe_name)
        {
            Ok(mut client) => {
                return match timeout(PIPE_TIMEOUT, async {
                    client.write_all(&[OPEN]).await?;
                    client.flush().await?;
                    let mut response = [0];
                    client.read_exact(&mut response).await?;
                    Ok::<_, io::Error>(response[0])
                })
                .await
                {
                    Ok(Ok(ACCEPTED)) => Ok(()),
                    Ok(Ok(_)) => Err(OpenRequestError::Rejected),
                    Ok(Err(error)) => Err(OpenRequestError::Transport(error)),
                    Err(_) => Err(OpenRequestError::Timeout),
                };
            }
            Err(error) if retryable(&error) && Instant::now() < deadline => {
                tokio::time::sleep(RETRY_DELAY).await;
            }
            Err(error) if retryable(&error) => return Err(OpenRequestError::Unavailable),
            Err(error) => return Err(OpenRequestError::Transport(error)),
        }
    }
}

impl OpenListener {
    /// Forwards accepted Open requests through a nonblocking callback.
    pub async fn run<F>(mut self, on_open: F, mut shutdown: watch::Receiver<bool>) -> io::Result<()>
    where
        F: Fn() -> bool + Send + Sync + 'static,
    {
        loop {
            let connected = tokio::select! {
                _ = wait_for_shutdown(&mut shutdown) => return Ok(()),
                connected = self.server.connect() => connected,
            };
            connected?;
            let next = create_server(&self.pipe_name, false);
            let _response = tokio::select! {
                _ = wait_for_shutdown(&mut shutdown) => return Ok(()),
                response = timeout(PIPE_TIMEOUT, serve_one(&mut self.server, &on_open)) => response,
            };
            let _ = self.server.disconnect();
            self.server = next?;
        }
    }
}

async fn serve_one<F>(server: &mut NamedPipeServer, on_open: &F) -> io::Result<()>
where
    F: Fn() -> bool,
{
    let mut request = [0];
    let accepted = server.read_exact(&mut request).await.is_ok() && request[0] == OPEN && on_open();
    server.write_all(&[u8::from(accepted)]).await?;
    server.flush().await
}

async fn wait_for_shutdown(shutdown: &mut watch::Receiver<bool>) {
    loop {
        if *shutdown.borrow() || shutdown.changed().await.is_err() {
            return;
        }
    }
}

fn retryable(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(code)
            if code == ERROR_FILE_NOT_FOUND.0 as i32
                || code == ERROR_PATH_NOT_FOUND.0 as i32
                || code == ERROR_PIPE_BUSY.0 as i32
    )
}

fn create_server(pipe_name: &str, first: bool) -> io::Result<NamedPipeServer> {
    let sid = super::identity::current_user_sid()?;
    let sddl: Vec<u16> = format!("D:P(A;;GA;;;{sid})\0").encode_utf16().collect();
    let mut descriptor = SecurityDescriptor(PSECURITY_DESCRIPTOR(ptr::null_mut()));
    // SAFETY: `sddl` is NUL terminated and `descriptor` is writable.
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl.as_ptr()),
            SDDL_REVISION_1,
            &mut descriptor.0,
            None,
        )
    }
    .map_err(io::Error::other)?;
    let mut attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0.0,
        bInheritHandle: false.into(),
    };
    let mut options = ServerOptions::new();
    options
        .first_pipe_instance(first)
        .reject_remote_clients(true)
        .max_instances(2)
        .in_buffer_size(64)
        .out_buffer_size(64);
    // SAFETY: `attributes` and its descriptor stay live through CreateNamedPipeW.
    unsafe {
        options.create_with_security_attributes_raw(
            pipe_name,
            (&mut attributes as *mut SECURITY_ATTRIBUTES).cast::<c_void>(),
        )
    }
}

struct SecurityDescriptor(PSECURITY_DESCRIPTOR);

impl Drop for SecurityDescriptor {
    fn drop(&mut self) {
        if !self.0.0.is_null() {
            // SAFETY: Windows allocated this descriptor for LocalFree.
            unsafe {
                let _ = LocalFree(Some(HLOCAL(self.0.0)));
            }
        }
    }
}
