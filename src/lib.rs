//! Client library for communicating with integrations exposed by the 1Password desktop app over IPC

pub(crate) mod chunking;

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

/// Sends a message to the IPC server at the given endpoint and returns the response.
///
/// This is a synchronous, blocking call that creates a fresh connection per invocation.
///
/// The `endpoint_name` is interpreted as a Mach port name on macOS, an abstract Unix socket
/// name on Linux, or a named pipe path on Windows.
///
/// Returns the response bytes on success, or an [`ErrorCode`] on failure.
///
/// # Examples
///
/// ```no_run
/// use onepassword_ipc_client::{send_to, ErrorCode};
///
/// let request = b"my request payload".to_vec();
/// match send_to("your_endpoint_name", request) {
///     Ok(response) => println!("Got {} bytes back", response.len()),
///     Err(err) => eprintln!("IPC failed: {:?}", err),
/// }
/// ```
#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
pub fn send_to(endpoint_name: &str, message: Vec<u8>) -> Result<Vec<u8>, ErrorCode> {
    platform::send_to(endpoint_name, message)
}
