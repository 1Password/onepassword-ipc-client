//! Client library for communicating with the 1Password desktop app over IPC.
//!
//! This crate provides a cross-platform client that speaks the 1Password IPC
//! protocol, using the native transport on each platform:
//!
//! - **macOS**: Mach ports
//! - **Linux**: Abstract Unix sockets
//! - **Windows**: Named pipes
//!
//! # Quick start
//!
//! ```no_run
//! use onepassword_ipc_client::send_to;
//!
//! let request: Vec<u8> = b"hello 1password".to_vec();
//! let response = send_to("my_port_name", request).unwrap();
//! println!("response: {} bytes", response.len());
//! ```

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
#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
pub fn send_to(endpoint_name: &str, message: Vec<u8>) -> Result<Vec<u8>, ErrorCode> {
    platform::send_to(endpoint_name, message)
}
