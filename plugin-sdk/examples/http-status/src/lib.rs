//! An HTTP call through the SDK: `:http-status example.com` connects (port 80), sends a `HEAD /` written by
//! `tarae_plugin::http::request`, reads the response as its bytes come (`http::Response`) and says its status. A
//! plugin has no sockets: tarae connects (`network`, each new host asked on a card) and hands the bytes over.
use std::sync::Mutex;

use tarae_plugin::http::{Response, request};
use tarae_plugin::{Action, Command, Connect, Context, Event, Guest, HostEvent, Now, Permission, Subscription};

struct Plugin;

const CONN: u64 = 1;

/// The host asked, and the response so far.
static CALL: Mutex<Option<(String, Response)>> = Mutex::new(None);

impl Guest for Plugin {
    fn commands() -> Vec<Command> {
        vec![Command { name: "http-status".into(), doc: "A host's HTTP status: :http-status <host>".into(), text: false }]
    }

    fn permissions() -> Vec<Permission> {
        vec![Permission::Network]
    }

    fn run(_: String, cx: Context) -> Vec<Action> {
        let host = cx.args.trim().to_string();
        if host.is_empty() {
            return vec![Action::Error("which host? :http-status example.com".into())];
        }
        *CALL.lock().unwrap() = Some((host.clone(), Response::new()));
        vec![Action::Connect(Connect { id: CONN, host, port: 80 })]
    }

    fn on_events(events: Vec<HostEvent>, _: Now) -> Vec<Action> {
        let mut call = CALL.lock().unwrap();
        let Some((host, response)) = call.as_mut() else { return Vec::new() };
        let mut out = Vec::new();
        for e in events {
            match e {
                HostEvent::Connected(CONN) => {
                    let head = request("HEAD", "/", host, &[("Connection", "close")], &[]);
                    out.push(Action::Send((CONN, head)));
                }
                HostEvent::Data((CONN, bytes)) => {
                    if let Err(e) = response.feed(&bytes) {
                        out.push(Action::Error(format!("{host}: {e}")));
                    }
                    if let Some(head) = response.head() {
                        out.push(Action::Status(format!("{host}: HTTP {}", head.status)));
                        out.push(Action::Stop(CONN));
                    }
                }
                HostEvent::Failed((CONN, why)) | HostEvent::Closed((CONN, why)) if response.head().is_none() => {
                    out.push(Action::Warning(format!("{host}: no answer ({why})")));
                }
                _ => {}
            }
        }
        out
    }

    fn subscriptions() -> Vec<Subscription> {
        Vec::new()
    }

    fn on(_: Event, _: Context) -> Vec<Action> {
        Vec::new()
    }
}

tarae_plugin::export!(Plugin);

#[cfg(test)]
mod tests {
    use super::*;
    use tarae_plugin::test::{Host, Project, Replies};

    /// The host's status, from a scripted reply to the HEAD the plugin sends.
    #[test]
    fn a_hosts_status_is_said() {
        let head = request("HEAD", "/", "example.com", &[("Connection", "close")], &[]);
        let reply = "HTTP/1.1 301 Moved Permanently\r\nLocation: https://example.com/\r\nContent-Length: 0\r\n\r\n";
        let project = Project::new("/p").host("example.com:80", Replies::new().on(head, reply));
        let mut h = Host::<Plugin>::load("http-status", project);
        h.run(":http-status example.com");
        assert_eq!(h.status(), Some("example.com: HTTP 301"));
    }
}
