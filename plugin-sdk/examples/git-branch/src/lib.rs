//! A program run with `run(prog)`: `:branch` runs `git rev-parse --abbrev-ref HEAD` (no shell — a program and its
//! arguments) and shows the branch as a badge on the status line. Its output comes back in lines, then its end with the
//! exit code and its last lines on stderr.
use tarae_plugin::{Action, Command, Context, Event, Guest, HostEvent, Now, Permission, Spawn, Subscription, Tone};

struct Plugin;

const GIT: u64 = 1;

impl Guest for Plugin {
    fn commands() -> Vec<Command> {
        vec![Command { name: "branch".into(), doc: "The branch, as a badge on the status line".into(), text: false }]
    }

    /// Only `git`, with any arguments — the card says so.
    fn permissions() -> Vec<Permission> {
        vec![Permission::Run("git".into())]
    }

    fn run(_: String, _: Context) -> Vec<Action> {
        let args = ["rev-parse", "--abbrev-ref", "HEAD"].map(String::from).to_vec();
        vec![Action::Spawn(Spawn { id: GIT, program: "git".into(), args, env: Vec::new() })]
    }

    fn on_events(events: Vec<HostEvent>, _: Now) -> Vec<Action> {
        events
            .into_iter()
            .filter_map(|e| match e {
                HostEvent::Lines((GIT, lines)) => {
                    let branch = lines.first()?.trim().to_string();
                    Some(Action::Badge(Some((branch, Tone::Info))))
                }
                HostEvent::Exit((GIT, Some(0), _)) => None,
                HostEvent::Exit((GIT, _, stderr)) => Some(Action::Warning(format!("git: {}", stderr.join(" ")))),
                HostEvent::Failed((GIT, why)) => Some(Action::Error(format!("couldn't run git: {why}"))),
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
    use tarae_plugin::test::{Host, Project, Script};

    /// git's answer as the badge; a git that fails is said with its stderr.
    #[test]
    fn the_branch_git_names_is_the_badge() {
        let mut h = Host::<Plugin>::load("git-branch", Project::new("/p").program("git", Script::lines(["main"])));
        h.run(":branch");
        assert_eq!(h.badge().map(|b| b.0.as_str()), Some("main"));
        let failing = Script::lines(Vec::<String>::new()).stderr(["fatal: not a git repository"]).exit(128);
        let mut h = Host::<Plugin>::load("git-branch", Project::new("/p").program("git", failing));
        h.run(":branch");
        assert_eq!(h.status(), Some("git: fatal: not a git repository"));
    }
}
