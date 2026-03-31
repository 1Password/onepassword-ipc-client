// Message Types
/// Bit representing that this was the last chunk of the message
const BYTE_NO_MORE_CHUNKS: u8 = 0x01;
/// Bit representing that more chunks are coming
const BYTE_MORE_CHUNKS: u8 = 0x02;

/// Max size of the data *payload* (leaves room for header)
pub const CHUNK_SIZE: usize = 500_000; // ~512k limit, play it safe

/// 1 byte telling if we more chunks are coming or not
const HEADER_SIZE: usize = 1;
/// Actual size of the payload inside a chunk
const PAYLOAD_SIZE: usize = CHUNK_SIZE - HEADER_SIZE;

#[cfg(test)]
mod tests;

/// Splits the payload into multiple chunks
pub fn build_chunks(payload: &[u8]) -> Vec<Vec<u8>> {
    use itertools::{Itertools, Position};

    if payload.is_empty() {
        // Empty payload => single terminator chunk
        return vec![vec![BYTE_NO_MORE_CHUNKS]];
    }

    payload
        .chunks(PAYLOAD_SIZE)
        .with_position()
        .map(|(pos, chunk)| {
            let header = match pos {
                Position::Only | Position::Last => BYTE_NO_MORE_CHUNKS,
                _ => BYTE_MORE_CHUNKS,
            };
            let mut v = Vec::with_capacity(CHUNK_SIZE);
            v.push(header);
            v.extend_from_slice(chunk);
            v
        })
        .collect()
}

/// Parse the received chunk
pub fn parse_chunk(message: &[u8]) -> Option<(bool, Vec<u8>)> {
    let last_chunk_marker = message.first()?; // assumes HEADER_SIZE == 1
    let payload = Vec::from(message.get(HEADER_SIZE..)?);
    Some((last_chunk_marker == &BYTE_NO_MORE_CHUNKS, payload))
}

/// Send an empty chunk to trigger sending the next chunk
pub fn build_dummy_chunk() -> Vec<u8> {
    Vec::new()
}
