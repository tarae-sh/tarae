//! Hints tied to a buffer version: each `TODO` line gets a hint at its end, worked out on the text at the version the
//! event came with. Edited, the hints go until they're sent again (`changed` comes once you stop typing), so a hint
//! never stands beside a line that moved.
use tarae_plugin::{Action, Command, Context, Event, Guest, Hint, Hints, HostEvent, Now, Permission, Subscription, Tone};

struct Plugin;

impl Guest for Plugin {
    fn commands() -> Vec<Command> {
        Vec::new()
    }

    fn subscriptions() -> Vec<Subscription> {
        [Event::Opened, Event::Changed]
            .into_iter()
            .map(|event| Subscription { event, text: true, languages: Vec::new() })
            .collect()
    }

    /// The project's buffers are given to it on its own (events): `project`.
    fn permissions() -> Vec<Permission> {
        vec![Permission::Project]
    }

    fn run(_: String, _: Context) -> Vec<Action> {
        Vec::new()
    }

    fn on(_: Event, cx: Context) -> Vec<Action> {
        let (Some(path), Some(text)) = (cx.buffer.path, cx.buffer.text) else { return Vec::new() };
        vec![Action::Hints(Hints { path, version: cx.buffer.version, hints: hints(&text) })]
    }

    fn on_events(_: Vec<HostEvent>, _: Now) -> Vec<Action> {
        Vec::new()
    }
}

/// A hint for each line with a `TODO`: its number among them.
fn hints(text: &str) -> Vec<Hint> {
    let todos = text.lines().enumerate().filter(|(_, l)| l.contains("TODO"));
    let all: Vec<usize> = todos.map(|(i, _)| i).collect();
    let n = all.len();
    all.into_iter()
        .enumerate()
        .map(|(k, line)| Hint { line: line as u64, text: format!("todo {} of {n}", k + 1), tone: Tone::Info })
        .collect()
}

tarae_plugin::export!(Plugin);

#[cfg(test)]
mod tests {
    use super::*;
    use tarae_plugin::test::{Host, Project};

    /// A hint at each TODO as the file opens, sent again for the version an edit made.
    #[test]
    fn each_todo_gets_a_hint_for_the_version_it_was_found_in() {
        let text = "fn a() {}\n// TODO one\nfn b() {}\n// TODO two\n";
        let mut h = Host::<Plugin>::load("todo-hints", Project::new("/p").file("a.rs", text));
        let said = |h: &Host<Plugin>| {
            let hints = h.hints("a.rs").expect("hints for a.rs");
            (hints.version, hints.hints.iter().map(|x| (x.line, x.text.clone())).collect::<Vec<_>>())
        };
        h.open("a.rs");
        assert_eq!(said(&h), (0, vec![(1, "todo 1 of 2".into()), (3, "todo 2 of 2".into())]));
        h.edit(0..0, "// TODO first\n");
        let (version, hints) = said(&h);
        assert_eq!((version, hints.len(), &hints[0]), (1, 3, &(0, "todo 1 of 3".to_string())));
    }
}
