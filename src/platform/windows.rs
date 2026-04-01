use std::fs::{File, OpenOptions};

use super::stream_io::send_and_receive;
use crate::ErrorCode;

/// Sends a message to the IPC server at the given endpoint and returns the response.
///
/// Creates a fresh connection per call.
pub fn send_to(endpoint_name: &str, message: Vec<u8>) -> Result<Vec<u8>, ErrorCode> {
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
pub fn send_with_pipe(pipe: &mut File, message: Vec<u8>) -> Result<Vec<u8>, ErrorCode> {
    send_and_receive(pipe, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    use crate::chunking::{build_chunks, parse_chunk};
    use futures_util::{SinkExt, StreamExt};
    use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
    use tokio_util::bytes::Bytes;
    use tokio_util::codec::{Framed, LengthDelimitedCodec};

    /// Async echo server that receives N messages and sends each back.
    /// Uses tokio's async I/O directly on the NamedPipeServer, avoiding
    /// conversion to a sync File (which breaks on overlapped handles).
    async fn async_echo_server(pipe: NamedPipeServer, message_count: usize) {
        let codec = LengthDelimitedCodec::builder()
            .native_endian()
            .max_frame_length(1_048_576)
            .length_field_length(4)
            .length_field_offset(0)
            .new_codec();

        let mut framed = Framed::new(pipe, codec);

        for _ in 0..message_count {
            let mut message = Vec::new();
            loop {
                let frame = framed.next().await.unwrap().unwrap();
                let (is_last, payload) = parse_chunk(&frame).unwrap();
                message.extend(payload);
                if is_last {
                    break;
                }
            }

            let chunks = build_chunks(&message);
            for chunk in chunks {
                framed.send(Bytes::from(chunk)).await.unwrap();
            }
        }
    }

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
                async_echo_server(server_pipe, 1).await;
            });
        });

        let message = vec![0xAB; 100];
        let mut pipe = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&pipe_name)
            .unwrap();
        let result = send_with_pipe(&mut pipe, message.clone()).unwrap();
        assert_eq!(result, message);

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
                async_echo_server(server_pipe, 3).await;
            });
        });

        let mut pipe = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&pipe_name)
            .unwrap();
        for (msg, exp) in messages.into_iter().zip(expected.iter()) {
            let result = send_with_pipe(&mut pipe, msg).unwrap();
            assert_eq!(&result, exp);
        }

        server.join().unwrap();
    }
}
