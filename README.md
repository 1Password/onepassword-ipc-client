# 1Password IPC Client

## Overview

The `onepassword-ipc-client` crate provides a cross-platform client for communicating with integrations exposed by the 1Password desktop app over IPC. Communication uses the native transport on each platform:

- **macOS**: Mach ports
- **Linux**: Abstract Unix sockets
- **Windows**: Named pipes

The API follows a request-response pattern: the client sends a message (as raw bytes) to a named endpoint and receives a response. Large messages are automatically split into chunks and reassembled transparently.

*By accessing or using 1Password Developer Tools, you agree to the [API and SDK Terms of Service](https://1password.com/legal/api-sdk-terms-of-service).*

### Availability and Usage

Availability of supported integrations in the 1Password desktop app may at any time be constrained by one or more of: feature flag-based rollouts,
protected in-app user settings, app release channels (ie "production" vs "nightly"), or the connecting process' "platform identity".

In addition, 1Password heavily utilizes IPC across platforms to support product features, such as:
- [Integrating the browser extension and desktop app](https://support.1password.com/connect-1password-browser-app/)
- [Integrating the `op` CLI and desktop app](https://developer.1password.com/docs/cli/get-started/#step-2-turn-on-the-1password-desktop-app-integration)
- [Providing easy authentication](https://developer.1password.com/docs/sdks/concepts#1password-desktop-app) from [1Password's SDK](https://github.com/1Password/onepassword-sdk-python)

These features, and others, use undocumented IPC endpoints which may change their structure or behavior at any time. No support for these will be provided.
This library's functionality should only be used with documented integration points which perscribe use of the library as the expected entrypoint.

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

## Credits

Made with ❤️ and ☕ by the [1Password](https://1password.com/) team.

### License

<sup>
Licensed under either of <a href="LICENSE-APACHE">Apache License, Version
2.0</a> or <a href="LICENSE-MIT">MIT license</a> at your option.
</sup>

<br>

<sub>
Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this crate by you, as defined in the Apache-2.0 license, shall
be dual licensed as above, without any additional terms or conditions.
</sub>
