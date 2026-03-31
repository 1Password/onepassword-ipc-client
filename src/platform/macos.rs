use std::time::Duration;

use crate::ErrorCode;
use crate::chunking::{build_chunks, build_dummy_chunk, parse_chunk};
use mach_listener::{Client, NewMessage};

const SEND_TIMEOUT_SECS: u64 = 5;

/// Sends a message to the IPC server at the given endpoint and returns the response.
///
/// Creates a fresh connection per call.
pub fn send_to(endpoint_name: &str, request: Vec<u8>) -> Result<Vec<u8>, ErrorCode> {
    let mut client = Client::connect(endpoint_name).map_err(|e| -> ErrorCode { e.into() })?;
    client.set_send_timeout(Some(Duration::from_secs(SEND_TIMEOUT_SECS)));
    send_with_client(&mut client, request)
}

pub fn send_with_client(client: &mut Client, request: Vec<u8>) -> Result<Vec<u8>, ErrorCode> {
    let chunks = build_chunks(&request);
    let mut iter = chunks.into_iter().peekable();

    let mut last_response: Option<NewMessage<Client>> = None;
    while let Some(chunk) = iter.next() {
        let response = client
            .send_with_reply(&chunk)
            .map_err(|e| -> ErrorCode { e.into() })?;
        if iter.peek().is_some() && !response.data.is_empty() {
            return Err(ErrorCode::Internal);
        }
        last_response = Some(response);
    }

    let Some(last_response) = last_response else {
        return Err(ErrorCode::Internal);
    };

    let (is_last_chunk, payload) = parse_chunk(&last_response.data).ok_or(ErrorCode::Internal)?;
    let mut full_response = payload;
    let mut is_response_last_chunk = is_last_chunk;

    while !is_response_last_chunk {
        let response = client
            .send_with_reply(&build_dummy_chunk())
            .map_err(|e| -> ErrorCode { e.into() })?;
        let (is_last_chunk, mut payload) =
            parse_chunk(&response.data).ok_or(ErrorCode::Internal)?;
        full_response.append(&mut payload);
        is_response_last_chunk = is_last_chunk
    }

    Ok(full_response)
}

impl From<mach_listener::Error> for ErrorCode {
    fn from(error: mach_listener::Error) -> Self {
        match error {
            mach_listener::Error::OsError(_) => ErrorCode::Internal,
            mach_listener::Error::RegistrationError { .. } => ErrorCode::FailedToConnect,
            mach_listener::Error::CorruptMessage => ErrorCode::FailedToDecode,
            mach_listener::Error::MessageTooLarge => ErrorCode::FailedToSend,
            mach_listener::Error::FailedToSend => ErrorCode::FailedToSend,
            mach_listener::Error::NoReply => ErrorCode::FailedToReceive,
        }
    }
}
