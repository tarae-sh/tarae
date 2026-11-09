//! A before-save check: `:w` on a file whose last line has no line end adds one, in the saved file. tarae waits for
//! the answer (up to 2 seconds) before it writes; the edit is one undo step, on the text the event came with.
use tarae_plugin::{Action, Command, Context, Edit, Event, Guest, HostEvent, Now, Permission, Subscription};

struct Plugin;

impl Guest for Plugin {
    fn commands() -> Vec<Command> {
        Vec::new()
    }

    fn subscriptions() -> Vec<Subscription> {
        vec![Subscription { event: Event::BeforeSave, text: true, languages: Vec::new() }]
    }

    /// The project's files are given to it on its own (an event): `project`. Outside it, nothing is added.
    fn permissions() -> Vec<Permission> {
        vec![Permission::Project]
    }

    fn run(_: String, _: Context) -> Vec<Action> {
        Vec::new()
    }

    fn on(event: Event, cx: Context) -> Vec<Action> {
        let Some(text) = cx.buffer.text.filter(|_| matches!(event, Event::BeforeSave)) else { return Vec::new() };
        if text.is_empty() || text.ends_with('\n') {
            return Vec::new();
        }
        let end = text.len() as u64;
        vec![Action::Edits(vec![Edit { from: end, to: end, insert: "\n".into() }])]
    }

    fn on_events(_: Vec<HostEvent>, _: Now) -> Vec<Action> {
        Vec::new()
    }
}

tarae_plugin::export!(Plugin);

#[cfg(test)]
mod tests {
    use super::*;
    use tarae_plugin::test::{Host, Project};

    /// A save adds the missing line end, in what's saved; one already there stays alone.
    #[test]
    fn a_save_ends_the_last_line() {
        let project = Project::new("/p").file("a.txt", "last line").file("b.txt", "ended\n");
        let mut h = Host::<Plugin>::load("final-newline", project);
        h.open("a.txt");
        h.save();
        assert_eq!(h.text(), "last line\n");
        h.open("b.txt");
        h.save();
        assert_eq!(h.text(), "ended\n");
    }
}
