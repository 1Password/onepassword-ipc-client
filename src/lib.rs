//! Client library for communicating with integrations exposed by the 1Password desktop app over IPC

pub mod chunking;

#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
mod platform;

#[cfg(target_os = "macos")]
pub use platform::send_with_client;
#[cfg(target_os = "windows")]
pub use platform::send_with_pipe;
#[cfg(target_os = "linux")]
pub use platform::send_with_stream;

/// Represents an error that can occur during IPC communication.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    /// Invalid arguments were provided (e.g. bad endpoint name).
    InvalidArguments,
    /// Failed to connect to the IPC endpoint.
    FailedToConnect,
    /// Failed to send the message.
    FailedToSend,
    /// Failed to receive a response from the server.
    FailedToReceive,
    /// Failed to encode the message into the wire format.
    FailedToEncode,
    /// Failed to decode the response from the wire format.
    FailedToDecode,
    /// The server closed the connection unexpectedly.
    ServerClosedConnection,
    /// An internal error occurred.
    Internal,
}

/// The response from an IPC call, including the responder's identity for reverse attestation.
///
/// Callers should verify [`peer_identity`](IpcResponse::peer_identity) to ensure the response
/// came from the expected server process before trusting the payload.
#[derive(Clone)]
pub struct IpcResponse {
    /// The response payload bytes.
    pub data: Vec<u8>,
    /// The identity of the IPC responder.
    pub peer_identity: PeerIdentity,
}

/// Platform-specific identity of the IPC responder, used for reverse attestation.
///
/// Alias for [`AuditToken`] on macOS.
#[cfg(target_os = "macos")]
pub type PeerIdentity = AuditToken;

/// Platform-specific identity of the IPC responder, used for reverse attestation.
///
/// Alias for [`ProcessId`] on Linux and Windows.
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub type PeerIdentity = ProcessId;

/// A macOS audit token identifying a process.
///
/// See <https://knight.sc/reverse%20engineering/2020/03/20/audit-tokens-explained.html>.
#[cfg(target_os = "macos")]
#[derive(Clone, Copy)]
pub struct AuditToken(mach_listener::audit_token_t);

#[cfg(target_os = "macos")]
#[link(name = "bsm")]
unsafe extern "C" {
    fn audit_token_to_pid(atoken: mach_listener::audit_token_t) -> i32;
}

#[cfg(target_os = "macos")]
impl AuditToken {
    pub(crate) fn new(raw: mach_listener::audit_token_t) -> Self {
        Self(raw)
    }

    /// Returns the raw audit token as a `[u32; 8]` array.
    ///
    /// This can be freely converted back into a platform `audit_token_t`.
    pub fn as_raw(&self) -> [u32; 8] {
        self.0.val
    }

    /// Returns the process ID.
    pub fn pid(&self) -> u32 {
        // SAFETY: `audit_token_to_pid` is a stable macOS API that reads
        // from a valid, initialized `audit_token_t`.
        unsafe { audit_token_to_pid(self.0) as u32 }
    }
}

/// A process identifier.
#[cfg(any(target_os = "linux", target_os = "windows"))]
#[derive(Clone, Copy)]
pub struct ProcessId(u32);

#[cfg(any(target_os = "linux", target_os = "windows"))]
impl ProcessId {
    pub(crate) fn new(pid: u32) -> Self {
        Self(pid)
    }

    /// Returns the process ID.
    pub fn pid(&self) -> u32 {
        self.0
    }
}

/// Sends a message to the IPC server at the given endpoint and returns the response.
///
/// This is a synchronous, blocking call that creates a fresh connection per invocation.
///
/// The `endpoint_name` is interpreted as a Mach port name on macOS, an abstract Unix socket
/// name on Linux, or a named pipe path on Windows.
///
/// Returns an [`IpcResponse`] containing the response bytes and the responder's
/// [`PeerIdentity`] on success, or an [`ErrorCode`] on failure.
///
/// # Examples
///
/// ```no_run
/// use onepassword_ipc_client::{send_to, ErrorCode};
///
/// let request = b"my request payload".to_vec();
/// match send_to("your_endpoint_name", request) {
///     Ok(response) => {
///         println!("Got {} bytes back from PID {}", response.data.len(), response.peer_identity.pid());
///     }
///     Err(err) => eprintln!("IPC failed: {:?}", err),
/// }
/// ```
#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
pub fn send_to(endpoint_name: &str, message: Vec<u8>) -> Result<IpcResponse, ErrorCode> {
    platform::send_to(endpoint_name, message)
}
