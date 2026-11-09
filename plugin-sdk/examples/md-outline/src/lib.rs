//! A side tab: the Markdown file in front's headings, as a tree in the side panel; `enter` on a row goes to its
//! section, and the row stays marked while the cursor is in it. It hears `shown` (another file came to the front) and
//! `changed`; a buffer it isn't given comes empty, so the tab clears.
use tarae_plugin::{Action, Command, Context, Event, Guest, HostEvent, Now, Permission, Side, SideRow, Subscription, Tone};

struct Plugin;

impl Guest for Plugin {
    fn commands() -> Vec<Command> {
        vec![Command { name: "md-outline".into(), doc: "This Markdown file's headings, in the side panel".into(), text: true }]
    }

    fn subscriptions() -> Vec<Subscription> {
        let md = || vec!["markdown".to_string()];
        vec![
            Subscription { event: Event::Shown, text: true, languages: Vec::new() },
            Subscription { event: Event::Changed, text: true, languages: md() },
        ]
    }

    /// The project's buffers are given to it on its own: `project`.
    fn permissions() -> Vec<Permission> {
        vec![Permission::Project]
    }

    /// `:md-outline` opens the tab with the keys in it.
    fn run(_: String, cx: Context) -> Vec<Action> {
        vec![side(&cx, true)]
    }

    fn on(_: Event, cx: Context) -> Vec<Action> {
        vec![side(&cx, false)]
    }

    fn on_events(_: Vec<HostEvent>, _: Now) -> Vec<Action> {
        Vec::new()
    }
}

/// The tab for the buffer in `cx`: a row a heading, by its depth, each ending where the next of its depth or less starts.
fn side(cx: &Context, open: bool) -> Action {
    let markdown = cx.buffer.language.as_deref() == Some("markdown");
    let (path, text) = match (&cx.buffer.path, &cx.buffer.text) {
        (Some(p), Some(t)) if markdown => (p.clone(), t.clone()),
        _ => return Action::Side(Some(Side { title: "outline".into(), note: String::new(), rows: Vec::new(), keys: Vec::new(), open, focus: open })),
    };
    let lines: Vec<&str> = text.lines().collect();
    let heads: Vec<(usize, usize, &str)> = (lines.iter().enumerate())
        .filter_map(|(i, l)| {
            let depth = l.chars().take_while(|c| *c == '#').count();
            (1..=6).contains(&depth).then(|| (i, depth, l[depth..].trim()))
        })
        .collect();
    let rows = (heads.iter().enumerate())
        .map(|(k, &(line, depth, title))| {
            let end = heads[k + 1..].iter().find(|h| h.1 <= depth).map_or(lines.len(), |h| h.0).saturating_sub(1);
            SideRow {
                id: line.to_string(),
                depth: depth as u64 - 1,
                text: title.to_string(),
                detail: format!("{}", end + 1 - line),
                tone: Tone::Ok,
                at: Some((path.clone(), line as u64, end.max(line) as u64)),
                note: false,
            }
        })
        .collect();
    Action::Side(Some(Side { title: "outline".into(), note: cx.buffer.name.clone(), rows, keys: Vec::new(), open, focus: open }))
}

tarae_plugin::export!(Plugin);

#[cfg(test)]
mod tests {
    use super::*;
    use tarae_plugin::test::{Host, Project};

    /// The headings as rows by depth, each with its section's length; a file that isn't Markdown clears the tab.
    #[test]
    fn the_headings_are_a_tree_in_the_side_tab() {
        let guide = "# Guide\nintro\n## Install\nsteps\n## Use\nmore\n";
        let project = Project::new("/p").file("guide.md", guide).file("a.txt", "# not a heading here\n");
        let mut h = Host::<Plugin>::load("md-outline", project);
        h.open("guide.md");
        h.run(":md-outline");
        let side = h.side().expect("its tab");
        let rows: Vec<(u64, &str, &str)> = side.rows.iter().map(|r| (r.depth, r.text.as_str(), r.detail.as_str())).collect();
        assert_eq!(rows, [(0, "Guide", "6"), (1, "Install", "2"), (1, "Use", "2")]);
        h.open("a.txt");
        assert!(h.side().expect("its tab").rows.is_empty());
    }
}
