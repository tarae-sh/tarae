//! State kept with `write-own`: `:stamp` notes the time and the file in front, and writes the whole list to a file of
//! its own (in its folder under tarae's state folder: no permission needed, nowhere else). Where it went comes back
//! (`written`). The time is the context's: a plugin has no clock of its own.
use std::sync::Mutex;

use tarae_plugin::{Action, Command, Context, Event, Guest, HostEvent, Now, Permission, Subscription};

struct Plugin;

const SAVE: u64 = 1;

/// Each stamp: (milliseconds since 1970, the buffer's name).
static STAMPS: Mutex<Vec<(u64, String)>> = Mutex::new(Vec::new());

impl Guest for Plugin {
    fn commands() -> Vec<Command> {
        vec![Command { name: "stamp".into(), doc: "Note the time and this file".into(), text: false }]
    }

    fn run(_: String, cx: Context) -> Vec<Action> {
        let mut stamps = STAMPS.lock().unwrap();
        stamps.push((cx.now.millis, cx.buffer.name));
        let text: String = stamps.iter().map(|(ms, name)| format!("{ms}\t{name}\n")).collect();
        vec![Action::WriteOwn((SAVE, "stamps.tsv".into(), text.into_bytes()))]
    }

    fn on_events(events: Vec<HostEvent>, _: Now) -> Vec<Action> {
        let n = STAMPS.lock().unwrap().len();
        events
            .into_iter()
            .filter_map(|e| match e {
                HostEvent::Written((SAVE, path)) => Some(Action::Status(format!("{n} stamps in {path}"))),
                HostEvent::Failed((SAVE, why)) => Some(Action::Error(format!("stamps not kept: {why}"))),
                _ => None,
            })
            .collect()
    }

    fn subscriptions() -> Vec<Subscription> {
        Vec::new()
    }

    fn permissions() -> Vec<Permission> {
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
    use tarae_plugin::test::{Host, Project};

    /// Each stamp the context's time and the file in front, the whole list written to its own file.
    #[test]
    fn the_stamps_are_kept_in_its_own_file() {
        let mut h = Host::<Plugin>::load("stamps", Project::new("/p").file("a.rs", "").file("b.rs", ""));
        h.open("a.rs");
        h.run(":stamp");
        h.advance(60_000);
        h.open("b.rs");
        h.run(":stamp");
        let kept = h.written_own("stamps.tsv").map(String::from_utf8_lossy);
        assert_eq!(kept.as_deref(), Some("1700000000000\ta.rs\n1700000060000\tb.rs\n"));
        assert_eq!(h.status(), Some("2 stamps in /state/tarae/plugins/stamps/stamps.tsv"));
    }
}
