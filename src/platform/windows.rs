use std::fs::{File, OpenOptions};
use std::os::windows::io::AsRawHandle;

use super::stream_io::send_and_receive;
use crate::{ErrorCode, IpcResponse, ProcessId};

fn peer_identity(pipe: &File) -> Result<ProcessId, ErrorCode> {
    let mut pid: u32 = 0;
    // SAFETY: `as_raw_handle()` returns a valid handle for the open pipe,
    // and `&mut pid` is a valid pointer to a u32.
    let result = unsafe {
        windows_sys::Win32::System::Pipes::GetNamedPipeServerProcessId(
            pipe.as_raw_handle().cast(),
            &mut pid,
        )
    };
    if result != 0 {
        Ok(ProcessId::new(pid))
    } else {
        Err(ErrorCode::Internal)
    }
}

/// Sends a message to the IPC server at the given endpoint and returns the response.
///
/// Creates a fresh connection per call.
pub fn send_to(endpoint_name: &str, message: Vec<u8>) -> Result<IpcResponse, ErrorCode> {
    let mut pipe = OpenOptions::new()
        .read(true)
        .write(true)
        .open(endpoint_name)
        .map_err(|_| ErrorCode::FailedToConnect)?;
    send_with_pipe(&mut pipe, message)
}

/// Sends a message over an existing named pipe and returns the response.
///
/// This allows reusing the same connection across multiple calls.
///
/// NOTE: This requires to send and receive messages one at a time as in multi-threaded
/// contexts, this will ruin the message integrity as chunks can potentially be out of sync.
///
/// # Examples
///
/// ```no_run
/// use std::fs::OpenOptions;
/// use onepassword_ipc_client::send_with_pipe;
///
/// let mut pipe = OpenOptions::new()
///     .read(true)
///     .write(true)
///     .open(r"\\.\pipe\your_endpoint_name")
///     .unwrap();
///
/// let response1 = send_with_pipe(&mut pipe, b"request one".to_vec()).unwrap();
/// let response2 = send_with_pipe(&mut pipe, b"request two".to_vec()).unwrap();
/// ```
pub fn send_with_pipe(pipe: &mut File, message: Vec<u8>) -> Result<IpcResponse, ErrorCode> {
    let peer = peer_identity(pipe)?;
    let data = send_and_receive(pipe, message)?;
    Ok(IpcResponse {
        data,
        peer_identity: peer,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    use tokio::net::windows::named_pipe::ServerOptions;

    #[test]
    fn chunked_round_trip_over_named_pipe() {
        let pipe_name = format!(r"\\.\pipe\test_chunked_single_{}", std::process::id());

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .build()
            .unwrap();
        let server_pipe = {
            let _guard = rt.enter();
            ServerOptions::new()
                .first_pipe_instance(true)
                .create(&pipe_name)
                .unwrap()
        };

        let server = thread::spawn(move || {
            rt.block_on(async {
                server_pipe.connect().await.unwrap();
                crate::platform::stream_io::async_echo_server(server_pipe, 1).await;
            });
        });

        let message = vec![0xAB; 100];
        let mut pipe = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&pipe_name)
            .unwrap();
        let result = send_with_pipe(&mut pipe, message.clone()).unwrap();
        assert_eq!(result.data, message);

        server.join().unwrap();
    }

    #[test]
    fn multiple_chunked_messages_over_same_connection() {
        let pipe_name = format!(r"\\.\pipe\test_chunked_multi_{}", std::process::id());

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .build()
            .unwrap();
        let server_pipe = {
            let _guard = rt.enter();
            ServerOptions::new()
                .first_pipe_instance(true)
                .create(&pipe_name)
                .unwrap()
        };

        let messages: Vec<Vec<u8>> = vec![vec![0x01; 50], vec![0x02; 100], vec![0x03; 200]];
        let expected = messages.clone();

        let server = thread::spawn(move || {
            rt.block_on(async {
                server_pipe.connect().await.unwrap();
                crate::platform::stream_io::async_echo_server(server_pipe, 3).await;
            });
        });

        let mut pipe = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&pipe_name)
            .unwrap();
        for (msg, exp) in messages.into_iter().zip(expected.iter()) {
            let result = send_with_pipe(&mut pipe, msg).unwrap();
            assert_eq!(&result.data, exp);
        }

        server.join().unwrap();
    }
}
