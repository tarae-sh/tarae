//! An event with a language filter: a YAML file opened or saved with a tab in its indentation says so (YAML refuses
//! tabs there). It subscribes to `opened` and `saved` in "yaml" only, with the text (`text: true`) — and asks for
//! `project`: on its own (an event) a plugin is given only the buffers its grant covers.
use tarae_plugin::{Action, Command, Context, Event, Guest, HostEvent, Now, Permission, Subscription};

struct Plugin;

impl Guest for Plugin {
    fn commands() -> Vec<Command> {
        Vec::new()
    }

    fn subscriptions() -> Vec<Subscription> {
        let yaml = || vec!["yaml".to_string()];
        vec![
            Subscription { event: Event::Opened, text: true, languages: yaml() },
            Subscription { event: Event::Saved, text: true, languages: yaml() },
        ]
    }

    fn permissions() -> Vec<Permission> {
        vec![Permission::Project]
    }

    fn run(_: String, _: Context) -> Vec<Action> {
        Vec::new()
    }

    fn on(_: Event, cx: Context) -> Vec<Action> {
        // (none: a buffer it isn't given — outside the project)
        let Some(text) = cx.buffer.text else { return Vec::new() };
        match first_tab(&text) {
            Some(line) => vec![Action::Warning(format!("{}: a tab indents line {}", cx.buffer.name, line + 1))],
            None => Vec::new(),
        }
    }

    fn on_events(_: Vec<HostEvent>, _: Now) -> Vec<Action> {
        Vec::new()
    }
}

/// The first line (from 0) whose indentation holds a tab.
fn first_tab(text: &str) -> Option<usize> {
    text.lines().position(|l| l.chars().take_while(|c| c.is_whitespace()).any(|c| c == '\t'))
}

tarae_plugin::export!(Plugin);

#[cfg(test)]
mod tests {
    use super::*;
    use tarae_plugin::test::{Host, Project};

    /// A YAML file opened with a tab in its indentation is said; a tab in another language's file isn't heard.
    #[test]
    fn a_tab_indenting_yaml_is_said_as_it_opens() {
        let project = Project::new("/p").file("notes.md", "\tfine in markdown\n").file("ci.yaml", "jobs:\n\tbuild: true\n");
        let mut h = Host::<Plugin>::load("yaml-tabs", project);
        h.open("notes.md");
        assert_eq!(h.status(), None);
        h.open("ci.yaml");
        assert_eq!(h.status(), Some("ci.yaml: a tab indents line 2"));
    }
}
