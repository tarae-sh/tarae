//! The attach socket (the remote plan's §8b, step 3): a host (`tarae --host`) drawn for an interface across a socket
//! — `tarae attach` (`client.rs`) in a terminal of its own. `$state/ui/<pid>.attach`, bound with the state socket's
//! guards (`uisocket::bind_named`: the folder 0700 and this user's, the socket 0600, bound before any thread runs). A
//! peer must be this user, say `Hello` within `HELLO_WITHIN` in this build's words (`v` and `build`), and — judged on
//! the main loop, which knows them — run under none of Claude's processes nor the host's own terminal panel (it would
//! draw the host inside itself). One interface at a time: another attach takes over, the first told so.
//!
//! The main loop never waits on it (`Wire`): a frame drawn goes whole into the writer's mailbox (`tarae_wire::Mailbox`:
//! an unsent one is replaced, so a slow interface skips frames, never one sent), and the writer thread finds what
//! changed against the last frame it *sent* (`Writer`), at most `IN_FLIGHT` unacked. The reader thread turns the
//! interface's input into `Event::Input` (its `seen` is what the "only once drawn" rules judge it by), an ack into
//! credit at once and into `Editor::frame_presented` on the loop. An interface attaching or going is swapped in or out
//! between frames (`swap`); "in sight now" starts over at an attach (`Editor.attached_at`).

use std::io::{self, Write};
use std::net::Shutdown;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, OnceLock, mpsc};
use std::time::Duration;

use tarae_wire::{self as wire, Decoder, Gone, Hello, Mailbox, Next, Order, ToHost, ToInterface};

use crate::editor::Editor;
use crate::event::{Event, Tx};
use crate::interface::{Facts, Interface, Link, Whole};
use crate::screen::Screen;

/// `Hello` comes within this, or the connection goes.
const HELLO_WITHIN: Duration = Duration::from_secs(5);
/// Bytes of messages queued and not sent past which the interface is taken for gone: it reads nothing.
const PENDING_MAX: usize = 16 << 20;
/// A write the interface takes nothing of for this long, and it's taken for gone (the writer's only wait).
const WRITE_WITHIN: Duration = Duration::from_secs(30);

/// This host's attach socket, once bound.
static BOUND: OnceLock<PathBuf> = OnceLock::new();

/// Host `pid`'s attach socket.
pub fn path_for(pid: u32) -> Option<PathBuf> {
    crate::uisocket::path_for(pid).map(|p| p.with_extension("attach"))
}

/// Binds this host's attach socket — at its start, before any thread runs (the umask is the process's a moment).
pub fn bind() -> Result<UnixListener, String> {
    let pid = std::process::id();
    let path = path_for(pid).ok_or("no state folder (neither XDG_STATE_HOME nor HOME)")?;
    let l = crate::uisocket::bind_named(path.parent().expect("in ui/"), &format!("{pid}.attach"))?;
    let _ = BOUND.set(path);
    Ok(l)
}

/// The socket goes with this host (`:restart`'s new binary binds it again at the same path: the pid stays).
pub fn remove() {
    if let Some(p) = BOUND.get() {
        let _ = std::fs::remove_file(p);
    }
}

/// Attaches heard: a thread in `accept`, and one a connection until its `Hello` is in.
pub fn serve(listener: UnixListener, tx: Tx) {
    crate::event::thread("tarae-attach", move || {
        for stream in listener.incoming().flatten() {
            let tx = tx.clone();
            crate::event::thread("tarae-hello", move || hello(stream, tx));
        }
    });
}

/// What's under way between frames: an interface attaching, one going, and the one attached.
#[derive(Default)]
pub struct Pending {
    /// Attached, swapped in after the batch (`swap`) — a newer one takes its place.
    pub coming: Option<Interface>,
    /// The interface (by its number) to let go, and what it's told — None: it's gone already, nobody to tell.
    pub going: Option<(u64, Option<(Gone, String)>)>,
    /// The interface's number drawn for now (0: none, or the terminal tarae runs in).
    pub current: u64,
    /// Numbers given out.
    given: u64,
    /// The last `Input.seq` of the interface attached now that was handled (`Frame.input`).
    pub input: u64,
    /// Messages for an interface (by its number), handed to its link once the loop is between frames (`flush`).
    pub out: Vec<(u64, ToInterface, Order)>,
    /// What was asked of an interface and not answered yet (`ask`), oldest first.
    asked: Vec<Waiting>,
    /// `Ask` ids given out.
    asks: u32,
}

/// What a question put to the interface was for: what its answer goes on to (`answered`).
pub enum Asked {
    /// A copy (`clipboard::copy_flashing`): what it copied, a yank's flash.
    Copy { what: String, flash: Option<crate::motion::What> },
    /// A paste (`clipboard::paste`): what takes the text.
    Paste(Takes),
    /// `:web-open`: the address handed over.
    Open { url: String },
}

/// What takes a paste's text, on the main loop.
pub type Takes = Box<dyn FnOnce(&mut Editor, String) + Send>;

impl Asked {
    /// What it asked, in words.
    pub fn said(&self) -> String {
        match self {
            Asked::Copy { what, .. } => format!("the copy of {what}"),
            Asked::Paste(_) => "the paste".into(),
            Asked::Open { url } => format!("opening {url}"),
        }
    }
}

struct Waiting {
    id: u32,
    /// The interface asked (`Interface.id`).
    iface: u64,
    asked: Asked,
}

/// How long an interface's answer is waited for: past it, the question is said unanswered and let go (a late answer
/// then goes nowhere, logged).
pub const ANSWER_WITHIN: Duration = Duration::from_secs(10);

/// `q` put to the interface attached now (its clipboard, its browser), after the next frame; `asked` kept for its
/// answer (`answered`). Unanswered within `ANSWER_WITHIN`, or the interface gone first (detached, taken over), it's
/// said and let go — never carried over to another interface: the answer can outlive the one asked, never reach the
/// next.
pub fn ask(ed: &mut Editor, q: wire::Ask, asked: Asked) {
    let iface = ed.attach.current;
    ed.attach.asks = ed.attach.asks.wrapping_add(1);
    let id = ed.attach.asks;
    ed.attach.out.push((iface, ToInterface::Ask { id, ask: q }, Order::AfterFrame));
    ed.attach.asked.push(Waiting { id, iface, asked });
    ed.events.jobs().spawn(move || {
        crate::event::sleep(ANSWER_WITHIN);
        move |ed: &mut Editor| {
            ed.drawn_for("an interface's answer waited out");
            let why = format!("the interface didn't answer within {} s", ANSWER_WITHIN.as_secs());
            unanswered(ed, |w| w.iface == iface && w.id == id, &why);
        }
    });
}

/// The questions `which` picks let go, each said: nothing came of them that tarae knows of (`why`).
fn unanswered(ed: &mut Editor, which: impl Fn(&Waiting) -> bool, why: &str) {
    let (gone, kept) = std::mem::take(&mut ed.attach.asked).into_iter().partition(|w| which(w));
    ed.attach.asked = kept;
    let gone: Vec<Waiting> = gone;
    if gone.is_empty() {
        ed.unchanged = true;
    }
    for w in gone {
        crate::log::line("attach", "info", &format!("{} let go: {why}", w.asked.said()));
        ed.set_warning(match w.asked {
            Asked::Copy { what, .. } => format!("not known whether {what} was copied — {why}"),
            Asked::Paste(_) => format!("nothing pasted — {why}"),
            Asked::Open { url } => format!("web: not known whether {url} opened — {why}"),
        });
    }
}

/// Interface `iface`'s answer to question `id`, on the main loop: what it was for goes on (`clipboard::answered`,
/// `:web-open`'s words). One no longer waited for (said unanswered already) is only logged.
pub(crate) fn answered(ed: &mut Editor, iface: u64, id: u32, answer: wire::Answer) {
    ed.drawn_for("an interface's answer");
    let Some(i) = ed.attach.asked.iter().position(|w| w.iface == iface && w.id == id) else {
        ed.unchanged = true;
        let said = format!("an answer no one waits for any more (ask {id}): {answer:?}");
        return crate::log::line("attach", "info", &said);
    };
    match (ed.attach.asked.remove(i).asked, answer) {
        (Asked::Open { url }, wire::Answer::Opened) => {
            ed.set_status(format!("web: {url} handed to your browser"))
        }
        (Asked::Open { .. }, wire::Answer::Failed(e)) => ed.set_error(format!("web: {e}")),
        (asked @ Asked::Open { .. }, answer) => {
            ed.set_error(format!("the interface answered {} with {answer:?}", asked.said()))
        }
        (asked, answer) => crate::clipboard::answered(ed, asked, answer),
    }
}

/// Between frames: what's queued for the interface handed to its link, behind the frame just drawn — what was for
/// one gone dropped (its questions were said unanswered as it went: `swap`).
pub fn flush(ed: &mut Editor, iface: &mut Interface) {
    for (to, m, order) in std::mem::take(&mut ed.attach.out) {
        if to == iface.id {
            iface.link.say(m, order);
        }
    }
}

/// The interface's zone moved (its `Zone`): the times shown are told by its offset from the next frame.
fn zone(ed: &mut Editor, iface: u64, offset: i32) {
    ed.drawn_for("an interface's zone");
    let z = crate::log::Zone::Offset(offset);
    if ed.attach.current != iface || ed.interface.zone == z {
        ed.unchanged = true;
        return;
    }
    crate::log::line("attach", "info", &format!("the interface's zone moved: {offset} s east of UTC"));
    ed.interface.zone = z;
}

/// A message's bytes on `s`, as one write (a refusal, said before anything else could be).
fn say(mut s: &UnixStream, m: &ToInterface) {
    let mut out = Vec::new();
    if wire::encode(m, &mut out).is_ok() {
        let _ = s.write_all(&out);
    }
}

fn refused(said: String) -> ToInterface {
    ToInterface::Gone { why: Gone::Refused { said }, said: String::new() }
}

/// A connection until its `Hello`: this user's only, in this build's words; then on to the main loop (`arrive`).
fn hello(stream: UnixStream, tx: Tx) {
    let (uid, pid) = match crate::uisocket::peer_cred(&stream) {
        Ok(p) => p,
        Err(e) => return say(&stream, &refused(format!("can't tell who's attaching: {e}"))),
    };
    // SAFETY: geteuid only reads.
    if uid != unsafe { libc::geteuid() } {
        return say(&stream, &refused("another user".into()));
    }
    let _ = stream.set_read_timeout(Some(HELLO_WITHIN));
    let mut dec = Decoder::<ToHost>::new();
    let hello = loop {
        match dec.message() {
            Ok(Some(ToHost::Hello(h))) => break h,
            Ok(Some(_)) => {
                let said = "its first message wasn't Hello".to_string();
                return say(
                    &stream,
                    &ToInterface::Gone { why: Gone::Protocol { said }, said: String::new() },
                );
            }
            Err(e) => {
                return say(
                    &stream,
                    &ToInterface::Gone { why: Gone::Protocol { said: e.said }, said: String::new() },
                );
            }
            Ok(None) => match dec.read_from(&mut &stream) {
                Ok(0) => return,
                Ok(_) => {}
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => {
                    return crate::log::line("attach", "info", &format!("pid {pid}: no Hello ({e})"));
                }
            },
        }
    };
    let _ = stream.set_read_timeout(None);
    let build = env!("CARGO_PKG_VERSION");
    if hello.v != wire::V || hello.build != build {
        let said = format!(
            "this host is tarae {build} (protocol {}), the attach tarae {} (protocol {}) — attach with the host's own build",
            wire::V,
            hello.build,
            hello.v
        );
        return say(&stream, &refused(said));
    }
    // (the peer and its parents — judged on the main loop, which knows Claude's processes and the terminal panels')
    let line = crate::uisocket::ancestors(pid);
    let job = move |ed: &mut Editor| arrive(ed, stream, hello, line, dec);
    let _ = tx.send(Event::Job(Box::new(job)));
}

/// An attach with its `Hello` in, on the main loop: refused under one of Claude's processes or the host's own terminal
/// panel, else its interface made and swapped in after the batch (`swap`).
fn arrive(ed: &mut Editor, stream: UnixStream, hello: Hello, line: Vec<u32>, dec: Decoder<ToHost>) {
    ed.drawn_for("an attach");
    // (nothing on screen changes until it's swapped in)
    ed.unchanged = true;
    let claude = ed.claude_pids();
    let shells: Vec<u32> = std::iter::once(&ed.workspace)
        .chain(&ed.workspaces)
        .filter_map(|w| w.terminal.as_ref().and_then(|t| t.pid()))
        .collect();
    let why = if let Some(p) = line.iter().find(|p| claude.contains(p)) {
        Some(format!("under a process of Claude's (pid {p}) — Claude has a view of its own"))
    } else {
        line.iter().find(|p| shells.contains(p)).map(|p| {
            format!(
                "from inside this host's own terminal panel (pid {p}) — it would draw the host inside itself; \
                 attach from another terminal"
            )
        })
    };
    if let Some(why) = why {
        crate::log::line("attach", "info", &format!("refused (pid {}): {why}", line[0]));
        let gone = refused(why);
        // (a few bytes on a fresh socket — written off the loop all the same)
        ed.events.jobs().detached(move || say(&stream, &gone));
        return;
    }
    ed.attach.given += 1;
    let id = ed.attach.given;
    let welcome = ToInterface::Welcome {
        v: wire::V,
        build: env!("CARGO_PKG_VERSION").into(),
        pid: std::process::id(),
        project: ed.workspace.root.display().to_string(),
    };
    match Wire::start(stream, id, dec, (hello.w, hello.h), welcome, ed.events.shared_sender()) {
        Ok(link) => {
            crate::log::line(
                "attach",
                "info",
                &format!(
                    "an interface attached (pid {}, {}×{}): it's drawn for from the next frame",
                    line[0], hello.w, hello.h
                ),
            );
            let mut iface = Interface::new(facts_of(&hello), Box::new(link));
            iface.id = id;
            ed.attach.coming = Some(iface);
        }
        Err(e) => crate::log::line("attach", "warn", &format!("an attach dropped: {e}")),
    }
}

/// What an interface says of itself in its `Hello`, as the editor reads it: its clipboard, speaker and browser are its
/// machine's, asked across the wire; the times it shows are told by its offset.
fn facts_of(h: &Hello) -> Facts {
    use crate::clipboard::Clip;
    use crate::interface::{Opener, Speaker};
    Facts {
        light: match h.ground {
            wire::Ground::Unsaid => None,
            wire::Ground::Dark => Some(false),
            wire::Ground::Light => Some(true),
        },
        undercurl: h.undercurl,
        zellij: h.zellij,
        clipboard: match h.clipboard {
            wire::Clip::None => Clip::None,
            wire::Clip::CopyAndPaste => Clip::Wire { paste: true },
            wire::Clip::CopyOnly => Clip::Wire { paste: false },
        },
        sound: if h.sound { Speaker::Wire } else { Speaker::None },
        opener: if h.open { Opener::Wire } else { Opener::None },
        zone: crate::log::Zone::Offset(h.offset),
        presents_late: true,
    }
}

/// Between frames (`term::event_loop`): the interface going let go (told why, if it's there to hear), the one attaching
/// swapped in — the one there before told it was taken over. Whether a frame is owed (one attached).
pub fn swap(ed: &mut Editor, iface: &mut Option<Interface>) -> bool {
    if let Some((id, gone)) = ed.attach.going.take() {
        if ed.attach.coming.as_ref().is_some_and(|i| i.id == id) {
            ed.attach.coming = None;
        }
        if iface.as_ref().is_some_and(|i| i.id == id && id > 0) {
            let mut old = iface.take().expect("there");
            if let Some((gone, said)) = gone {
                old.link.bye(gone, said);
            }
            drop(old);
            asks_gone(ed, id);
            ed.attach.current = 0;
            ed.detached = true;
            ed.interface = crate::term::no_interface();
            crate::log::line("attach", "info", "detached: nothing is drawn until an interface attaches");
        }
    }
    let Some(mut new) = ed.attach.coming.take() else { return false };
    if let Some(mut old) = iface.take() {
        old.link.bye(Gone::TakenOver, String::new());
        crate::log::line("attach", "info", "taken over: the interface attached before was told");
        asks_gone(ed, old.id);
    }
    // (a wire's: nothing to make ready — its client did)
    let _ = new.link.begin();
    new.link.listen(ed.events.shared_sender());
    ground(ed, new.facts.light);
    ed.interface = new.facts.clone();
    ed.detached = false;
    ed.attach.current = new.id;
    ed.attach.input = 0;
    // (what an interface before it presented counts no more — decision D)
    ed.attached_at = ed.frames;
    // (the frames kept were the one before's to show)
    ed.kept.clear();
    ed.press_refused = false;
    ed.drawn_by.push(crate::editor::DrawnFor::Named("an attach"));
    *iface = Some(new);
    true
}

/// Interface `iface` let go (detached, taken over): what it was asked and hasn't answered said and let go — its
/// answer, if one still comes, goes nowhere.
pub(crate) fn asks_gone(ed: &mut Editor, iface: u64) {
    unanswered(ed, |w| w.iface == iface, "the interface asked went before it answered");
}

/// The interface's ground as the theme's: `default` is picked again (hanji on a light one, else meok) when it differs.
fn ground(ed: &mut Editor, light: Option<bool>) {
    let Some(light) = light.filter(|&l| l != crate::theme::is_light()) else { return };
    crate::theme::set_light(light);
    if ed.config.theme != "default" {
        return;
    }
    ed.events.jobs().spawn(|| {
        let t = crate::theme::load("default");
        move |ed: &mut Editor| match t {
            Ok(t) if ed.config.theme == "default" => ed.theme = t,
            Ok(_) => ed.unchanged = true,
            Err(e) => ed.set_error(e),
        }
    });
}

/// `:detach`: the interface attached let go, told so (its client restores its terminal and says how to come back);
/// the host works on with nothing drawn.
pub fn detach(ed: &mut Editor) -> Result<(), String> {
    if !ed.host {
        return Err(":detach needs tarae --host — this tarae draws in the terminal it runs in".into());
    }
    if ed.attach.current == 0 {
        return Err("nothing is attached".into());
    }
    ed.attach.going = Some((ed.attach.current, Some((Gone::Detached, String::new()))));
    Ok(())
}

/// Interface `id`'s link ended (`why`; `tell`: what it's told, if it can still hear): let go after the batch.
fn lost(tx: &Tx, id: u64, why: String, tell: Option<Gone>) {
    let job = move |ed: &mut Editor| {
        ed.drawn_for("an interface gone");
        ed.unchanged = true;
        if ed.attach.current == id || ed.attach.coming.as_ref().is_some_and(|i| i.id == id) {
            crate::log::line("attach", "info", &format!("the interface went ({why})"));
            ed.attach.going = Some((id, tell.map(|g| (g, String::new()))));
        }
    };
    let _ = tx.send(Event::Job(Box::new(job)));
}

/// The interface presented frame `n` (its `Ack`) — what waited for it counts now. Nothing drawn for it.
fn presented(ed: &mut Editor, id: u64, n: u64) {
    ed.drawn_for("an interface's ack");
    ed.unchanged = true;
    if ed.attach.current == id {
        ed.frame_presented(n);
    }
}

/// What the main loop, the reader and the writer share of one interface.
struct Shared {
    mail: Mailbox<Whole, Out>,
    /// Its size in cells: `w << 16 | h`.
    size: AtomicU32,
    /// Its input still goes to the editor — false once it's let go (what's read after is dropped).
    live: AtomicBool,
    /// The next frame sent goes whole: one asked for it (`Whole.unknown`), and the frame that said so may be
    /// replaced in the mailbox before it went.
    unknown: AtomicBool,
    id: u64,
}

/// A message queued for the writer, and when a sound to play was decided (`at`): one held up past `PLAY_LATE` (a
/// socket that took nothing a while) would ring late, and goes unsent.
struct Out {
    m: ToInterface,
    at: Option<std::time::Instant>,
}

/// How late a sound may still go out (the remote plan's §8b).
const PLAY_LATE: Duration = Duration::from_millis(500);

/// An interface across the attach socket, as the main loop's link (`interface::Link`): every method returns at once.
pub struct Wire {
    shared: Arc<Shared>,
    stream: UnixStream,
    /// What the reader starts with (`listen`).
    reader: Option<(UnixStream, Decoder<ToHost>)>,
    /// Told once the writer ended (everything queued went, or the interface went).
    done: mpsc::Receiver<()>,
    /// Its last words were said (`bye`).
    said: bool,
}

impl Wire {
    /// The link for an interface that said `Hello` on `stream` (`dec` holds what it sent after it): its writer
    /// started, `welcome` the first thing it's sent.
    fn start(
        stream: UnixStream,
        id: u64,
        dec: Decoder<ToHost>,
        (w, h): (u16, u16),
        welcome: ToInterface,
        tx: Tx,
    ) -> io::Result<Wire> {
        let shared = Arc::new(Shared {
            mail: Mailbox::new(),
            size: AtomicU32::new(pack(w, h)),
            live: AtomicBool::new(true),
            unknown: AtomicBool::new(false),
            id,
        });
        shared.mail.with(|s| s.push(Out { m: welcome, at: None }, Order::AtOnce, 0));
        let out = stream.try_clone()?;
        out.set_write_timeout(Some(WRITE_WITHIN))?;
        let reader = stream.try_clone()?;
        let (ended, done) = mpsc::channel();
        let sh = Arc::clone(&shared);
        crate::event::thread("tarae-wire-out", move || {
            write_loop(&sh, out, &tx);
            let _ = ended.send(());
        });
        Ok(Wire { shared, stream, reader: Some((reader, dec)), done, said: false })
    }

    /// `m` queued to go by `order` — past `PENDING_MAX` unsent, the interface is taken for gone (it reads nothing).
    fn push(&self, m: ToInterface, order: Order) -> bool {
        let bytes = match &m {
            ToInterface::Gone { said, .. } => said.len(),
            ToInterface::Ask { ask: wire::Ask::Copy(t) | wire::Ask::Open(t), .. } => t.len(),
            _ => 0,
        } + 16;
        let at = matches!(m, ToInterface::Sound(wire::Sound::Play { .. })).then(crate::clock::now);
        let m = Out { m, at };
        self.shared.mail.with(|s| {
            if s.pending() + bytes > PENDING_MAX {
                s.close();
                return false;
            }
            s.push(m, order, bytes);
            true
        })
    }
}

fn pack(w: u16, h: u16) -> u32 {
    (w as u32) << 16 | h as u32
}

impl Link for Wire {
    fn begin(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn end(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn size(&mut self) -> io::Result<(u16, u16)> {
        let s = self.shared.size.load(Ordering::Relaxed);
        Ok(((s >> 16) as u16, s as u16))
    }

    fn listen(&mut self, tx: Tx) {
        let Some((s, dec)) = self.reader.take() else { return };
        let sh = Arc::clone(&self.shared);
        crate::event::thread("tarae-wire-in", move || read_loop(&sh, s, dec, &tx));
    }

    fn write(&mut self, _frame: &[u8], _own: &[u8]) -> io::Result<()> {
        Err(io::Error::other("an attached interface takes whole frames (`hand`)"))
    }

    fn whole(&self) -> bool {
        true
    }

    fn hand(&mut self, f: Whole) -> io::Result<Option<Vec<u8>>> {
        if f.unknown {
            self.shared.unknown.store(true, Ordering::Relaxed);
        }
        let back = self.shared.mail.with(|s| s.put_frame(f.n, f).or_else(|| s.take_spare()));
        Ok(back.map(|w| w.bytes))
    }

    fn bye(&mut self, gone: Gone, said: String) {
        if std::mem::replace(&mut self.said, true) {
            return;
        }
        self.shared.live.store(false, Ordering::Relaxed);
        if self.push(ToInterface::Gone { why: gone, said }, Order::AtOnce) {
            self.shared.mail.with(|s| s.close());
        }
    }

    fn flushed(&mut self, within: Duration) {
        let _ = self.done.recv_timeout(within);
    }

    fn say(&mut self, m: ToInterface, order: Order) {
        if !self.said {
            self.push(m, order);
        }
    }
}

impl Drop for Wire {
    /// Let go with nothing more to say: what's queued still goes (its writer shuts the socket after), the reader ends
    /// at once.
    fn drop(&mut self) {
        self.shared.live.store(false, Ordering::Relaxed);
        self.shared.mail.with(|s| s.close());
        let _ = self.stream.shutdown(Shutdown::Read);
    }
}

/// The writer: what the mailbox lets go next, made bytes (`Writer::step`) and written — the only place that waits on
/// the interface. Once closed, or the interface gone, the socket is shut.
fn write_loop(sh: &Shared, mut out: UnixStream, tx: &Tx) {
    let mut w = Writer::new();
    let mut bytes = Vec::with_capacity(1 << 16);
    loop {
        let next = match sh.mail.wait() {
            Next::Frame(n, mut f) => {
                f.unknown |= sh.unknown.swap(false, Ordering::Relaxed);
                Next::Frame(n, f)
            }
            Next::Msg(Out { at: Some(at), .. }) if crate::clock::since(at) > PLAY_LATE => continue,
            Next::Msg(o) => Next::Msg(o.m),
            Next::Wait => Next::Wait,
            Next::Closed => Next::Closed,
        };
        let closed = matches!(next, Next::Closed);
        if let Some(back) = w.step(next, &mut bytes) {
            sh.mail.with(|s| s.give_back(back));
        }
        if let Err(e) = out.write_all(&bytes) {
            sh.mail.with(|s| s.close());
            lost(tx, sh.id, format!("writing to it: {e}"), None);
            break;
        }
        bytes.clear();
        if closed {
            break;
        }
    }
    let _ = out.shutdown(Shutdown::Both);
}

/// What an interface's writer sends: each frame against the last one it sent (what the interface holds once it reads
/// on — never the last acked: on a stream nothing is lost, and a cell changed and changed back between frames it
/// skipped would stay wrong), the other messages as they are. A pure step: the simulation drives it with no thread.
pub struct Writer {
    screen: Screen,
}

impl Writer {
    pub fn new() -> Writer {
        Writer { screen: Screen::new() }
    }

    /// `next`'s bytes on the end of `out`; a frame's buffer comes back, for the loop's next frame.
    pub fn step(&mut self, next: Next<Whole, ToInterface>, out: &mut Vec<u8>) -> Option<Whole> {
        let (m, back) = match next {
            Next::Frame(n, mut f) => {
                if f.unknown {
                    self.screen.invalidate();
                }
                let bytes = self.screen.present_drawn(&f.bytes, f.w, f.h, f.drawn.take()).to_vec();
                (ToInterface::Frame { n, input: f.input, w: f.w, h: f.h, bytes }, Some(f))
            }
            Next::Msg(m) => (m, None),
            Next::Wait | Next::Closed => return None,
        };
        if let Err(e) = wire::encode(&m, out) {
            crate::log::line("attach", "warn", &format!("a message not sent: {e}"));
        }
        back
    }
}

/// The reader: the interface's messages until it goes. Input is the editor's while it's live; an ack is credit at
/// once (never waiting on a busy loop) and presented on the loop; a resize the next frame's size.
fn read_loop(sh: &Shared, mut s: UnixStream, mut dec: Decoder<ToHost>, tx: &Tx) {
    let (why, tell) = loop {
        match dec.message() {
            Ok(Some(m)) => {
                if let Some(end) = took(sh, m, tx) {
                    break end;
                }
            }
            Ok(None) => match dec.read_from(&mut s) {
                Ok(0) if dec.pending() > 0 => break ("its socket ended inside a message".into(), None),
                Ok(0) => break ("its socket closed".into(), None),
                Ok(_) => {}
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => break (format!("reading from it: {e}"), None),
            },
            Err(e) => break (format!("its bytes were wrong: {e}"), Some(Gone::Protocol { said: e.said })),
        }
    };
    lost(tx, sh.id, why, tell);
}

/// One message from the interface — Some: its end (why, and what it's told).
fn took(sh: &Shared, m: ToHost, tx: &Tx) -> Option<(String, Option<Gone>)> {
    let id = sh.id;
    let sent = match m {
        ToHost::Input { seq, seen, event } => match sh.live.load(Ordering::Relaxed) {
            true => tx.send(Event::Input { seen, seq, event: Box::new(event_of(event)) }),
            false => Ok(()),
        },
        ToHost::Resize { w, h } => {
            sh.size.store(pack(w, h), Ordering::Relaxed);
            tx.send(Event::Resize)
        }
        ToHost::Ack { frame } => {
            sh.mail.with(|s| s.ack(frame));
            tx.send(Event::Job(Box::new(move |ed: &mut Editor| presented(ed, id, frame))))
        }
        ToHost::Answer { id: ask, answer } => {
            tx.send(Event::Job(Box::new(move |ed: &mut Editor| answered(ed, id, ask, answer))))
        }
        ToHost::Zone { offset, .. } => {
            tx.send(Event::Job(Box::new(move |ed: &mut Editor| zone(ed, id, offset))))
        }
        ToHost::Bye => return Some(("it said goodbye".into(), None)),
        ToHost::Hello(_) => {
            let said = "a second Hello".to_string();
            return Some((said.clone(), Some(Gone::Protocol { said })));
        }
    };
    // (the editor is ending)
    sent.is_err().then(|| ("the host is ending".into(), None))
}

/// The wire's input as tarae's event.
pub fn event_of(i: wire::Input) -> Event {
    use crate::event::{Mouse, MouseKind as M};
    use crate::key::{Code as C, Key};
    use wire::{Code as WC, MouseKind as WM};
    match i {
        wire::Input::Key(k) => Event::Key(Key {
            code: match k.code {
                WC::Char(c) => C::Char(c),
                WC::Enter => C::Enter,
                WC::Esc => C::Esc,
                WC::Tab => C::Tab,
                WC::BackTab => C::BackTab,
                WC::Backspace => C::Backspace,
                WC::Delete => C::Delete,
                WC::Left => C::Left,
                WC::Right => C::Right,
                WC::Up => C::Up,
                WC::Down => C::Down,
                WC::Home => C::Home,
                WC::End => C::End,
                WC::PageUp => C::PageUp,
                WC::PageDown => C::PageDown,
                WC::F(n) => C::F(n),
            },
            ctrl: k.ctrl,
            alt: k.alt,
            shift: k.shift,
        }),
        wire::Input::Mouse(m) => Event::Mouse(Mouse {
            kind: match m.kind {
                WM::Down => M::Down,
                WM::Drag => M::Drag,
                WM::Up => M::Up,
                WM::ScrollUp => M::ScrollUp,
                WM::ScrollDown => M::ScrollDown,
                WM::ScrollLeft => M::ScrollLeft,
                WM::ScrollRight => M::ScrollRight,
                WM::ShiftScrollUp => M::ShiftScrollUp,
                WM::ShiftScrollDown => M::ShiftScrollDown,
                WM::Middle => M::Middle,
            },
            x: m.x,
            y: m.y,
            alt: m.alt,
            ctrl: m.ctrl,
        }),
        wire::Input::Hover { x, y } => Event::Hover(x, y),
        wire::Input::Paste(t) => Event::Paste(t),
        wire::Input::Focus(g) => Event::Focus(g),
    }
}

/// tarae's event as the wire's input (an attached client's: `client.rs`) — None: no input (a resize is its own
/// message, the rest are the editor's own).
pub fn input_of(ev: Event) -> Option<wire::Input> {
    use crate::event::MouseKind as M;
    use crate::key::Code as C;
    use wire::{Code as WC, MouseKind as WM};
    Some(match ev {
        Event::Key(k) => wire::Input::Key(wire::Key {
            code: match k.code {
                C::Char(c) => WC::Char(c),
                C::Enter => WC::Enter,
                C::Esc => WC::Esc,
                C::Tab => WC::Tab,
                C::BackTab => WC::BackTab,
                C::Backspace => WC::Backspace,
                C::Delete => WC::Delete,
                C::Left => WC::Left,
                C::Right => WC::Right,
                C::Up => WC::Up,
                C::Down => WC::Down,
                C::Home => WC::Home,
                C::End => WC::End,
                C::PageUp => WC::PageUp,
                C::PageDown => WC::PageDown,
                C::F(n) => WC::F(n),
            },
            ctrl: k.ctrl,
            alt: k.alt,
            shift: k.shift,
        }),
        Event::Mouse(m) => wire::Input::Mouse(wire::Mouse {
            kind: match m.kind {
                M::Down => WM::Down,
                M::Drag => WM::Drag,
                M::Up => WM::Up,
                M::ScrollUp => WM::ScrollUp,
                M::ScrollDown => WM::ScrollDown,
                M::ScrollLeft => WM::ScrollLeft,
                M::ScrollRight => WM::ScrollRight,
                M::ShiftScrollUp => WM::ShiftScrollUp,
                M::ShiftScrollDown => WM::ShiftScrollDown,
                M::Middle => WM::Middle,
            },
            x: m.x,
            y: m.y,
            alt: m.alt,
            ctrl: m.ctrl,
        }),
        Event::Hover(x, y) => wire::Input::Hover { x, y },
        Event::Paste(t) => wire::Input::Paste(t),
        Event::Focus(g) => wire::Input::Focus(g),
        _ => return None,
    })
}

/// Where a host's attach socket is, for the client: `pid`'s, else the tarae `tarae state` would pick
/// (`uisocket::find`) — its state socket's pid.
pub fn find(pid: Option<u32>) -> anyhow::Result<(u32, PathBuf)> {
    let pid = match pid {
        Some(p) => p,
        None => {
            let sock = crate::uisocket::find()?;
            let stem = sock.file_stem().and_then(|s| s.to_str()).and_then(|s| s.parse::<u32>().ok());
            stem.ok_or_else(|| anyhow::anyhow!("{}: not a tarae's socket", sock.display()))?
        }
    };
    let path = path_for(pid).ok_or_else(|| anyhow::anyhow!("no state folder"))?;
    Ok((pid, path))
}

/// Connects to the attach socket at `path` (a long path from its folder, as the state socket's).
pub fn connect(path: &Path) -> io::Result<UnixStream> {
    crate::uisocket::at(path, |p: &Path| UnixStream::connect(p))
}
