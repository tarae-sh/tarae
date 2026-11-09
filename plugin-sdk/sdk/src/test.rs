//! The mock host: a plugin's native tests drive it as tarae would, and it holds the plugin to its declared
//! permissions with tarae's own rules and words (`tarae-plugin-rules`, the crate tarae itself judges by).
//!
//! ```ignore
//! use tarae_plugin::test::{Host, Project, Script};
//!
//! #[test]
//! fn words_are_counted() {
//!     let project = Project::new("/p").file("notes.md", "one two three");
//!     let mut h = Host::<Plugin>::load("words", project);
//!     h.open("notes.md");
//!     h.select(&[(0, 13)]);
//!     h.run(":words");
//!     assert_eq!(h.status(), Some("3 words"));
//! }
//! ```
//!
//! - **The project** is virtual: files (`~/` ones under `home`), environment variables, programs with scripted
//!   output, hosts with scripted replies, the time and the 32 random bytes every call gets. Nothing touches the
//!   disk, the environment or the network.
//! - **Each answer's actions are applied as tarae applies them**: a `read-file` gets the file's bytes, `spawn` its
//!   script's lines and exit, `connect` its host's replies, `find` the files a pattern matches (never ones named
//!   like keys), edits change the buffer; panel, side tab, hints, overlay, status and badge are kept as last drawn.
//!   What comes back goes to `on-events` in one call a round, in the order the actions came, until nothing is
//!   pending — deterministic; tarae's own order is asynchronous, so a plugin relies on no more than each
//!   request's events coming in order.
//! - **A refusal fails the test**: what tarae refuses (an undeclared program or variable, a read with no read
//!   permission, `network` undeclared, a file it may not open, an edit where it may not) comes back to the plugin as
//!   tarae sends it — the same `failed` and words —, and is recorded. So is a card tarae would raise (a file outside
//!   its read patterns, a host not in the project): the card's words, and nothing comes back, as while a card
//!   waits — unless `allowed` answered it. When the host drops, each one not acknowledged by `expect_refused`
//!   fails the test.
//! - **Not here**: tarae's approvals (the load card is taken as answered yes), the hosts tarae never reaches
//!   (cloud metadata servers, link-local addresses: tarae's own list — a mock connection reaches only a scripted
//!   host), and running tarae's commands (`run` actions are kept: `ran`).
//!
//! Keep a plugin's state in a `thread_local!`: each test runs on its own thread, so each starts fresh.

use std::collections::BTreeMap;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use tarae_plugin_rules as rules;

use crate::{
    Action, Buffer, Command, Context, Event, Guest, Hints, HostEvent, KeyEvent, Now, Overlay, Panel, Permission,
    Pick, Selection, Side, Subscription, Tone, View,
};

/// A program's scripted run: its lines on stdout, its last lines on stderr, its exit code.
#[derive(Clone, Debug, Default)]
pub struct Script {
    lines: Vec<String>,
    stderr: Vec<String>,
    code: Option<u64>,
}

impl Script {
    /// A run that prints `lines` and exits 0.
    pub fn lines<S: Into<String>>(lines: impl IntoIterator<Item = S>) -> Script {
        Script { lines: lines.into_iter().map(Into::into).collect(), stderr: Vec::new(), code: Some(0) }
    }

    /// Its last lines on stderr.
    pub fn stderr<S: Into<String>>(mut self, lines: impl IntoIterator<Item = S>) -> Script {
        self.stderr = lines.into_iter().map(Into::into).collect();
        self
    }

    /// Its exit code (None: a signal stopped it).
    pub fn exit(mut self, code: impl Into<Option<u64>>) -> Script {
        self.code = code.into();
        self
    }
}

/// A host's scripted replies: what it says on connecting, and its answer to each exact `send`.
#[derive(Clone, Debug, Default)]
pub struct Replies {
    greeting: Option<Vec<u8>>,
    on: Vec<(Vec<u8>, Vec<u8>)>,
}

impl Replies {
    pub fn new() -> Replies {
        Replies::default()
    }

    /// What it sends as the connection opens.
    pub fn greet(mut self, bytes: impl AsRef<[u8]>) -> Replies {
        self.greeting = Some(bytes.as_ref().to_vec());
        self
    }

    /// Its answer to a `send` of exactly `sent`.
    pub fn on(mut self, sent: impl AsRef<[u8]>, reply: impl AsRef<[u8]>) -> Replies {
        self.on.push((sent.as_ref().to_vec(), reply.as_ref().to_vec()));
        self
    }
}

/// The world a plugin runs in, for a test: a virtual project and what's around it.
#[derive(Clone, Debug)]
pub struct Project {
    root: PathBuf,
    home: PathBuf,
    files: BTreeMap<PathBuf, Vec<u8>>,
    env: BTreeMap<String, String>,
    programs: BTreeMap<String, Script>,
    hosts: BTreeMap<String, Replies>,
    allowed: Vec<String>,
    millis: u64,
    entropy: [u8; 32],
}

impl Project {
    /// A project at `root` (absolute; nothing on disk), home at `/home/u`.
    pub fn new(root: impl Into<PathBuf>) -> Project {
        let root = root.into();
        assert!(root.is_absolute(), "a project's root is absolute: {}", root.display());
        Project {
            root,
            home: PathBuf::from("/home/u"),
            files: BTreeMap::new(),
            env: BTreeMap::new(),
            programs: BTreeMap::new(),
            hosts: BTreeMap::new(),
            allowed: Vec::new(),
            millis: 1_700_000_000_000,
            entropy: [7; 32],
        }
    }

    /// Home somewhere else (where `~/` leads).
    pub fn home(mut self, home: impl Into<PathBuf>) -> Project {
        self.home = home.into();
        self
    }

    /// A file: relative to the root, from `~/`, or absolute.
    pub fn file(mut self, path: &str, text: impl AsRef<[u8]>) -> Project {
        let at = self.path(path);
        self.files.insert(at, text.as_ref().to_vec());
        self
    }

    /// An environment variable.
    pub fn env(mut self, name: &str, value: &str) -> Project {
        self.env.insert(name.into(), value.into());
        self
    }

    /// A program on the PATH, and what a run of it does (whatever its arguments).
    pub fn program(mut self, name: &str, script: Script) -> Project {
        self.programs.insert(name.into(), script);
        self
    }

    /// A host that listens (`host:port`, an IPv6 address in brackets) — and the card asking about it answered
    /// yes: a connection to it opens.
    pub fn host(mut self, target: &str, replies: Replies) -> Project {
        self.hosts.insert(target.into(), replies);
        self
    }

    /// A file outside its read patterns whose card was answered yes (as written: from `~/` or absolute).
    pub fn allowed(mut self, path: &str) -> Project {
        let at = self.path(path);
        self.allowed.push(at.display().to_string());
        self
    }

    /// The time (ms since 1970) and the random bytes every call gets.
    pub fn now(mut self, millis: u64, entropy: [u8; 32]) -> Project {
        self.millis = millis;
        self.entropy = entropy;
        self
    }

    fn path(&self, path: &str) -> PathBuf {
        let whole = if path.starts_with('/') || path.starts_with('~') {
            path.to_string()
        } else {
            format!("{}/{path}", self.root.display())
        };
        rules::expand(&whole, Some(&self.home)).unwrap_or_else(|| panic!("not a path in the project: {path}"))
    }
}

/// The buffer in front.
#[derive(Clone, Debug, Default)]
struct Buf {
    path: Option<PathBuf>,
    /// A view of the plugin's (`show`), by its id.
    view: Option<u64>,
    text: String,
    language: Option<String>,
    version: u64,
    modified: bool,
    /// (anchor, head) a selection, byte offsets.
    selections: Vec<(u64, u64)>,
}

/// A plugin loaded in the mock host. Dropped with a refusal not acknowledged (`expect_refused`), it fails the test.
pub struct Host<P: Guest> {
    name: String,
    project: Project,
    commands: Vec<Command>,
    subscriptions: Vec<Subscription>,
    permissions: Vec<Permission>,
    buf: Buf,
    now: u64,
    messages: Vec<(Tone, String)>,
    status_bar: Option<String>,
    panel: Option<Panel>,
    side: Option<Side>,
    row: u64,
    hints: BTreeMap<String, Hints>,
    overlay: Option<Overlay>,
    badge: Option<(String, Tone)>,
    views: BTreeMap<u64, View>,
    own: BTreeMap<String, Vec<u8>>,
    asks: Vec<(u64, String)>,
    pick: Option<Pick>,
    timers: BTreeMap<u64, u64>,
    connections: BTreeMap<u64, String>,
    ran: Vec<String>,
    opened: Vec<String>,
    offered: Vec<String>,
    refused: Vec<String>,
    _plugin: PhantomData<P>,
}

/// Where `write-own` keeps a plugin's files, in the mock.
const OWN: &str = "/state/tarae/plugins";
/// Rounds of answers and events before the host gives up on a plugin that never settles.
const ROUNDS: usize = 1000;

impl<P: Guest> Host<P> {
    /// Loads the plugin as `name` (its file's name in tarae: the words say it), in `project`: its commands,
    /// subscriptions and permissions — a list tarae refuses at load panics, in tarae's words.
    pub fn load(name: &str, project: Project) -> Host<P> {
        let permissions = P::permissions();
        if let Some(why) = rules::invalid(&permissions.iter().map(perm).collect::<Vec<_>>()) {
            panic!("tarae won't load {name}: {why}");
        }
        Host {
            name: name.into(),
            now: project.millis,
            project,
            commands: P::commands(),
            subscriptions: P::subscriptions(),
            permissions,
            buf: Buf::default(),
            messages: Vec::new(),
            status_bar: None,
            panel: None,
            side: None,
            row: 0,
            hints: BTreeMap::new(),
            overlay: None,
            badge: None,
            views: BTreeMap::new(),
            own: BTreeMap::new(),
            asks: Vec::new(),
            pick: None,
            timers: BTreeMap::new(),
            connections: BTreeMap::new(),
            ran: Vec::new(),
            opened: Vec::new(),
            offered: Vec::new(),
            refused: Vec::new(),
            _plugin: PhantomData,
        }
    }

    // ── What you do ──

    /// Opens a project file in front (`opened`, then `shown`, to a plugin subscribed — with its text only when its
    /// grant covers the file, #457).
    pub fn open(&mut self, path: &str) {
        let at = self.project.path(path);
        let text = self.project.files.get(&at).unwrap_or_else(|| panic!("no file {} in the project", at.display()));
        let language = language(&at);
        self.buf = Buf {
            text: String::from_utf8_lossy(text).into_owned(),
            path: Some(at),
            language,
            selections: vec![(0, 0)],
            ..Buf::default()
        };
        self.fire(Event::Opened);
        self.fire(Event::Shown);
    }

    /// The buffer in front's language, as tarae names it (`open` guesses from the extension).
    pub fn language(&mut self, language: &str) {
        self.buf.language = Some(language.into());
    }

    /// The selections, (anchor, head) each — byte offsets; the last is the primary.
    pub fn select(&mut self, ranges: &[(u64, u64)]) {
        let len = self.buf.text.len() as u64;
        assert!(ranges.iter().all(|&(a, h)| a <= len && h <= len), "selections past the buffer's end");
        self.buf.selections = ranges.to_vec();
    }

    /// Runs one of its commands as you would: `":name args"` (or without the colon) on the buffer in front.
    pub fn run(&mut self, line: &str) {
        let line = line.strip_prefix(':').unwrap_or(line);
        let (name, args) = line.split_once(' ').unwrap_or((line, ""));
        let cmd = self.commands.iter().find(|c| c.name == name).cloned();
        let Some(cmd) = cmd else { panic!("{} registers no command :{name}", self.name) };
        // (a file named like a key is never given, even to your command — #457)
        if let Some(path) = self.buf.path.as_ref().filter(|p| rules::secretnames::named_secret(p)) {
            let why = format!(":{name} isn't given {}: a file named like a key", path.display());
            self.messages.push((Tone::Error, why));
            return;
        }
        let cx = self.context(args, true, cmd.text);
        let actions = P::run(name.into(), cx);
        self.settle(actions, true);
    }

    /// An edit of yours to the buffer in front (`changed` follows, as after a pause).
    pub fn edit(&mut self, range: std::ops::Range<u64>, insert: &str) {
        let (from, to) = (range.start as usize, range.end as usize);
        assert!(self.buf.text.is_char_boundary(from) && self.buf.text.is_char_boundary(to), "not on a character");
        self.buf.text.replace_range(from..to, insert);
        self.buf.version += 1;
        self.buf.modified = true;
        self.fire(Event::Changed);
    }

    /// Saves the buffer in front: `before-save` (whose edits land in what's saved), then `saved`.
    pub fn save(&mut self) {
        self.fire(Event::BeforeSave);
        if let Some(path) = &self.buf.path {
            self.project.files.insert(path.clone(), self.buf.text.clone().into_bytes());
        }
        self.buf.modified = false;
        self.fire(Event::Saved);
    }

    /// Closes the buffer in front (`closed`).
    pub fn close(&mut self) {
        self.fire(Event::Closed);
        self.buf = Buf::default();
    }

    /// A key in its panel — or its side tab, with no panel up — on the row `row` selects.
    pub fn key(&mut self, key: &str) {
        let id = match (&self.panel, &self.side) {
            (Some(p), _) => p.ids.get(self.row as usize).cloned(),
            (None, Some(s)) => s.rows.get(self.row as usize).map(|r| r.id.clone()),
            (None, None) => panic!("{} has no panel or side tab up for a key", self.name),
        };
        self.events(vec![HostEvent::Key((key.into(), Some(self.row), id))]);
    }

    /// Moves the selected row of its panel or side tab (from 0).
    pub fn row(&mut self, row: u64) {
        self.row = row;
    }

    /// Answers what it asks: its pick if one is up, else its oldest prompt (None: esc).
    pub fn answer(&mut self, answer: Option<&str>) {
        let id = match self.pick.take() {
            Some(p) => p.id,
            None if !self.asks.is_empty() => self.asks.remove(0).0,
            None => panic!("{} asks nothing to answer", self.name),
        };
        self.events(vec![HostEvent::Answer((id, answer.map(String::from)))]);
    }

    /// A press on its overlay's row `row`.
    pub fn press(&mut self, row: u64) {
        assert!(self.overlay.is_some(), "{} shows no overlay", self.name);
        self.events(vec![HostEvent::Pressed(row)]);
    }

    /// Keys you pressed, as a plugin that hears keys hears them.
    pub fn keys(&mut self, keys: Vec<KeyEvent>) {
        assert!(self.permissions.contains(&Permission::Keys), "{} didn't ask to hear keys", self.name);
        self.events(vec![HostEvent::Keys(keys)]);
    }

    /// Time passes: the timers due go off, in order.
    pub fn advance(&mut self, ms: u64) {
        let until = self.now + ms;
        loop {
            let due = self.timers.iter().filter(|(_, at)| **at <= until).min_by_key(|(id, at)| (**at, **id));
            let Some((&id, &at)) = due else { break };
            self.timers.remove(&id);
            self.now = at;
            self.events(vec![HostEvent::Tick(id)]);
        }
        self.now = until;
    }

    /// Events of your own making, straight to `on-events` — then their answers applied.
    pub fn events(&mut self, events: Vec<HostEvent>) {
        let actions = P::on_events(events, self.stamp());
        self.settle(actions, false);
    }

    /// One of tarae's refusals or cards (its words, or a part of them) expected: it no longer fails the test.
    pub fn expect_refused(&mut self, words: &str) {
        let Some(i) = self.refused.iter().position(|r| r.contains(words)) else {
            panic!("tarae refused nothing saying “{words}” — it refused: {:?}", self.refused);
        };
        self.refused.remove(i);
    }

    // ── What it shows ──

    /// The buffer in front's text.
    pub fn text(&self) -> &str {
        &self.buf.text
    }

    /// The selections in front, (anchor, head) each.
    pub fn selections(&self) -> &[(u64, u64)] {
        &self.buf.selections
    }

    /// The last message it gave (status, success, warning or error).
    pub fn status(&self) -> Option<&str> {
        self.messages.last().map(|(_, m)| m.as_str())
    }

    /// Every message it gave, with its tone.
    pub fn messages(&self) -> &[(Tone, String)] {
        &self.messages
    }

    /// Its status text for the buffer in front.
    pub fn status_bar(&self) -> Option<&str> {
        self.status_bar.as_deref()
    }

    pub fn panel(&self) -> Option<&Panel> {
        self.panel.as_ref()
    }

    pub fn side(&self) -> Option<&Side> {
        self.side.as_ref()
    }

    /// Its hints for a file (as written: relative to the root, from `~/`, or absolute).
    pub fn hints(&self, path: &str) -> Option<&Hints> {
        self.hints.get(&self.project.path(path).display().to_string())
    }

    pub fn overlay(&self) -> Option<&Overlay> {
        self.overlay.as_ref()
    }

    pub fn badge(&self) -> Option<&(String, Tone)> {
        self.badge.as_ref()
    }

    /// A buffer of its own (`show`, then `append`s), by id.
    pub fn view(&self, id: u64) -> Option<&View> {
        self.views.get(&id)
    }

    /// What it wrote to its own folder (`write-own`) as `name`.
    pub fn written_own(&self, name: &str) -> Option<&[u8]> {
        self.own.get(name).map(Vec::as_slice)
    }

    /// Its prompts waiting for an answer: (id, label).
    pub fn prompts(&self) -> &[(u64, String)] {
        &self.asks
    }

    /// Its pick up, if one is.
    pub fn pick(&self) -> Option<&Pick> {
        self.pick.as_ref()
    }

    /// The commands it ran (`run` actions: tarae runs movement commands only).
    pub fn ran(&self) -> &[String] {
        &self.ran
    }

    /// The files it opened.
    pub fn opened(&self) -> &[String] {
        &self.opened
    }

    /// The tools it offered to install.
    pub fn offered(&self) -> &[String] {
        &self.offered
    }

    // ── The host ──

    fn stamp(&self) -> Now {
        Now { millis: self.now, entropy: self.project.entropy.to_vec() }
    }

    /// What its grant reaches: its read patterns, the project, the files allowed on its card (the mock has no links:
    /// a path's real form is the path).
    fn reach(&self) -> rules::Reach {
        let mut reach = rules::Reach { files: self.project.allowed.clone(), ..rules::Reach::default() };
        for p in &self.permissions {
            match p {
                Permission::Read(pat) => {
                    let at = rules::expand(pat, Some(&self.project.home));
                    reach.patterns.push((at.clone(), at));
                }
                Permission::Project => reach.project = Some(self.project.root.clone()),
                _ => {}
            }
        }
        reach
    }

    fn reads(&self) -> bool {
        self.permissions.iter().any(|p| matches!(p, Permission::Read(_) | Permission::Project))
    }

    /// Whether its own grant covers the buffer in front (#457): its file, or a view of its own.
    fn covers(&self) -> bool {
        match &self.buf.path {
            Some(path) => self.reads() && self.reach().covers(None, path),
            None => self.buf.view.is_some(),
        }
    }

    /// The context a call gets: the buffer in front if it's given (`given`: your command, or its grant covers it),
    /// its text if asked for.
    fn context(&self, args: &str, yours: bool, text: bool) -> Context {
        let given = yours || self.covers();
        let now = self.stamp();
        if !given {
            let buffer = Buffer { path: None, name: String::new(), language: None, text: None, version: 0, modified: false };
            return Context { args: args.into(), buffer, selections: Vec::new(), primary: 0, now };
        }
        let b = &self.buf;
        let name = match (&b.path, b.view) {
            (Some(p), _) => p.strip_prefix(&self.project.root).unwrap_or(p).display().to_string(),
            (None, Some(id)) => self.views.get(&id).map(|v| v.title.clone()).unwrap_or_default(),
            (None, None) => "[scratch]".into(),
        };
        let selections = b
            .selections
            .iter()
            .map(|&(anchor, head)| {
                let (from, to) = (anchor.min(head), anchor.max(head));
                let text = b.text.get(from as usize..to as usize).unwrap_or_default().to_string();
                let line = b.text[..head as usize].matches('\n').count() as u64;
                Selection { anchor, head, from, to, text, line }
            })
            .collect();
        let buffer = Buffer {
            path: b.path.as_ref().map(|p| p.display().to_string()),
            name,
            language: b.language.clone(),
            text: text.then(|| b.text.clone()),
            version: b.version,
            modified: b.modified,
        };
        let primary = b.selections.len().saturating_sub(1) as u64;
        Context { args: args.into(), buffer, selections, primary, now }
    }

    /// An event on the buffer in front, to the plugin if it subscribed: only `shown`, empty, for a buffer its grant
    /// doesn't cover (#457); its languages filter the rest.
    fn fire(&mut self, event: Event) {
        let covers = self.covers();
        let lang = self.buf.language.clone();
        let sub = self.subscriptions.iter().find(|s| {
            s.event == event
                && match covers {
                    true => s.languages.is_empty() || lang.as_ref().is_some_and(|l| s.languages.contains(l)),
                    false => event == Event::Shown,
                }
        });
        let Some(text) = sub.map(|s| s.text) else { return };
        let cx = self.context("", false, text && covers);
        let actions = P::on(event, cx);
        self.settle(actions, false);
    }

    /// Applies an answer's actions, then hands back what they brought, round after round until nothing's pending.
    fn settle(&mut self, actions: Vec<Action>, yours: bool) {
        let mut actions = actions;
        let mut yours = yours;
        for _ in 0..ROUNDS {
            let mut back = Vec::new();
            for a in actions {
                self.act(a, yours, &mut back);
            }
            if back.is_empty() {
                return;
            }
            yours = false;
            actions = P::on_events(back, self.stamp());
        }
        panic!("{} answered {ROUNDS} rounds of events and never settled", self.name);
    }

    /// tarae's refusal: recorded (it fails the test unless expected), and its failure, if the action waits on one.
    fn refuse(&mut self, why: String, id: Option<u64>, back: &mut Vec<HostEvent>) {
        self.refused.push(why.clone());
        if let Some(id) = id {
            back.push(HostEvent::Failed((id, why)));
        }
    }

    /// Whether it may do what needs `need` — declared (approval: the load card taken as answered yes).
    fn may(&self, need: &Permission) -> Result<(), String> {
        match self.permissions.contains(need) {
            true => Ok(()),
            false => Err(rules::not_asked(perm(need))),
        }
    }

    fn act(&mut self, a: Action, yours: bool, back: &mut Vec<HostEvent>) {
        let hears = rules::hears_keys(&self.permissions.iter().map(perm).collect::<Vec<_>>());
        if let Some(what) = rules::keys_refused(kind(&a)).filter(|_| hears) {
            return self.refuse(rules::keys_refusal(what), waits_on(&a), back);
        }
        match a {
            Action::Status(m) => self.messages.push((Tone::Info, rules::shown(&m))),
            Action::Success(m) => self.messages.push((Tone::Ok, rules::shown(&m))),
            Action::Warning(m) => self.messages.push((Tone::Warning, rules::shown(&m))),
            Action::Error(m) => self.messages.push((Tone::Error, rules::shown(&m))),
            Action::StatusBar(m) => {
                let text: String = rules::shown(&m).replace('\n', " ");
                self.status_bar = (!text.trim().is_empty()).then_some(text);
            }
            Action::ReplaceSelections(texts) => {
                if self.may_edit(yours, back) {
                    if texts.len() != self.buf.selections.len() {
                        let n = self.buf.selections.len();
                        return self.messages.push((Tone::Error, format!("{} texts for {n} selections", texts.len())));
                    }
                    let edits = self.buf.selections.iter().zip(texts).map(|(&(a, h), insert)| (a.min(h), a.max(h), insert));
                    let edits: Vec<_> = edits.collect();
                    self.apply(edits);
                }
            }
            Action::Edits(edits) => {
                if self.may_edit(yours, back) {
                    self.apply(edits.into_iter().map(|e| (e.from, e.to, e.insert)).collect());
                }
            }
            Action::Select(ranges) => {
                if self.may_edit(yours, back) {
                    self.buf.selections = ranges;
                }
            }
            Action::Run(name) => self.ran.push(name),
            Action::Open(path) => self.open_for(&path, back),
            Action::Spawn(sp) => {
                let r = match rules::not_a_program(&sp.program) {
                    Some(why) => Err(why),
                    None => self.may(&Permission::Run(sp.program.clone())),
                };
                if let Err(why) = r {
                    return self.refuse(why, Some(sp.id), back);
                }
                match self.project.programs.get(&sp.program) {
                    Some(s) => {
                        back.push(HostEvent::Lines((sp.id, s.lines.clone())));
                        back.push(HostEvent::Exit((sp.id, s.code, s.stderr.clone())));
                    }
                    None => back.push(HostEvent::Failed((sp.id, format!("{}: not found (no Project::program)", sp.program)))),
                }
            }
            Action::Connect(c) => {
                let r = self.may(&Permission::Network).map_err(Some).and_then(|()| match rules::connect_form(&c.host, c.port) {
                    Some(why) => Err(Some(why)),
                    None => Ok(()),
                });
                match r {
                    Err(Some(why)) => self.refuse(why, Some(c.id), back),
                    Err(None) => {}
                    Ok(()) => {
                        let target = rules::target(&c.host, c.port);
                        let Some(replies) = self.project.hosts.get(&target) else {
                            let (title, question) = rules::ask_connect(&self.name, &target);
                            return self.refuse(format!("{title} {question}"), None, back);
                        };
                        back.push(HostEvent::Connected(c.id));
                        if let Some(g) = &replies.greeting {
                            back.push(HostEvent::Data((c.id, g.clone())));
                        }
                        self.connections.insert(c.id, target);
                    }
                }
            }
            Action::Send((id, bytes)) => {
                let replies = self.connections.get(&id).and_then(|t| self.project.hosts.get(t));
                match replies {
                    None => back.push(HostEvent::Failed((id, "no open connection with that id".into()))),
                    Some(r) => {
                        if let Some((_, reply)) = r.on.iter().find(|(sent, _)| *sent == bytes) {
                            back.push(HostEvent::Data((id, reply.clone())));
                        }
                    }
                }
            }
            Action::Stop(id) => {
                self.timers.remove(&id);
                if self.connections.remove(&id).is_some() {
                    back.push(HostEvent::Closed((id, String::new())));
                }
            }
            Action::ReadFile((id, path)) => self.read_for(id, &path, back),
            Action::ReadEnv((id, name)) => match self.may(&Permission::Env(name.clone())) {
                Ok(()) => back.push(HostEvent::Env((id, self.project.env.get(&name).cloned()))),
                Err(why) => self.refuse(why, Some(id), back),
            },
            Action::Timer((id, ms)) => {
                self.timers.insert(id, self.now + ms.min(86_400_000));
            }
            Action::Panel(p) => self.panel = p,
            Action::Prompt((id, label)) => {
                self.asks.retain(|(i, _)| *i != id);
                self.asks.push((id, rules::shown(&label)));
            }
            Action::Show(view) => {
                self.buf = Buf { text: view.text.clone(), view: Some(view.id), language: view.language.clone(), ..Buf::default() };
                self.views.insert(view.id, view);
            }
            Action::Append((id, text)) => match self.views.get_mut(&id) {
                Some(v) => {
                    v.text.push_str(&text);
                    if self.buf.view == Some(id) {
                        self.buf.text.push_str(&text);
                    }
                }
                None => self.messages.push((Tone::Error, format!("no buffer of its own with id {id}"))),
            },
            Action::Hints(h) => {
                self.hints.insert(h.path.clone(), h);
            }
            Action::Side(s) => self.side = s,
            Action::Pick(p) => self.pick = Some(p),
            Action::Badge(b) => self.badge = b,
            Action::Find((id, pattern)) => {
                let why = self.may(&Permission::Project).err().or_else(|| rules::find_pattern(&pattern));
                if let Some(why) = why {
                    return self.refuse(why, Some(id), back);
                }
                let root = &self.project.root;
                let whole = root.join(&pattern);
                let found = (self.project.files.keys())
                    .filter(|p| p.starts_with(root) && rules::path_matches_in(&whole, p) && !rules::secretnames::named_secret(p))
                    .take(2000)
                    .map(|p| p.display().to_string())
                    .collect();
                back.push(HostEvent::Found((id, found)));
            }
            Action::WriteOwn((id, name, bytes)) => match rules::own_file(&name, bytes.len()) {
                Err(why) => self.refuse(why, Some(id), back),
                Ok(()) => {
                    back.push(HostEvent::Written((id, format!("{OWN}/{}/{name}", self.name))));
                    self.own.insert(name, bytes);
                }
            },
            Action::OfferInstall((_, what)) => self.offered.push(what),
            Action::Overlay(o) => self.overlay = o,
        }
    }

    /// Whether an edit or a move of its applies to the buffer in front: in answer to your command, or in a buffer its
    /// grant covers — else refused, in tarae's words.
    fn may_edit(&mut self, yours: bool, back: &mut Vec<HostEvent>) -> bool {
        if yours || self.covers() {
            return true;
        }
        let why = "it edits only a file it may read, or in answer to your command — its edits weren't applied";
        self.refuse(why.into(), None, back);
        false
    }

    /// Edits (from, to, insert) to the buffer in front, as one step; then `changed`.
    fn apply(&mut self, mut edits: Vec<(u64, u64, String)>) {
        // (from the end back, so each edit's offsets still hold)
        edits.sort();
        edits.reverse();
        let text = &self.buf.text;
        let fits = edits.iter().all(|(f, t, _)| f <= t && text.is_char_boundary(*f as usize) && text.is_char_boundary(*t as usize));
        if !fits {
            return self.messages.push((Tone::Error, "its edits don't fit the buffer — not applied".into()));
        }
        for (from, to, insert) in edits {
            self.buf.text.replace_range(from as usize..to as usize, &insert);
        }
        self.buf.version += 1;
        self.buf.modified = true;
        self.fire(Event::Changed);
    }

    /// A `read-file`: the file's bytes if its grant reaches it, a card if it reads files at all, else its refusal.
    fn read_for(&mut self, id: u64, path: &str, back: &mut Vec<HostEvent>) {
        let Some(at) = rules::expand(path, Some(&self.project.home)) else {
            return self.refuse(rules::not_absolute(path), Some(id), back);
        };
        if !self.reads() {
            return self.refuse(rules::NO_READS.into(), Some(id), back);
        }
        if !self.reach().reaches(Some(&at), &at) {
            let (title, question) = rules::ask_read(&self.name, &at);
            return self.refuse(format!("{title} {question}"), None, back);
        }
        back.push(match self.project.files.get(&at) {
            Some(bytes) => HostEvent::File((id, bytes.clone())),
            None => HostEvent::Failed((id, format!("{}: No such file or directory (os error 2)", at.display()))),
        });
    }

    /// An `open`: only a file its grant covers (#457), put in front.
    fn open_for(&mut self, path: &str, back: &mut Vec<HostEvent>) {
        if path.chars().any(char::is_control) {
            return self.messages.push((Tone::Error, "a path with control characters".into()));
        }
        // (`path:line`, from 1: there, if the whole isn't a file)
        let (file, line) = match path.rsplit_once(':') {
            Some((f, l)) if !self.project.files.contains_key(&self.project.path(path)) => (f, l.parse::<u64>().ok()),
            _ => (path, None),
        };
        let at = self.project.path(file);
        if !(self.reads() && self.reach().covers(Some(&at), &at)) {
            return self.refuse(rules::may_not_open(path), None, back);
        }
        self.opened.push(path.into());
        if self.project.files.contains_key(&at) {
            self.open(&at.display().to_string());
            if let Some(l) = line.filter(|l| *l > 0) {
                let at = self.buf.text.split_inclusive('\n').take(l as usize - 1).map(str::len).sum::<usize>() as u64;
                self.buf.selections = vec![(at, at)];
            }
        }
    }
}

impl<P: Guest> Drop for Host<P> {
    /// The refusals no `expect_refused` acknowledged fail the test — but not while a failure is already unwinding
    /// (a panic in a panic aborts, and hides the first).
    fn drop(&mut self) {
        if !self.refused.is_empty() && !std::thread::panicking() {
            let list: Vec<String> = self.refused.iter().map(|r| format!("  - {r}")).collect();
            panic!("tarae would refuse {} here:\n{}\n(expected? `expect_refused`)", self.name, list.join("\n"));
        }
    }
}

/// A permission as the shared rules read it.
fn perm(p: &Permission) -> rules::Perm<'_> {
    match p {
        Permission::Run(prog) => rules::Perm::Run(prog),
        Permission::Read(pat) => rules::Perm::Read(pat),
        Permission::Env(name) => rules::Perm::Env(name),
        Permission::Network => rules::Perm::Network,
        Permission::Project => rules::Perm::Project,
        Permission::Keys => rules::Perm::Keys,
    }
}

/// An action's kind, for the shared rules (the match is whole: a new action is one more `Act`).
fn kind(a: &Action) -> rules::Act {
    use rules::Act;
    match a {
        Action::Status(_) => Act::Status,
        Action::Success(_) => Act::Success,
        Action::Warning(_) => Act::Warning,
        Action::Error(_) => Act::Error,
        Action::ReplaceSelections(_) => Act::ReplaceSelections,
        Action::Edits(_) => Act::Edits,
        Action::Select(_) => Act::Select,
        Action::Run(_) => Act::Run,
        Action::Open(_) => Act::Open,
        Action::StatusBar(_) => Act::StatusBar,
        Action::Spawn(_) => Act::Spawn,
        Action::Connect(_) => Act::Connect,
        Action::Send(_) => Act::Send,
        Action::Stop(_) => Act::Stop,
        Action::ReadFile(_) => Act::ReadFile,
        Action::ReadEnv(_) => Act::ReadEnv,
        Action::Timer(_) => Act::Timer,
        Action::Panel(_) => Act::Panel,
        Action::Prompt(_) => Act::Prompt,
        Action::Show(_) => Act::Show,
        Action::Append(_) => Act::Append,
        Action::Hints(_) => Act::Hints,
        Action::Side(_) => Act::Side,
        Action::Pick(_) => Act::Pick,
        Action::Badge(_) => Act::Badge,
        Action::Find(_) => Act::Find,
        Action::WriteOwn(_) => Act::WriteOwn,
        Action::OfferInstall(_) => Act::OfferInstall,
        Action::Overlay(_) => Act::Overlay,
    }
}

/// The request id an action waits on an answer for, if it does.
fn waits_on(a: &Action) -> Option<u64> {
    match a {
        Action::Spawn(s) => Some(s.id),
        Action::Connect(c) => Some(c.id),
        Action::Pick(p) => Some(p.id),
        Action::Prompt((id, _))
        | Action::WriteOwn((id, ..))
        | Action::OfferInstall((id, _))
        | Action::ReadFile((id, _))
        | Action::ReadEnv((id, _))
        | Action::Find((id, _)) => Some(*id),
        _ => None,
    }
}

#[cfg(test)]
mod tests;

/// A file's language from its extension, in tarae's names (a test sets another with `Host::language`).
fn language(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_str()?;
    let name = match ext {
        "rs" => "rust",
        "toml" => "toml",
        "md" => "markdown",
        "yaml" | "yml" => "yaml",
        "json" => "json",
        "py" => "python",
        "go" => "go",
        "js" => "javascript",
        "ts" => "typescript",
        "sql" => "sql",
        "java" => "java",
        _ => return None,
    };
    Some(name.into())
}
