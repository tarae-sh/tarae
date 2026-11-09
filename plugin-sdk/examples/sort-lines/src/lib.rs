//! A command over the selections: `:sort-lines` sorts the lines inside each selection, `:sort-lines r` backwards.
//! A command gets the selections' text without asking (`text: false` — the whole buffer isn't needed), and answers
//! with one new text per selection: one undo step.
use tarae_plugin::{Action, Command, Context, Event, Guest, HostEvent, Now, Permission, Subscription};

struct Plugin;

impl Guest for Plugin {
    fn commands() -> Vec<Command> {
        vec![Command { name: "sort-lines".into(), doc: "Sort the lines in each selection (r: backwards)".into(), text: false }]
    }

    fn run(name: String, cx: Context) -> Vec<Action> {
        if name != "sort-lines" {
            return vec![Action::Error(format!("unknown command {name}"))];
        }
        let backwards = cx.args.trim() == "r";
        let sorted: Vec<String> = cx.selections.iter().map(|s| sort(&s.text, backwards)).collect();
        let lines: usize = cx.selections.iter().map(|s| s.text.lines().count()).sum();
        vec![Action::ReplaceSelections(sorted), Action::Status(format!("sorted {lines} lines"))]
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

    fn on_events(_: Vec<HostEvent>, _: Now) -> Vec<Action> {
        Vec::new()
    }
}

/// `text`'s lines sorted, its last line end kept as it was (a selection of whole lines ends with one).
fn sort(text: &str, backwards: bool) -> String {
    let end = if text.ends_with('\n') { "\n" } else { "" };
    let mut lines: Vec<&str> = text.lines().collect();
    lines.sort_unstable();
    if backwards {
        lines.reverse();
    }
    lines.join("\n") + end
}

tarae_plugin::export!(Plugin);

#[cfg(test)]
mod tests {
    use super::*;
    use tarae_plugin::test::{Host, Project};

    /// The lines in the selection sorted, its last line end kept; `r` backwards.
    #[test]
    fn the_selections_lines_are_sorted() {
        let mut h = Host::<Plugin>::load("sort-lines", Project::new("/p").file("fruit.txt", "pear\napple\nfig\n"));
        h.open("fruit.txt");
        h.select(&[(0, 15)]);
        h.run(":sort-lines");
        assert_eq!(h.text(), "apple\nfig\npear\n");
        assert_eq!(h.status(), Some("sorted 3 lines"));
        h.select(&[(0, 15)]);
        h.run(":sort-lines r");
        assert_eq!(h.text(), "pear\nfig\napple\n");
    }
}
