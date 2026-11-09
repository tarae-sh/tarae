//! Reading a file under a `read` pattern: `:note` reads `~/notes/today.md` and says its first line. The pattern
//! (`~/notes/**`) is what you approve on its card; a file outside it is asked about, once, and a file named like a key
//! is never read whatever the pattern says. What it asked for comes back as an event (`file`, or `failed`).
use tarae_plugin::{Action, Command, Context, Event, Guest, HostEvent, Now, Permission, Subscription};

struct Plugin;

/// The request's id, to tell its answer from others.
const TODAY: u64 = 1;

impl Guest for Plugin {
    fn commands() -> Vec<Command> {
        vec![Command { name: "note".into(), doc: "Today's note's first line".into(), text: false }]
    }

    fn permissions() -> Vec<Permission> {
        vec![Permission::Read("~/notes/**".into())]
    }

    fn run(_: String, _: Context) -> Vec<Action> {
        vec![Action::ReadFile((TODAY, "~/notes/today.md".into()))]
    }

    fn on_events(events: Vec<HostEvent>, _: Now) -> Vec<Action> {
        events
            .into_iter()
            .filter_map(|e| match e {
                HostEvent::File((TODAY, bytes)) => {
                    let text = String::from_utf8_lossy(&bytes);
                    let first = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("(empty)");
                    Some(Action::Status(format!("today: {}", first.trim_start_matches('#').trim())))
                }
                HostEvent::Failed((TODAY, why)) => Some(Action::Warning(format!("no note today: {why}"))),
                _ => None,
            })
            .collect()
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
    use tarae_plugin::test::{Host, Project};

    /// Today's note, under its read pattern, read and its first line said.
    #[test]
    fn todays_note_is_read_under_its_pattern() {
        let project = Project::new("/p").file("~/notes/today.md", "\n# Ship the plugin\n- its tests\n");
        let mut h = Host::<Plugin>::load("read-notes", project);
        h.run(":note");
        assert_eq!(h.status(), Some("today: Ship the plugin"));
    }
}
