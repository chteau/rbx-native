//! LSP's base protocol: a `Content-Length` header block, a blank line, then
//! exactly that many bytes of JSON. Nothing else in the header matters to
//! this client — `luau-lsp` only ever sends `Content-Length`, and the spec's
//! one other header (`Content-Type`) has a single legal value.

use std::io::{self, BufRead, Write};

use serde_json::Value;

pub(crate) fn write(out: &mut impl Write, message: &Value) -> io::Result<()> {
    let body = message.to_string();
    write!(out, "Content-Length: {}\r\n\r\n{body}", body.len())?;
    out.flush()
}

/// The next message, or `None` once the stream has ended cleanly between
/// messages — the server exiting.
pub(crate) fn read(input: &mut impl BufRead) -> io::Result<Option<Value>> {
    let mut length = None;
    let mut line = String::new();
    loop {
        line.clear();
        if input.read_line(&mut line)? == 0 {
            return match length {
                None => Ok(None),
                Some(_) => Err(io::ErrorKind::UnexpectedEof.into()),
            };
        }
        let header = line.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                length = value.trim().parse::<usize>().ok();
            }
        }
    }
    let Some(length) = length else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "LSP message without a Content-Length",
        ));
    };
    let mut body = vec![0; length];
    input.read_exact(&mut body)?;
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{read, write};

    #[test]
    fn round_trips_several_messages_back_to_back() {
        let mut buffer = Vec::new();
        let first = json!({"jsonrpc": "2.0", "id": 1, "result": "héllo"});
        let second = json!({"jsonrpc": "2.0", "method": "exit"});
        write(&mut buffer, &first).unwrap();
        write(&mut buffer, &second).unwrap();

        let mut input = buffer.as_slice();
        assert_eq!(read(&mut input).unwrap(), Some(first));
        assert_eq!(read(&mut input).unwrap(), Some(second));
        assert_eq!(read(&mut input).unwrap(), None);
    }

    #[test]
    fn length_counts_bytes_not_characters() {
        let mut buffer = Vec::new();
        write(&mut buffer, &json!("é")).unwrap();
        assert!(buffer.starts_with(b"Content-Length: 4\r\n\r\n"));
    }

    #[test]
    fn ignores_other_headers_and_their_case() {
        let raw = b"content-length: 2\r\nContent-Type: application/vscode-jsonrpc\r\n\r\n{}";
        assert_eq!(read(&mut &raw[..]).unwrap(), Some(json!({})));
    }

    #[test]
    fn a_stream_cut_mid_message_is_an_error() {
        assert!(read(&mut &b"Content-Length: 10\r\n\r\n{}"[..]).is_err());
        assert!(read(&mut &b"Content-Length: 10\r\n"[..]).is_err());
        assert!(read(&mut &b"X: 1\r\n\r\n{}"[..]).is_err());
    }
}
