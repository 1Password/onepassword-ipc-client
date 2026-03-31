use crate::ErrorCode;
use crate::chunking::{CHUNK_SIZE, build_chunks, parse_chunk};
use std::io::{self, Cursor, ErrorKind, Read, Write};
use tokio_util::bytes::BytesMut;
use tokio_util::codec::Encoder;

use super::*;

struct SocketLikeStream {
    read_cursor: Cursor<Vec<u8>>,
    written: Vec<u8>,
}

impl SocketLikeStream {
    fn new(preloaded_read_data: Vec<u8>) -> Self {
        Self {
            read_cursor: Cursor::new(preloaded_read_data),
            written: Vec::new(),
        }
    }

    fn written(&self) -> &[u8] {
        &self.written
    }
}

impl Read for SocketLikeStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.read_cursor.read(buf)
    }
}

impl Write for SocketLikeStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.written.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Helper: encode a list of chunks as frames using ByteCodec,
/// ready to be read by `receive_with_buffer()`.
fn encode_chunks_as_frames(chunks: &[Vec<u8>]) -> Vec<u8> {
    let mut codec = ByteCodec::default();
    let mut buf = BytesMut::new();

    for chunk in chunks {
        codec.encode(chunk.clone().into(), &mut buf).unwrap();
    }

    buf.to_vec()
}

#[test]
fn send_and_receive_round_trip_small_message() {
    // Client will send "ping", server will respond with "pong".
    let request = b"ping".to_vec();
    let response_body = b"pong".to_vec();

    // Build a single logical response message, chunked and encoded as frames.
    let response_chunks = build_chunks(&response_body);
    let encoded_response = encode_chunks_as_frames(&response_chunks);

    // The in-memory stream starts with the server response already "in the pipe".
    let stream = SocketLikeStream::new(encoded_response);

    let result = send_and_receive(stream, request).unwrap();
    assert_eq!(result, response_body);
}

#[test]
fn send_and_receive_round_trip_large_message() {
    // Larger message to ensure multiple chunks internally (depending on build_chunks).
    let request = vec![1u8; 256 * 1024 + 42]; // Not a multiple of chunk size.
    let response_body = vec![2u8; 512 * 1024 + 123]; // Not a multiple of chunk size.

    let response_chunks = build_chunks(&response_body);
    let encoded_response = encode_chunks_as_frames(&response_chunks);
    let encoded_request = encode_chunks_as_frames(build_chunks(&request).as_slice());

    let mut stream = SocketLikeStream::new(encoded_response);

    let result = send_and_receive(&mut stream, request).unwrap();
    assert_eq!(result, response_body);
    assert_eq!(stream.written(), &encoded_request[..]);
}

/// A stream where all writes fail.
struct FailingWriteStream;

impl Read for FailingWriteStream {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        Ok(0) // EOF; shouldn't matter since we fail on write
    }
}

impl Write for FailingWriteStream {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("write failed"))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn send_and_receive_write_error_maps_to_failed_to_send() {
    let stream = FailingWriteStream;
    let result = send_and_receive(stream, b"hello".to_vec());
    assert_eq!(result.unwrap_err(), ErrorCode::FailedToSend);
}

/// A stream that returns EOF on first read.
struct EofReadStream;

impl Read for EofReadStream {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        Ok(0)
    }
}

impl Write for EofReadStream {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        Ok(0)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn receive_eof_returns_server_closed_connection() {
    let mut codec = ByteCodec::default();
    let stream = EofReadStream;
    let mut read_buf = BytesMut::with_capacity(CHUNK_SIZE);

    let result = receive_with_buffer(stream, &mut codec, &mut read_buf);
    assert_eq!(result.unwrap_err(), ErrorCode::ServerClosedConnection);
}

/// A stream that returns an arbitrary error on read.
struct ErrorReadStream;

impl Read for ErrorReadStream {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("bad read"))
    }
}

impl Write for ErrorReadStream {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        Ok(0)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn receive_read_error_maps_to_failed_to_receive() {
    let mut codec = ByteCodec::default();
    let stream = ErrorReadStream;
    let mut read_buf = BytesMut::with_capacity(CHUNK_SIZE);

    let result = receive_with_buffer(stream, &mut codec, &mut read_buf);
    assert_eq!(result.unwrap_err(), ErrorCode::FailedToReceive);
}

/// A stream that first reports Interrupted, then returns valid data.
struct InterruptedThenOkStream {
    state: u8,
    data: Vec<u8>,
}

impl InterruptedThenOkStream {
    fn new(data: Vec<u8>) -> Self {
        Self { state: 0, data }
    }
}

impl Read for InterruptedThenOkStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.state == 0 {
            self.state = 1;
            return Err(io::Error::new(ErrorKind::Interrupted, "interrupted"));
        }
        let mut s = Cursor::new(self.data.clone());
        s.read(buf)
    }
}

impl Write for InterruptedThenOkStream {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        Ok(0)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn receive_retries_on_interrupted_and_then_succeeds() {
    let message_body = b"hello interrupted".to_vec();
    let chunks = build_chunks(&message_body);
    let encoded = encode_chunks_as_frames(&chunks);
    let mut read_buf = BytesMut::with_capacity(CHUNK_SIZE);

    let mut codec = ByteCodec::default();
    let stream = InterruptedThenOkStream::new(encoded);

    let result = receive_with_buffer(stream, &mut codec, &mut read_buf).unwrap();
    // result is a single chunk (the encoded message frame),
    // which should be parseable via parse_chunk.
    let (_, parsed) = parse_chunk(&result).unwrap();
    assert_eq!(parsed, message_body);
}
