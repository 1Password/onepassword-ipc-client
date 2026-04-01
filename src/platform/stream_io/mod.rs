use crate::ErrorCode;
use crate::chunking::{self, CHUNK_SIZE};
use std::io::ErrorKind;
use std::io::Read;
use std::io::Write;
use tokio_util::bytes::{Bytes, BytesMut};
use tokio_util::codec::{Decoder, Encoder, LengthDelimitedCodec};

#[cfg(test)]
mod tests;

const MESSAGE_SIZE_HINT: usize = 1024;

const MAX_FRAME_LENGTH: usize = 1_048_576; // 1 MB

struct ByteCodec(LengthDelimitedCodec);

impl Default for ByteCodec {
    fn default() -> Self {
        ByteCodec(
            LengthDelimitedCodec::builder()
                .native_endian()
                .max_frame_length(MAX_FRAME_LENGTH)
                .length_field_length(4)
                .length_field_offset(0)
                .new_codec(),
        )
    }
}

impl Encoder<Bytes> for ByteCodec {
    type Error = std::io::Error;

    fn encode(&mut self, item: Bytes, dst: &mut BytesMut) -> Result<(), Self::Error> {
        self.0.encode(item, dst)
    }
}

impl Decoder for ByteCodec {
    type Item = BytesMut;
    type Error = std::io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        self.0.decode(src)
    }
}

pub(super) fn send_and_receive<S>(mut stream: S, message: Vec<u8>) -> Result<Vec<u8>, ErrorCode>
where
    S: Read + Write,
{
    // Create separate codecs for encoding (sending) and decoding (receiving)
    // LengthDelimitedCodec maintains internal state, so we need separate instances
    let mut encode_codec = ByteCodec::default();
    let mut decode_codec = ByteCodec::default();

    // Split message into chunks and send chunks one by one
    let chunks = chunking::build_chunks(&message);
    for chunk in chunks {
        send(&mut stream, chunk, &mut encode_codec)?;
    }

    // Receive chunks until last one and combine them into the response
    // We maintain a persistent buffer across chunks to handle leftover data
    // from the LengthDelimitedCodec decode operation
    let mut response = Vec::new();
    let mut persistent_read_buf = BytesMut::with_capacity(MESSAGE_SIZE_HINT);

    loop {
        // Receive the chunk - this will block until we get data
        // We pass the persistent buffer so leftover data is preserved
        let chunk = receive_with_buffer(&mut stream, &mut decode_codec, &mut persistent_read_buf)?;

        let (is_last_chunk, chunk_data) =
            chunking::parse_chunk(&chunk).ok_or(ErrorCode::FailedToDecode)?;

        response.extend(chunk_data);

        if is_last_chunk {
            break;
        }
    }
    Ok(response)
}

fn send<S>(mut stream: S, message: Vec<u8>, codec: &mut ByteCodec) -> Result<(), ErrorCode>
where
    S: Write,
{
    // A buffer to hold the encoded frame
    let mut buf = BytesMut::with_capacity(CHUNK_SIZE);

    // Encode the message into the buffer
    codec
        .encode(message.into(), &mut buf)
        .map_err(|_| ErrorCode::FailedToEncode)?;

    // Write request
    stream
        .write_all(&buf)
        .map_err(|_| ErrorCode::FailedToSend)?;

    Ok(())
}

fn receive_with_buffer<S>(
    mut stream: S,
    codec: &mut ByteCodec,
    read_buf: &mut BytesMut,
) -> Result<Vec<u8>, ErrorCode>
where
    S: Read,
{
    let mut temp = [0u8; MESSAGE_SIZE_HINT];

    loop {
        let n = match stream.read(&mut temp) {
            Ok(0) => return Err(ErrorCode::ServerClosedConnection), // EOF
            Ok(n) => n,
            Err(ref e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(_) => return Err(ErrorCode::FailedToReceive),
        };

        read_buf.extend_from_slice(&temp[..n]);

        // Try to decode a complete frame
        // The codec will consume the frame from read_buf, leaving any leftover data for the next frame
        match codec.decode(read_buf) {
            Ok(Some(complete_frame)) => return Ok(complete_frame.to_vec()),
            Ok(None) => continue, // Incomplete frame, need more data
            Err(_) => return Err(ErrorCode::FailedToDecode),
        }
    }
}
