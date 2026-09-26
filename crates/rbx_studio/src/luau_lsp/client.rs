//! JSON-RPC over the server's stdio: requests matched to their responses by
//! id on one reader thread, which also answers the few requests the server
//! sends the other way.

use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use serde_json::{json, Value};

use super::wire;

type Reply = Result<Value, String>;
type Pending = Arc<Mutex<HashMap<u64, Sender<Reply>>>>;
type Writer = Arc<Mutex<Box<dyn Write + Send>>>;

pub(crate) struct Client {
    writer: Writer,
    pending: Pending,
    next_id: AtomicU64,
    child: Option<Child>,
}

impl Client {
    pub(crate) fn spawn(mut command: Command) -> io::Result<Client> {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            return Err(io::Error::other("the server's stdio was not piped"));
        };
        let mut client = Client::over(BufReader::new(stdout), stdin);
        client.child = Some(child);
        Ok(client)
    }

    fn over(reader: impl BufRead + Send + 'static, writer: impl Write + Send + 'static) -> Client {
        let writer: Writer = Arc::new(Mutex::new(Box::new(writer)));
        let pending = Pending::default();
        let (thread_writer, thread_pending) = (writer.clone(), pending.clone());
        thread::spawn(move || read_loop(reader, &thread_writer, &thread_pending));
        Client {
            writer,
            pending,
            next_id: AtomicU64::new(1),
            child: None,
        }
    }

    /// Sends a request; its reply arrives on the receiver. The receiver
    /// disconnects without a reply if the server exits first.
    pub(crate) fn request(&self, method: &str, params: Value) -> Receiver<Reply> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = mpsc::channel();
        lock(&self.pending).insert(id, sender);
        let message = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        if send(&self.writer, &message).is_err() {
            lock(&self.pending).remove(&id);
        }
        receiver
    }

    pub(crate) fn notify(&self, method: &str, params: Value) {
        // A dead server has nothing to be told; its reader thread has already
        // disconnected every waiting request, which is where that shows.
        let _ = send(
            &self.writer,
            &json!({"jsonrpc": "2.0", "method": method, "params": params}),
        );
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        // `shutdown` asks for a reply this is not going to wait for; `exit`
        // right behind it is what actually ends the process, and the kill
        // covers a server too wedged to read either.
        let _ = self.request("shutdown", Value::Null);
        self.notify("exit", Value::Null);
        let _ = child.kill();
        let _ = child.wait();
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn send(writer: &Writer, message: &Value) -> io::Result<()> {
    wire::write(&mut *lock(writer), message)
}

fn read_loop(mut reader: impl BufRead, writer: &Writer, pending: &Pending) {
    while let Ok(Some(message)) = wire::read(&mut reader) {
        let id = message.get("id").cloned();
        match (id, message.get("method")) {
            // A server-to-client request (`client/registerCapability`,
            // `window/workDoneProgress/create`): every one this client can
            // receive is satisfied by a plain null result, and an unanswered
            // one can stall the server.
            (Some(id), Some(_)) => {
                let _ = send(writer, &json!({"jsonrpc": "2.0", "id": id, "result": null}));
            }
            (Some(id), None) => {
                let Some(sender) = id.as_u64().and_then(|id| lock(pending).remove(&id)) else {
                    continue;
                };
                let reply = match message.get("error") {
                    Some(error) => Err(error["message"].as_str().unwrap_or("error").to_owned()),
                    None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
                };
                let _ = sender.send(reply);
            }
            // Notifications: diagnostics are pulled rather than pushed (see
            // `shell::luau_lsp`), and the log is only for a human at a terminal.
            (None, _) => {}
        }
    }
    lock(pending).clear();
}

#[cfg(test)]
mod tests {
    use std::io::{BufReader, PipeReader, PipeWriter};
    use std::thread;

    use serde_json::{json, Value};

    use super::{wire, Client};

    /// A client wired to a fake server running `serve` on its own thread.
    fn with_server(
        serve: impl FnOnce(BufReader<PipeReader>, PipeWriter) + Send + 'static,
    ) -> Client {
        let (client_in, server_out) = std::io::pipe().unwrap();
        let (server_in, client_out) = std::io::pipe().unwrap();
        thread::spawn(move || serve(BufReader::new(server_in), server_out));
        Client::over(BufReader::new(client_in), client_out)
    }

    #[test]
    fn matches_replies_to_requests_by_id_in_any_order() {
        let client = with_server(|mut input, mut output| {
            let first = wire::read(&mut input).unwrap().unwrap();
            let second = wire::read(&mut input).unwrap().unwrap();
            for request in [second, first] {
                let reply =
                    json!({"jsonrpc": "2.0", "id": request["id"], "result": request["method"]});
                wire::write(&mut output, &reply).unwrap();
            }
        });
        let a = client.request("a", Value::Null);
        let b = client.request("b", Value::Null);
        assert_eq!(a.recv().unwrap(), Ok(json!("a")));
        assert_eq!(b.recv().unwrap(), Ok(json!("b")));
    }

    #[test]
    fn answers_server_requests_and_reports_errors() {
        let client = with_server(|mut input, mut output| {
            let request = wire::read(&mut input).unwrap().unwrap();
            let asks =
                json!({"jsonrpc": "2.0", "id": "reg", "method": "client/registerCapability"});
            wire::write(&mut output, &asks).unwrap();
            let answer = wire::read(&mut input).unwrap().unwrap();
            assert_eq!(answer["id"], "reg");
            assert_eq!(answer["result"], Value::Null);
            let error = json!({"jsonrpc": "2.0", "id": request["id"], "error": {"code": 1, "message": "no"}});
            wire::write(&mut output, &error).unwrap();
        });
        assert_eq!(
            client.request("x", Value::Null).recv().unwrap(),
            Err("no".into())
        );
    }

    #[test]
    fn a_server_that_exits_disconnects_what_is_still_waiting() {
        let client = with_server(|mut input, _output| {
            let _ = wire::read(&mut input);
        });
        assert!(client.request("x", Value::Null).recv().is_err());
    }
}
