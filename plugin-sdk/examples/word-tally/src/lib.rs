//! A panel with its own keys: `:word-tally` counts the words of the buffer in front into a table below the code.
//! tarae moves through the rows (j k, the page keys, g G …); the keys it leaves come to the plugin — `s` sorts by
//! word or by count, `x` closes it. What the plugin keeps between calls is its own state.
use std::sync::Mutex;

use tarae_plugin::{Action, Command, Context, Event, Guest, HostEvent, Now, Panel, Permission, Subscription};

struct Plugin;

/// The last tally (word, count) and whether it's sorted by word.
static TALLY: Mutex<(Vec<(String, usize)>, bool)> = Mutex::new((Vec::new(), false));

impl Guest for Plugin {
    fn commands() -> Vec<Command> {
        vec![Command { name: "word-tally".into(), doc: "Count this buffer's words in a panel".into(), text: true }]
    }

    fn run(_: String, cx: Context) -> Vec<Action> {
        let mut counts: Vec<(String, usize)> = Vec::new();
        for w in cx.buffer.text.unwrap_or_default().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()) {
            let w = w.to_lowercase();
            match counts.iter_mut().find(|(k, _)| *k == w) {
                Some((_, n)) => *n += 1,
                None => counts.push((w, 1)),
            }
        }
        let mut t = TALLY.lock().unwrap();
        *t = (counts, false);
        vec![panel(&mut t)]
    }

    fn on_events(events: Vec<HostEvent>, _: Now) -> Vec<Action> {
        let mut t = TALLY.lock().unwrap();
        let mut out = Vec::new();
        for e in events {
            match e {
                HostEvent::Key((key, _, _)) if key == "s" => {
                    t.1 = !t.1;
                    out.push(panel(&mut t));
                }
                HostEvent::Key((key, _, _)) if key == "x" => out.push(Action::Panel(None)),
                _ => {}
            }
        }
        out
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

/// The panel for the tally, sorted as it says; ids keep the selection on its word as the rows move.
fn panel((counts, by_word): &mut (Vec<(String, usize)>, bool)) -> Action {
    match by_word {
        true => counts.sort_by(|a, b| a.0.cmp(&b.0)),
        false => counts.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0))),
    }
    Action::Panel(Some(Panel {
        title: "words".into(),
        columns: vec!["word".into(), "count".into()],
        rows: counts.iter().map(|(w, n)| vec![w.clone(), n.to_string()]).collect(),
        ids: counts.iter().map(|(w, _)| w.clone()).collect(),
        status: format!("{} different words", counts.len()),
        keys: vec![("s".into(), if *by_word { "by count" } else { "by word" }.into()), ("x".into(), "close".into())],
    }))
}

tarae_plugin::export!(Plugin);

#[cfg(test)]
mod tests {
    use super::*;
    use tarae_plugin::test::{Host, Project};

    /// The tally in its panel by count; `s` sorts it by word, `x` closes it.
    #[test]
    fn the_words_are_tallied_in_a_panel_with_its_own_keys() {
        let mut h = Host::<Plugin>::load("word-tally", Project::new("/p").file("a.txt", "The cat saw the dog"));
        h.open("a.txt");
        h.run(":word-tally");
        let first = |h: &Host<Plugin>| h.panel().expect("its panel").rows[0].join(" ");
        assert_eq!(first(&h), "the 2");
        assert_eq!(h.panel().unwrap().status, "4 different words");
        h.key("s");
        assert_eq!(first(&h), "cat 1");
        h.key("x");
        assert!(h.panel().is_none());
    }
}
