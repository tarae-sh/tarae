//! The mock host held to tarae's rules, through a stand-in plugin whose permissions and answer each test sets.

use std::cell::RefCell;

use super::*;
use crate::{Connect, Spawn};

thread_local! {
    static ASKS: RefCell<Vec<Permission>> = RefCell::default();
    static ANSWER: RefCell<Vec<Action>> = RefCell::default();
    static HEARD: RefCell<Vec<HostEvent>> = RefCell::default();
    static GIVEN: RefCell<Vec<(Event, Option<String>)>> = RefCell::default();
}

struct Stand;

impl Guest for Stand {
    fn commands() -> Vec<Command> {
        vec![Command { name: "go".into(), doc: "answers what the test set".into(), text: true }]
    }
    fn subscriptions() -> Vec<Subscription> {
        let on = |event| Subscription { event, text: true, languages: Vec::new() };
        vec![on(Event::Opened), on(Event::Shown)]
    }
    fn permissions() -> Vec<Permission> {
        ASKS.with(|a| a.borrow().clone())
    }
    fn run(_: String, _: Context) -> Vec<Action> {
        ANSWER.with(|a| a.take())
    }
    fn on(event: Event, cx: Context) -> Vec<Action> {
        GIVEN.with(|g| g.borrow_mut().push((event, cx.buffer.text)));
        Vec::new()
    }
    fn on_events(events: Vec<HostEvent>, _: Now) -> Vec<Action> {
        HEARD.with(|h| h.borrow_mut().extend(events));
        Vec::new()
    }
}

/// The stand-in asking for `asks`, loaded in `project`; `:go` answers `answer`.
fn host(asks: Vec<Permission>, answer: Vec<Action>, project: Project) -> Host<Stand> {
    ASKS.with(|a| *a.borrow_mut() = asks);
    ANSWER.with(|a| *a.borrow_mut() = answer);
    Host::load("stand", project)
}

fn heard() -> Vec<HostEvent> {
    HEARD.with(|h| h.take())
}

fn spawn(id: u64, program: &str) -> Action {
    Action::Spawn(Spawn { id, program: program.into(), args: Vec::new(), env: Vec::new() })
}

fn connect(id: u64, host: &str, port: u64) -> Action {
    Action::Connect(Connect { id, host: host.into(), port })
}

/// A read outside its patterns is tarae's card, said in its words, and nothing comes back (the card waits); one
/// inside them gets the file; with no read permission at all, it's refused — the failure tarae sends.
#[test]
fn a_read_outside_its_patterns_is_taraes_card() {
    let project = Project::new("/p").file("docs/a.md", "in").file("/etc/hosts", "out");
    let read = |id, path: &str| Action::ReadFile((id, path.into()));
    let mut h = host(vec![Permission::Read("/p/docs/**".into())], vec![read(1, "/etc/hosts"), read(2, "/p/docs/a.md")], project);
    h.run(":go");
    assert_eq!(heard(), vec![HostEvent::File((2, b"in".to_vec()))]);
    h.expect_refused("Plugin stand wants to read a file /etc/hosts\n(outside the files it asked for)");

    let mut h = host(Vec::new(), vec![read(1, "/p/docs/a.md")], Project::new("/p").file("docs/a.md", "in"));
    h.run(":go");
    assert_eq!(heard(), vec![HostEvent::Failed((1, "it didn't ask to read files (permissions())".into()))]);
    h.expect_refused("it didn't ask to read files");
}

/// A program it didn't declare is refused as tarae refuses it; one it did runs its script.
#[test]
fn an_undeclared_program_is_refused_in_taraes_words() {
    let project = Project::new("/p").program("git", Script::lines(["a", "b"]).exit(1));
    let mut h = host(vec![Permission::Run("cargo".into())], vec![spawn(1, "git")], project.clone());
    h.run(":go");
    let why = "it didn't ask to run the program git (with any arguments, as you) (permissions())";
    assert_eq!(heard(), vec![HostEvent::Failed((1, why.into()))]);
    h.expect_refused(why);

    let mut h = host(vec![Permission::Run("git".into())], vec![spawn(1, "git")], project);
    h.run(":go");
    let ran = vec![HostEvent::Lines((1, vec!["a".into(), "b".into()])), HostEvent::Exit((1, Some(1), Vec::new()))];
    assert_eq!(heard(), ran);
}

/// A connection without `network` is refused in tarae's words; with it, a host of the project's opens and says its
/// greeting, and one it isn't is tarae's card.
#[test]
fn a_connection_without_network_is_refused_in_taraes_words() {
    let project = Project::new("/p").host("db:5432", Replies::new().greet("hi"));
    let mut h = host(Vec::new(), vec![connect(1, "db", 5432)], project.clone());
    h.run(":go");
    let why = "it didn't ask to connect over the network (each new host is asked) (permissions())";
    assert_eq!(heard(), vec![HostEvent::Failed((1, why.into()))]);
    h.expect_refused(why);

    let mut h = host(vec![Permission::Network], vec![connect(1, "db", 5432), connect(2, "other", 80)], project);
    h.run(":go");
    assert_eq!(heard(), vec![HostEvent::Connected(1), HostEvent::Data((1, b"hi".to_vec()))]);
    h.expect_refused("Plugin stand wants to connect to other:80");
}

/// A refusal no test acknowledged fails it as the host drops — but not while the test is already failing: that
/// panic would abort the run and hide the first.
#[test]
fn an_unacknowledged_refusal_fails_the_test_but_never_twice() {
    let dropped = std::panic::catch_unwind(|| {
        let mut h = host(Vec::new(), vec![Action::ReadEnv((1, "HOME".into()))], Project::new("/p"));
        h.run(":go");
    });
    let said = dropped.unwrap_err().downcast::<String>().unwrap();
    assert!(said.contains("tarae would refuse stand here") && said.contains("read the environment variable HOME"), "{said}");

    let failing = std::panic::catch_unwind(|| {
        let mut h = host(Vec::new(), vec![Action::ReadEnv((1, "HOME".into()))], Project::new("/p"));
        h.run(":go");
        panic!("the test's own failure");
    });
    assert_eq!(*failing.unwrap_err().downcast::<&str>().unwrap(), "the test's own failure");
}

/// #457: a buffer goes to it on its own only when its grant covers the file — else only an empty `shown` — and a
/// file named like a key never, whatever it asked for.
#[test]
fn a_buffer_is_given_only_when_its_grant_covers_it() {
    let project = Project::new("/p").file("a.rs", "fn a() {}").file(".env", "TOKEN=x");
    let given = || GIVEN.with(|g| g.take());
    let mut h = host(Vec::new(), Vec::new(), project.clone());
    h.open("a.rs");
    assert_eq!(given(), vec![(Event::Shown, None)]);

    let mut h = host(vec![Permission::Project], Vec::new(), project);
    h.open("a.rs");
    let text = Some("fn a() {}".to_string());
    assert_eq!(given(), vec![(Event::Opened, text.clone()), (Event::Shown, text)]);
    h.open(".env");
    assert_eq!(given(), vec![(Event::Shown, None)]);
}

/// Your command's edits apply to the buffer in front, as one step; an open of a file its grant doesn't cover is
/// refused in tarae's words.
#[test]
fn edits_apply_and_an_open_outside_its_grant_is_refused() {
    let project = Project::new("/p").file("a.txt", "one two");
    let answer = vec![Action::ReplaceSelections(vec!["ONE".into()]), Action::Open("/etc/hosts".into())];
    let mut h = host(vec![Permission::Project], answer, project);
    h.open("a.txt");
    h.select(&[(0, 3)]);
    h.run(":go");
    assert_eq!(h.text(), "ONE two");
    h.expect_refused("it may not open “/etc/hosts”: not a file it may read (permissions())");
}

/// A plugin that hears keys answering with more than its overlay is refused as tarae refuses it, and a list tarae
/// won't load doesn't load here either.
#[test]
fn a_plugin_that_hears_keys_shows_only_its_overlay() {
    let mut h = host(vec![Permission::Keys], vec![Action::Status("heard".into()), Action::Overlay(None)], Project::new("/p"));
    h.run(":go");
    assert_eq!(h.status(), None);
    h.expect_refused("a message: a plugin that hears keys shows what it hears in its overlay only");

    let loaded = std::panic::catch_unwind(|| host(vec![Permission::Keys, Permission::Network], Vec::new(), Project::new("/p")));
    let said = loaded.err().unwrap().downcast::<String>().unwrap();
    assert!(said.contains("tarae won't load stand: it asks to hear keys and for “network”"), "{said}");
}
