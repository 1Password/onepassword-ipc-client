use std::os::linux::net::SocketAddrExt;
use std::os::unix::io::AsRawFd;
use std::os::unix::net::SocketAddr;
use std::os::unix::net::UnixStream;

use super::stream_io::send_and_receive;
use crate::{ErrorCode, IpcResponse, ProcessId};

/// Returns the [`ProcessId`] of the peer at the other end of a Unix stream.
pub fn peer_identity(stream: &UnixStream) -> Result<ProcessId, ErrorCode> {
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: `stream.as_raw_fd()` is a valid socket fd, and `cred`/`len`
    // are valid pointers to appropriately sized buffers.
    let ret = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut cred as *mut _ as *mut libc::c_void,
            &mut len,
        )
    };
    if ret == 0 {
        Ok(ProcessId::new(cred.pid as u32))
    } else {
        Err(ErrorCode::Internal)
    }
}

/// Sends a message to the IPC server at the given endpoint and returns the response.
///
/// Creates a fresh connection per call.
pub fn send_to(endpoint_name: &str, message: Vec<u8>) -> Result<IpcResponse, ErrorCode> {
    let addr =
        SocketAddr::from_abstract_name(endpoint_name).map_err(|_| ErrorCode::InvalidArguments)?;
    let mut stream = UnixStream::connect_addr(&addr).map_err(|_| ErrorCode::FailedToConnect)?;
    let peer = peer_identity(&stream)?;
    let data = send_and_receive(&mut stream, message)?;
    Ok(IpcResponse {
        data,
        peer_identity: peer,
    })
}

/// Sends a message over an existing stream and returns the response.
///
/// This allows reusing the same connection across multiple calls. Use
/// [`peer_identity`] once after connecting, then pass the result into
/// each call to avoid redundant syscalls.
///
/// NOTE: This requires to send and receive messages one at a time as in multi-threaded
/// contexts, this will ruin the message integrity as chunks can potentially be out of sync.
///
/// # Examples
///
/// ```no_run
/// use std::os::linux::net::SocketAddrExt;
/// use std::os::unix::net::{SocketAddr, UnixStream};
/// use onepassword_ipc_client::{send_with_stream, peer_identity};
///
/// let addr = SocketAddr::from_abstract_name("your_endpoint_name").unwrap();
/// let mut stream = UnixStream::connect_addr(&addr).unwrap();
/// let peer = peer_identity(&stream).unwrap();
///
/// let response1 = send_with_stream(&mut stream, peer, b"request one".to_vec()).unwrap();
/// let response2 = send_with_stream(&mut stream, peer, b"request two".to_vec()).unwrap();
/// ```
pub fn send_with_stream(
    stream: &mut UnixStream,
    peer: ProcessId,
    message: Vec<u8>,
) -> Result<IpcResponse, ErrorCode> {
    let data = send_and_receive(stream, message)?;
    Ok(IpcResponse {
        data,
        peer_identity: peer,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn chunked_round_trip_over_abstract_unix_socket() {
        let endpoint = format!("test_chunked_single_{}", std::process::id());
        let addr = SocketAddr::from_abstract_name(&endpoint).unwrap();

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .build()
            .unwrap();

        let std_listener = std::os::unix::net::UnixListener::bind_addr(&addr).unwrap();
        std_listener.set_nonblocking(true).unwrap();

        let server = thread::spawn(move || {
            rt.block_on(async {
                let listener = tokio::net::UnixListener::from_std(std_listener).unwrap();
                let (stream, _) = listener.accept().await.unwrap();
                crate::platform::stream_io::async_echo_server(stream, 1).await;
            });
        });

        let message = vec![0xAB; 100];
        let mut stream = UnixStream::connect_addr(&addr).unwrap();
        let peer = peer_identity(&stream).unwrap();
        let result = send_with_stream(&mut stream, peer, message.clone()).unwrap();
        assert_eq!(result.data, message);

        server.join().unwrap();
    }

    #[test]
    fn multiple_chunked_messages_over_same_connection() {
        let endpoint = format!("test_chunked_multi_{}", std::process::id());
        let addr = SocketAddr::from_abstract_name(&endpoint).unwrap();

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .build()
            .unwrap();

        let std_listener = std::os::unix::net::UnixListener::bind_addr(&addr).unwrap();
        std_listener.set_nonblocking(true).unwrap();

        let messages: Vec<Vec<u8>> = vec![vec![0x01; 50], vec![0x02; 100], vec![0x03; 200]];
        let expected = messages.clone();

        let server = thread::spawn(move || {
            rt.block_on(async {
                let listener = tokio::net::UnixListener::from_std(std_listener).unwrap();
                let (stream, _) = listener.accept().await.unwrap();
                crate::platform::stream_io::async_echo_server(stream, 3).await;
            });
        });

        let mut stream = UnixStream::connect_addr(&addr).unwrap();
        let peer = peer_identity(&stream).unwrap();
        for (msg, exp) in messages.into_iter().zip(expected.iter()) {
            let result = send_with_stream(&mut stream, peer, msg).unwrap();
            assert_eq!(&result.data, exp);
        }

        server.join().unwrap();
    }
}
