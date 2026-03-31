use super::{
    BYTE_MORE_CHUNKS, BYTE_NO_MORE_CHUNKS, HEADER_SIZE, PAYLOAD_SIZE, build_chunks, parse_chunk,
};

/// Helper: assert headers and content roundtrip back to payload.
fn assert_headers_and_roundtrip(payload: &[u8], chunks: &[Vec<u8>]) {
    // Rebuild the payload from chunk bodies (skip header at index 0)
    let rebuilt: Vec<u8> = chunks.iter().flat_map(|c| c[1..].iter().copied()).collect();
    assert_eq!(rebuilt, payload, "payload roundtrip mismatch");

    // Header expectations:
    if !chunks.is_empty() {
        // All but the last must be BYTE_MORE_CHUNKS
        for c in &chunks[..chunks.len() - 1] {
            assert_eq!(c[0], BYTE_MORE_CHUNKS, "non-last chunk has wrong header");
        }
        // Last must be BYTE_NO_MORE_CHUNKS
        assert_eq!(
            chunks.last().unwrap()[0],
            BYTE_NO_MORE_CHUNKS,
            "last chunk has wrong header"
        );
    }
}

/// Helper: assert chunk lengths (including header) are as expected.
fn assert_chunk_lengths(payload_len: usize, chunks: &[Vec<u8>]) {
    if chunks.is_empty() {
        assert_eq!(payload_len, 0);
        return;
    }

    // Expected number of chunks
    let total = payload_len.div_ceil(PAYLOAD_SIZE).max(1);
    assert_eq!(chunks.len(), total, "unexpected chunk count");

    // For every chunk except the last: length must be 1 (header) + PAYLOAD_SIZE
    for c in &chunks[..chunks.len().saturating_sub(1)] {
        assert_eq!(
            c.len(),
            1 + PAYLOAD_SIZE,
            "non-last chunk length should be 1 + PAYLOAD_SIZE"
        );
    }

    // Last chunk length:
    let rem = payload_len % PAYLOAD_SIZE;
    let expected_last = 1 + if rem == 0 {
        if chunks.len() == 1 { rem } else { PAYLOAD_SIZE }
    } else {
        rem
    };
    assert_eq!(
        chunks.last().unwrap().len(),
        expected_last,
        "last chunk length mismatch"
    );
}

#[test]
fn build_chunks_empty_payload() {
    let payload: Vec<u8> = Vec::new();
    let chunks = build_chunks(&payload);

    assert!(!chunks.is_empty(), "empty payload should yield 1 chunk");
    assert_eq!(
        chunks[0][0], BYTE_NO_MORE_CHUNKS,
        "single chunk must be marked as last"
    );
    // Body matches payload
    assert_eq!(&chunks[0][1..], &payload[..], "chunk body mismatch");
    assert_headers_and_roundtrip(&payload, &chunks);
    assert_chunk_lengths(payload.len(), &chunks);
}

#[test]
fn build_chunks_single_short_chunk() {
    // shorter than PAYLOAD_SIZE to ensure a single partial chunk
    let len = PAYLOAD_SIZE.saturating_sub(1).max(1);
    let payload: Vec<u8> = (0..len).map(|x| (x % 256) as u8).collect();

    let chunks = build_chunks(&payload);

    // One chunk only
    assert_eq!(chunks.len(), 1, "should produce exactly one chunk");
    // Header must be "no more"
    assert_eq!(
        chunks[0][0], BYTE_NO_MORE_CHUNKS,
        "single chunk must be marked as last"
    );
    // Body matches payload
    assert_eq!(&chunks[0][1..], &payload[..], "chunk body mismatch");

    assert_headers_and_roundtrip(&payload, &chunks);
    assert_chunk_lengths(payload.len(), &chunks);
}

#[test]
fn build_chunks_exact_multiple_of_payload_size() {
    // Build payload with exact multiple of PAYLOAD_SIZE
    let payload_len = PAYLOAD_SIZE * 3; // 3 full chunks
    let payload: Vec<u8> = (0..payload_len).map(|x| (x % 256) as u8).collect();

    let chunks = build_chunks(&payload);

    // Count
    assert_eq!(chunks.len(), 3, "expected three chunks");

    // Headers: first two are MORE, last is NO_MORE
    assert_eq!(chunks[0][0], BYTE_MORE_CHUNKS);
    assert_eq!(chunks[1][0], BYTE_MORE_CHUNKS);
    assert_eq!(chunks[2][0], BYTE_NO_MORE_CHUNKS);

    // Each non-last chunk body is exactly PAYLOAD_SIZE; last too (exact multiple)
    for (i, c) in chunks.iter().enumerate() {
        let body = &c[1..];
        assert_eq!(
            body.len(),
            PAYLOAD_SIZE,
            "chunk {i} body length should equal PAYLOAD_SIZE"
        );
    }

    assert_headers_and_roundtrip(&payload, &chunks);
    assert_chunk_lengths(payload.len(), &chunks);
}

#[test]
fn build_chunks_multiple_with_remainder() {
    // 2 full chunks + remainder
    let payload_len = (PAYLOAD_SIZE * 2) + (PAYLOAD_SIZE / 2).max(1);
    let payload: Vec<u8> = (0..payload_len).map(|x| (x % 256) as u8).collect();

    let chunks = build_chunks(&payload);

    // Count
    let expected = payload_len.div_ceil(PAYLOAD_SIZE);
    assert_eq!(chunks.len(), expected);

    // Headers: all but last MORE, last NO_MORE
    for c in &chunks[..chunks.len().saturating_sub(1)] {
        assert_eq!(c[0], BYTE_MORE_CHUNKS);
    }
    assert_eq!(chunks.last().unwrap()[0], BYTE_NO_MORE_CHUNKS);

    // Last body length matches remainder
    let rem = payload_len % PAYLOAD_SIZE;
    let expected_last_body = if rem == 0 { PAYLOAD_SIZE } else { rem };
    assert_eq!(chunks.last().unwrap()[1..].len(), expected_last_body);

    assert_headers_and_roundtrip(&payload, &chunks);
    assert_chunk_lengths(payload.len(), &chunks);
}

/// Build a message: first header byte, then (HEADER_SIZE-1) padding bytes, then body.
fn make_message(header: u8, body: &[u8]) -> Vec<u8> {
    let mut m = Vec::with_capacity(HEADER_SIZE + body.len());
    m.push(header);
    if HEADER_SIZE > 1 {
        m.extend(std::iter::repeat_n(0u8, HEADER_SIZE - 1));
    }
    m.extend_from_slice(body);
    m
}

#[test]
fn parse_chunk_last_with_body() {
    let body = vec![1, 2, 3, 4];
    let msg = make_message(BYTE_NO_MORE_CHUNKS, &body);

    let (is_last, payload) = parse_chunk(&msg).expect("expected to successfully parse chunk");

    assert!(is_last, "expected last-chunk flag");
    assert_eq!(payload, body, "payload mismatch");
}

#[test]
fn parse_chunk_non_last_with_body() {
    let body = vec![9, 8];
    let msg = make_message(BYTE_MORE_CHUNKS, &body);

    let (is_last, payload) = parse_chunk(&msg).expect("expected to successfully parse chunk");

    assert!(!is_last, "expected non-last-chunk flag");
    assert_eq!(payload, body);
}

#[test]
fn parse_chunk_last_empty_body() {
    let msg = make_message(BYTE_NO_MORE_CHUNKS, &[]);

    let (is_last, payload) = parse_chunk(&msg).expect("expected to successfully parse chunk");

    assert!(is_last);
    assert!(payload.is_empty(), "expected empty payload for empty body");
}

#[test]
fn parse_chunk_return_none_on_too_short_message() {
    // Construct a message shorter than HEADER_SIZE.
    let too_short = if HEADER_SIZE == 0 {
        // If someone set it to 0 (shouldn't happen), still ensure returning None by using empty vec.
        Vec::<u8>::new()
    } else {
        vec![0u8; HEADER_SIZE - 1]
    };

    // This should return None
    assert!(parse_chunk(&too_short).is_none());
}

// Optional: verifies that padding bytes (when HEADER_SIZE > 1) are ignored for payload.
#[test]
fn parse_chunk_ignores_header_padding() {
    let body = vec![0xAA, 0xBB, 0xCC];
    let msg = make_message(BYTE_MORE_CHUNKS, &body);

    let Some((_is_last, payload)) = parse_chunk(&msg) else {
        panic!("expected to successfully parse chunk");
    };

    assert_eq!(payload, body);
}
