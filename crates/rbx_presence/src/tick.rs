//! How the relay's WebSocket loops read without blocking their sends: a read
//! gives up after one tick, the loop sends what queued meanwhile, and reads
//! again.
//!
//! Elsewhere that is a read timeout. Winsock leaves a socket whose read timed
//! out "in an indeterminate state" (the `SO_RCVTIMEO` docs), and a later call
//! on it can fail with `ERROR_IO_PENDING` (os error 997), which dropped play
//! connections mid-session. On Windows the socket is non-blocking instead and
//! the loop waits the tick out itself when nothing was there. A non-blocking
//! socket can also be full when a message is sent: tungstenite keeps it
//! queued, and [`flush`] on the next turn sends it on.

use std::io::{self, ErrorKind, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use tungstenite::{Error, Message, WebSocket};

/// Makes a read on `stream` come back within `tick`.
pub(crate) fn set(stream: &TcpStream, tick: Duration) -> io::Result<()> {
    if cfg!(windows) {
        stream.set_nonblocking(true)
    } else {
        stream.set_read_timeout(Some(tick))
    }
}

/// Whether a read's `error` only says nothing came within the tick, after
/// waiting the tick out where the read itself did not.
pub(crate) fn nothing_read(error: &Error, tick: Duration) -> bool {
    let nothing = matches!(error, Error::Io(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut));
    if nothing && cfg!(windows) {
        std::thread::sleep(tick);
    }
    nothing
}

/// Sends `message`; a full socket keeps it queued for [`flush`].
pub(crate) fn send<S: Read + Write>(ws: &mut WebSocket<S>, message: Message) -> Result<(), Error> {
    queued(ws.send(message))
}

/// Sends on whatever a full socket kept queued.
pub(crate) fn flush<S: Read + Write>(ws: &mut WebSocket<S>) -> Result<(), Error> {
    queued(ws.flush())
}

fn queued(result: Result<(), Error>) -> Result<(), Error> {
    match result {
        Err(Error::Io(e)) if e.kind() == ErrorKind::WouldBlock => Ok(()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_read_that_found_nothing_is_not_a_failure() {
        let tick = Duration::from_millis(1);
        for kind in [ErrorKind::WouldBlock, ErrorKind::TimedOut] {
            assert!(nothing_read(&Error::Io(kind.into()), tick));
        }
        assert!(!nothing_read(
            &Error::Io(ErrorKind::ConnectionReset.into()),
            tick
        ));
        assert!(!nothing_read(&Error::ConnectionClosed, tick));
    }

    #[test]
    fn a_full_socket_queues_rather_than_fails() {
        assert!(queued(Err(Error::Io(ErrorKind::WouldBlock.into()))).is_ok());
        assert!(queued(Err(Error::Io(ErrorKind::TimedOut.into()))).is_err());
        assert!(queued(Err(Error::ConnectionClosed)).is_err());
    }
}
