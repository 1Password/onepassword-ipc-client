# 1Password IPC Client 

## Overview

The `onepassword-ipc-client` crate provides a cross-platform client for communicating with integrations exposed by the 1Password desktop app over IPC. Communication uses the native transport on each platform:

- **macOS**: Mach ports
- **Linux**: Abstract Unix sockets
- **Windows**: Named pipes

The API follows a request-response pattern: the client sends a message (as raw bytes) to a named endpoint and receives a response. Large messages are automatically split into chunks and reassembled transparently.

## Transport

### One-Shot Connections

The simplest usage creates a fresh connection per call:

```rust
use onepassword_ipc_client::send_to;

let request: Vec<u8> = b"hello 1password".to_vec();
let response = send_to("your_endpoint_name", request).unwrap();
```

`send_to` is the top-level convenience function re-exported from the crate root. It connects, sends, receives, and disconnects in a single blocking call.

### Reusing Connections

Each platform exposes a lower-level function that accepts an existing connection handle, allowing multiple request-response exchanges over the same connection:

**macOS:**

```rust
use onepassword_ipc_client::send_with_client;
use mach_listener::Client;

let mut client = Client::connect("your_endpoint_name").unwrap();
let response1 = send_with_client(&mut client, request1).unwrap();
let response2 = send_with_client(&mut client, request2).unwrap();
```

**Linux:**

```rust
use onepassword_ipc_client::send_with_stream;
use std::os::unix::net::UnixStream;

let mut stream = /* connect to abstract socket */;
let response1 = send_with_stream(&mut stream, request1).unwrap();
let response2 = send_with_stream(&mut stream, request2).unwrap();
```

**Windows:**

```rust
use onepassword_ipc_client::send_with_pipe;
use std::fs::OpenOptions;

let mut pipe = OpenOptions::new()
    .read(true)
    .write(true)
    .open(r"\\.\pipe\your_endpoint_name")
    .unwrap();
let response1 = send_with_pipe(&mut pipe, request1).unwrap();
let response2 = send_with_pipe(&mut pipe, request2).unwrap();
```

Note: Windows and Linux have a limitation in multi-threaded environments with message integrity. Users must ensure they send and receive one at a time.

## Public API

### `send_to`

```rust
pub fn send_to(endpoint_name: &str, message: Vec<u8>) -> Result<Vec<u8>, ErrorCode>
```

Sends a message to the IPC server at the given endpoint and returns the response. This is a synchronous, blocking call that creates a fresh connection per invocation.

| Parameter | Type | Description |
| :-------- | :--- | :---------- |
| endpoint_name | `&str` | The endpoint to connect to. Interpreted as a Mach port name on macOS, an abstract Unix socket name on Linux, or a named pipe path on Windows. |
| message | `Vec<u8>` | The raw request bytes to send |

**Returns:** `Ok(Vec<u8>)` containing the response bytes, or `Err(ErrorCode)` on failure.

### `send_with_client` (macOS only)

```rust
pub fn send_with_client(client: &mut Client, request: Vec<u8>) -> Result<Vec<u8>, ErrorCode>
```

Sends a message over an existing Mach port client. Allows reusing the same connection across multiple calls.

### `send_with_stream` (Linux only)

```rust
pub fn send_with_stream(stream: &mut UnixStream, message: Vec<u8>) -> Result<Vec<u8>, ErrorCode>
```

Sends a message over an existing Unix stream. Allows reusing the same connection across multiple calls.

### `send_with_pipe` (Windows only)

```rust
pub fn send_with_pipe(pipe: &mut File, message: Vec<u8>) -> Result<Vec<u8>, ErrorCode>
```

Sends a message over an existing named pipe. Allows reusing the same connection across multiple calls.

## Error Handling

All platforms share a single `ErrorCode` enum, re-exported from the crate root as `onepassword_ipc_client::ErrorCode`.

| Value | Description |
| :---- | :---------- |
| `InvalidArguments` | Invalid arguments were provided (e.g. bad endpoint name). |
| `FailedToConnect` | Failed to connect to the IPC endpoint. |
| `FailedToSend` | Failed to send the message. |
| `FailedToReceive` | Failed to receive a response from the server. |
| `FailedToEncode` | Failed to encode the message into the wire format. |
| `FailedToDecode` | Failed to decode the response from the wire format. |
| `ServerClosedConnection` | The server closed the connection unexpectedly. |
| `Internal` | An internal error occurred. |

## Wire Protocol

### Framing (Linux / Windows)

Messages are framed using a length-delimited codec:

| Field | Size | Description |
| :---- | :--- | :---------- |
| Length prefix | 4 bytes, native endian | The byte length of the frame payload |
| Frame payload | Variable | The chunk data (header + payload) |

The maximum frame length is 1,048,576 bytes (1 MB).

### Chunking

All platforms use the same chunking protocol to split large messages into smaller pieces.

Each chunk has the following format:

| Field | Size | Description |
| :---- | :--- | :---------- |
| Header | 1 byte | `0x01` = last chunk, `0x02` = more chunks follow |
| Payload | 0–499,999 bytes | The chunk's data portion |

The maximum chunk size is 500,000 bytes (1-byte header + up to 499,999 bytes of payload).

**Chunking rules:**

- Messages ≤ 499,999 bytes produce a single chunk with header `0x01`.
- Larger messages are split into multiple chunks. All chunks except the last have header `0x02`; the last has header `0x01`.
- An empty message produces a single terminator chunk: `[0x01]`.

### Request-Response Flow

**Sending a request:**

1. The client splits the request into chunks using the chunking protocol.
2. Each chunk is wrapped in a length-delimited frame and sent over the connection.

**Receiving a response:**

1. The client reads length-delimited frames from the connection.
2. Each frame is parsed as a chunk. The payload bytes are concatenated.
3. When a chunk with header `0x01` (last) is received, the full response is assembled.

**macOS multi-chunk responses:**

On macOS, if the server's response spans multiple chunks, the client sends a dummy (empty) chunk after each intermediate response chunk to request the next one. On Linux and Windows, all response chunks are sent by the server without further prompting.

## Complete Client Example

```rust
use onepassword_ipc_client::{send_to, ErrorCode};

fn main() {
    let endpoint = "your_endpoint_name";
    let request = b"my request payload".to_vec();

    match send_to(endpoint, request) {
        Ok(response) => {
            println!("Got {} bytes back", response.len());
        }
        Err(err) => {
            eprintln!("IPC failed: {:?}", err);
        }
    }
}
```

### Connection Reuse Example (Linux)

```rust
use std::os::linux::net::SocketAddrExt;
use std::os::unix::net::{SocketAddr, UnixStream};
use onepassword_ipc_client::send_with_stream;

fn main() {
    let addr = SocketAddr::from_abstract_name("your_endpoint_name").unwrap();
    let mut stream = UnixStream::connect_addr(&addr).unwrap();

    let response1 = send_with_stream(&mut stream, b"request one".to_vec()).unwrap();
    let response2 = send_with_stream(&mut stream, b"request two".to_vec()).unwrap();

    println!("Response 1: {} bytes", response1.len());
    println!("Response 2: {} bytes", response2.len());
}
```
