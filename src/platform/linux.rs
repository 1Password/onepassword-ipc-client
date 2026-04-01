use std::os::linux::net::SocketAddrExt;
use std::os::unix::net::SocketAddr;
use std::os::unix::net::UnixStream;

use super::stream_io::send_and_receive;
use crate::ErrorCode;

/// Sends a message to the IPC server at the given endpoint and returns the response.
///
/// Creates a fresh connection per call.
pub fn send_to(endpoint_name: &str, message: Vec<u8>) -> Result<Vec<u8>, ErrorCode> {
    let addr =
        SocketAddr::from_abstract_name(endpoint_name).map_err(|_| ErrorCode::InvalidArguments)?;
    let mut stream = UnixStream::connect_addr(&addr).map_err(|_| ErrorCode::FailedToConnect)?;
    send_with_stream(&mut stream, message)
}

/// Sends a message over an existing stream and returns the response.
///
/// This allows reusing the same connection across multiple calls.
///
/// NOTE: This requires to send and receive messages one at a time as in multi-threaded
/// contexts, this will ruin the message integrity as chunks can potentially be out of sync.
///
/// # Examples
///
/// ```no_run
/// use std::os::linux::net::SocketAddrExt;
/// use std::os::unix::net::{SocketAddr, UnixStream};
/// use onepassword_ipc_client::send_with_stream;
///
/// let addr = SocketAddr::from_abstract_name("your_endpoint_name").unwrap();
/// let mut stream = UnixStream::connect_addr(&addr).unwrap();
///
/// let response1 = send_with_stream(&mut stream, b"request one".to_vec()).unwrap();
/// let response2 = send_with_stream(&mut stream, b"request two".to_vec()).unwrap();
/// ```
pub fn send_with_stream(stream: &mut UnixStream, message: Vec<u8>) -> Result<Vec<u8>, ErrorCode> {
    send_and_receive(stream, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    use crate::chunking::{build_chunks, parse_chunk};
    use futures_util::{SinkExt, StreamExt};
    use tokio_util::bytes::Bytes;
    use tokio_util::codec::{Framed, LengthDelimitedCodec};

    async fn async_echo_server(stream: tokio::net::UnixStream, message_count: usize) {
        let codec = LengthDelimitedCodec::builder()
            .native_endian()
            .max_frame_length(1_048_576)
            .length_field_length(4)
            .length_field_offset(0)
            .new_codec();

        let mut framed = Framed::new(stream, codec);

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
                async_echo_server(stream, 1).await;
            });
        });

        let message = vec![0xAB; 100];
        let mut stream = UnixStream::connect_addr(&addr).unwrap();
        let result = send_with_stream(&mut stream, message.clone()).unwrap();
        assert_eq!(result, message);

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
                async_echo_server(stream, 3).await;
            });
        });

        let mut stream = UnixStream::connect_addr(&addr).unwrap();
        for (msg, exp) in messages.into_iter().zip(expected.iter()) {
            let result = send_with_stream(&mut stream, msg).unwrap();
            assert_eq!(&result, exp);
        }

        server.join().unwrap();
    }
}
