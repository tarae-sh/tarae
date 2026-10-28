//! `:web`'s keys on saved pages and fetched ones, and the reader's times (`cargo perf`) — the reader's own tests are
//! its crate's (`crates/tarae-web/src/tests.rs`), on the same saved pages (tests/web).

use super::{Prefer, read};
use crate::test_results::settle_until;
use std::path::{Path, PathBuf};

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/web")
}

fn bytes(name: &str) -> Vec<u8> {
    std::fs::read(dir().join(format!("pages/{name}.html"))).unwrap()
}

/// A paragraph of about 300 characters, three commas in each sentence, its word first (the reader's tests' own).
fn para(word: &str) -> String {
    format!("<p>{}</p>", format!("{word} said, in a few words, one, two, of the text ").repeat(6))
}

fn feed(ed: &mut crate::editor::Editor, keys: &str) {
    use crate::key::{Code, Key};
    let mut chars = keys.chars();
    while let Some(c) = chars.next() {
        let key = if c == '<' {
            let name: String = chars.by_ref().take_while(|&c| c != '>').collect();
            name.parse().unwrap()
        } else {
            Key::plain(Code::Char(c))
        };
        ed.handle_key(key);
    }
}

fn toasts(ed: &crate::editor::Editor) -> Vec<String> {
    ed.toasts.iter().map(|t| t.text.clone()).collect()
}

/// The front buffer is a page whose read has landed (or ended).
fn landed(ed: &crate::editor::Editor) -> bool {
    super::page::of(ed.doc()).is_some_and(|p| p.fetching.is_none())
}

/// No page's read is under way.
fn none_reading(ed: &crate::editor::Editor) -> bool {
    !ed.docs.iter().any(|d| super::page::of(d).is_some_and(|p| p.fetching.is_some()))
}

/// `:web <path>` from the project's folder, an ignored folder's as `:open` takes it: the page read on a worker into
/// a buffer of its own, named by its title, in markdown; how it read on the status line. A file that isn't a page
/// says what it is and opens nothing.
#[test]
fn web_reads_a_saved_page_into_a_buffer_named_by_its_title() {
    let root = crate::scratch::Scratch::new("web-saved");
    std::fs::create_dir_all(root.join("saved")).unwrap();
    std::fs::write(root.join(".gitignore"), "saved/\n").unwrap();
    std::fs::copy(dir().join("pages/issue.html"), root.join("saved/issue page.html")).unwrap();
    std::fs::write(root.join("saved/a.pdf"), b"%PDF-1.7\n").unwrap();
    let mut ed = crate::editor::Editor::new(crate::config::Config::default());
    ed.workspace.root = std::fs::canonicalize(root.path()).unwrap();
    let first = ed.doc().id;

    feed(&mut ed, ":web 'saved/issue page.html' https://github.com/ethnw/tarae/issues/12<ret>");
    // (its grammar loads on a worker after it)
    settle_until(&mut ed, |ed| ed.doc().id != first && ed.doc().lang_name().is_some());
    let doc = ed.doc();
    assert_eq!(
        doc.title.as_deref(),
        Some("web · github.com › web: read a saved page into a buffer · Issue #12 · ethnw/tarae · GitHub")
    );
    assert!(doc.text.to_string().contains("\n# web: read a saved page into a buffer #12\n"));
    assert_eq!(doc.lang_name(), Some("markdown"));
    assert!(
        toasts(&ed).iter().any(|t| t.starts_with("main · 3k chars · ") && t.ends_with(" · UTF-8")),
        "{:?}",
        toasts(&ed)
    );
    // Its links went to the address given
    assert!(doc.text.to_string().contains("[web](https://github.com/ethnw/tarae/issues?q="));

    // Again: the one buffer, read anew
    let docs = ed.docs.len();
    feed(&mut ed, ":web 'saved/issue page.html' https://github.com/ethnw/tarae/issues/12 page<ret>");
    settle_until(&mut ed, |ed| ed.toasts.iter().any(|t| t.text.starts_with("page · ")));
    assert_eq!(ed.docs.len(), docs);

    // What isn't a page: its buffer says so, faint, on its first row — and its line
    feed(&mut ed, ":web saved/a.pdf<ret>");
    settle_until(&mut ed, |ed| toasts(ed).iter().any(|t| t.starts_with("web: a PDF")));
    assert_eq!(ed.docs.len(), docs + 1);
    let p = super::page::of(ed.doc()).unwrap();
    assert!(p.read.is_none() && p.failed.as_deref().is_some_and(|f| f.starts_with("a PDF")));
    assert!(ed.doc().text.to_string().starts_with("a PDF"), "{}", ed.doc().text);
    assert_eq!(ed.doc().paint.len(), 1, "faint");
}

/// `:web https://…` with the session's copy of that page (fetched within `KEEP`, by the address asked or the one it
/// came from) opens it with no network, said with its age; an address the fetch refuses is said at once, nothing
/// fetched, no buffer made.
#[test]
fn web_fetches_a_page_or_takes_the_session_s_copy() {
    let mut ed = crate::editor::Editor::new(crate::config::Config::default());
    let f = super::fetch::Fetched {
        asked: "http://kept.test/a".into(),
        url: "https://kept.test/a".into(),
        status: 200,
        content_type: Some("text/html".into()),
        bytes: b"<title>Kept</title><main><h1>A kept page</h1><p>Its text.</p></main>".to_vec(),
        from: Vec::new(),
        hops: 0,
        upgraded: true,
        said: None,
        cut: false,
        took: std::time::Duration::from_millis(40),
        elsewhere: None,
    };
    let gone = ed.workspace.pages.put(std::sync::Arc::new(f), crate::clock::now());
    assert!(gone.is_empty());
    feed(&mut ed, ":web https://kept.test/a<ret>");
    assert_eq!(ed.doc().title.as_deref(), Some("web · kept.test"), "made at once");
    settle_until(&mut ed, landed);
    assert_eq!(ed.doc().title.as_deref(), Some("web · kept.test › Kept"));
    assert!(ed.doc().text.to_string().contains("# A kept page"));
    assert_eq!(ed.doc().selection().primary().head, 0, "a first read at its top");
    let said = |t: &String| t.ends_with(" · UTF-8 · read just now · over https");
    assert!(toasts(&ed).iter().any(said), "{:?}", toasts(&ed));

    for (typed, said) in [
        (":web https://user:pw@kept.test/<ret>", "an address with a user or password in it isn't fetched"),
        (":web https://10.0.0.1/<ret>", "a bare address isn't fetched"),
    ] {
        let (before, docs) = (toasts(&ed).len(), ed.docs.len());
        feed(&mut ed, typed);
        assert!(toasts(&ed)[before..].iter().any(|t| t.starts_with(said)), "{typed}: {:?}", toasts(&ed));
        assert!(none_reading(&ed) && ed.docs.len() == docs, "{typed}: nothing under way, no buffer");
    }
}

/// The session keeps only a whole 2xx page that isn't this machine's: the same address again (its buffer closed) opens
/// the copy, but an error's page, one maybe cut short and a `localhost` one (a dev server changes under you) are
/// fetched each time; with its buffer open it's that buffer, nothing fetched; `:web!` fetches again into it whatever
/// was kept, the new copy kept in its place.
#[test]
fn web_keeps_only_whole_pages_and_web_bang_fetches_again() {
    use super::fetch::tests::{Reply, ok, path, reply, server, way, yours_rules};
    let (at, heads) = server(true, |head| match path(head) {
        "/missing" => reply(
            "HTTP/1.1 404 Not Found\r\nContent-Type: text/html\r\nContent-Length: 11\r\n\r\n<p>gone</p>",
        ),
        "/cut" => Reply {
            close_notify: false,
            ..reply("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n<p>to the close</p>")
        },
        _ => ok("<title>A</title><main><h1>A page</h1><p>Its text.</p></main>"),
    });
    let mut ed = crate::editor::Editor::new(crate::config::Config::default());
    ed.workspace.web_way =
        Some(std::sync::Arc::new(move |stop| super::fetch::Way { stop, ..way(at, yours_rules()) }));
    // (how many requests the server had for what `typed` opened, its buffer closed after)
    let opened = |ed: &mut crate::editor::Editor, typed: &str| {
        feed(ed, &format!("{typed}<ret>"));
        settle_until(ed, |ed| landed(ed) && none_reading(ed));
        heads.try_iter().count()
    };
    let asked = |ed: &mut crate::editor::Editor, typed: &str| {
        let n = opened(ed, typed);
        feed(ed, ":buffer-close<ret>");
        n
    };
    let kept =
        |ed: &crate::editor::Editor, url: &str| ed.workspace.pages.get(url, crate::clock::now()).is_some();
    let local = format!("https://localhost:{}/a", at.port());
    for (url, keeps) in [
        ("https://page.test/a", true),
        ("https://page.test/missing", false),
        ("https://page.test/cut", false),
        (local.as_str(), false),
    ] {
        assert_eq!(asked(&mut ed, &format!(":web {url}")), 1, "{url}: fetched");
        assert_eq!(kept(&ed, url), keeps, "{url}");
        assert_eq!(asked(&mut ed, &format!(":web {url}")), usize::from(!keeps), "{url}: again");
    }
    let missing = opened(&mut ed, ":web https://page.test/missing");
    let (page, docs) = (ed.doc().id, ed.docs.len());
    assert_eq!((missing, opened(&mut ed, ":web https://page.test/missing")), (1, 0), "its buffer: no fetch");
    assert_eq!((ed.doc().id, ed.docs.len()), (page, docs));

    let before = ed.workspace.pages.get("https://page.test/a", crate::clock::now()).map(|(at, _)| at);
    assert_eq!(opened(&mut ed, ":web https://page.test/a"), 0, "the copy");
    let (page, docs) = (ed.doc().id, ed.docs.len());
    assert_eq!(opened(&mut ed, ":web! https://page.test/a"), 1, ":web! fetches again");
    assert_eq!((ed.doc().id, ed.docs.len()), (page, docs), "into its buffer");
    let after = ed.workspace.pages.get("https://page.test/a", crate::clock::now()).map(|(at, _)| at);
    assert!(after > before, "the new copy kept in its place");
}

/// `:web <path> <address>` into the buffer a fetch of that address made: the file is its source from then on —
/// `:reload` and `:web-reader` read the file again, nothing fetched, the network's copy gone with its text; `:web!`
/// fetches it again, and the fetch is its source.
#[test]
fn a_saved_page_read_into_a_fetched_pages_buffer_is_its_source_from_then_on() {
    use super::fetch::tests::{ok, server, way, yours_rules};
    let (at, heads) =
        server(true, |_| ok("<title>Net</title><main><h1>From the network</h1><p>t.</p></main>"));
    let root = crate::scratch::Scratch::new("web-saved-into");
    std::fs::copy(dir().join("pages/issue.html"), root.join("issue.html")).unwrap();
    let mut ed = crate::editor::Editor::new(crate::config::Config::default());
    ed.workspace.root = std::fs::canonicalize(root.path()).unwrap();
    ed.workspace.web_way =
        Some(std::sync::Arc::new(move |stop| super::fetch::Way { stop, ..way(at, yours_rules()) }));
    let opened = |ed: &mut crate::editor::Editor, typed: &str| {
        feed(ed, &format!("{typed}<ret>"));
        settle_until(ed, |ed| landed(ed) && none_reading(ed));
        heads.try_iter().count()
    };
    let source = |ed: &crate::editor::Editor| {
        let p = super::page::of(ed.doc()).unwrap();
        (
            p.saved.as_deref().and_then(|s| s.file_name()).map(|n| n.to_string_lossy().into_owned()),
            p.copy.is_some(),
        )
    };
    assert_eq!(opened(&mut ed, ":web https://page.test/a"), 1);
    let (page, docs) = (ed.doc().id, ed.docs.len());
    assert_eq!(source(&ed), (None, true));

    assert_eq!(opened(&mut ed, ":web issue.html https://page.test/a"), 0);
    assert_eq!((ed.doc().id, ed.docs.len()), (page, docs), "into its buffer");
    assert!(ed.doc().text.to_string().contains("# web: read a saved page into a buffer #12"));
    assert_eq!(source(&ed), (Some("issue.html".into()), false), "the file its source, the copy gone");
    assert_eq!(opened(&mut ed, ":reload"), 0, "the file read again");
    assert_eq!(opened(&mut ed, ":web-reader page"), 0);
    assert!(ed.doc().text.to_string().contains("# web: read a saved page into a buffer #12"));
    assert_eq!(source(&ed), (Some("issue.html".into()), false));

    assert_eq!(opened(&mut ed, ":web! https://page.test/a"), 1, ":web! fetches");
    assert!(ed.doc().text.to_string().contains("# From the network"));
    assert_eq!(source(&ed), (None, true), "the fetch its source");
}

/// A read whose redirect lands on an address another buffer has: that one is shown, and where to go goes along. A
/// buffer that never landed goes; one read before (`:reload`) keeps what it showed, said — never closed under you.
#[test]
fn a_redirect_to_an_open_page_shows_it_and_closes_only_a_buffer_that_never_landed() {
    use super::fetch::tests::{ok, path, reply, server, way, yours_rules};
    use std::sync::atomic::{AtomicBool, Ordering};
    let moved = std::sync::Arc::new(AtomicBool::new(false));
    let moves = moved.clone();
    let (at, _) = server(true, move |head| {
        let to_b = || reply("HTTP/1.1 302 Found\r\nLocation: /b\r\nContent-Length: 0\r\n\r\n");
        match path(head) {
            "/a" if moves.load(Ordering::SeqCst) => to_b(),
            "/a" => ok("<title>A</title><main><h1>A page</h1><p>Its text.</p></main>"),
            "/c" => to_b(),
            _ => ok(&format!("<title>B</title><main><h1>B</h1>{}<h2 id=x>X</h2><p>u.</p></main>", para("b"))),
        }
    });
    let mut ed = crate::editor::Editor::new(crate::config::Config::default());
    ed.workspace.web_way =
        Some(std::sync::Arc::new(move |stop| super::fetch::Way { stop, ..way(at, yours_rules()) }));
    let opened = |ed: &mut crate::editor::Editor, typed: &str| {
        feed(ed, &format!("{typed}<ret>"));
        settle_until(ed, |ed| landed(ed) && none_reading(ed));
    };
    opened(&mut ed, ":web https://page.test/b");
    let b = ed.doc().id;
    opened(&mut ed, ":web https://page.test/a");
    let (a, docs, text) = (ed.doc().id, ed.docs.len(), ed.doc().text.to_string());

    moved.store(true, Ordering::SeqCst);
    opened(&mut ed, ":reload");
    assert_eq!(ed.doc().id, b, "the page it redirects to shown");
    let kept = ed.docs.iter().find(|d| d.id == a).expect("the reloaded buffer kept");
    assert_eq!((kept.text.to_string(), ed.docs.len()), (text, docs), "as it was");
    let said = "web: it redirects to https://page.test/b, open already — this one kept as it was";
    assert!(toasts(&ed).iter().any(|t| t == said), "{:?}", toasts(&ed));

    // A new one that lands there: gone, its fragment taken to the open one
    opened(&mut ed, ":web https://page.test/c#x");
    assert_eq!((ed.doc().id, ed.docs.len()), (b, docs), "its buffer gone");
    let line = ed.doc().text.byte_to_line(ed.doc().selection().primary().head);
    assert_eq!(ed.doc().text.line(line).to_string(), "## X\n", "at #x");
}

/// A read that fails says why once in each place: its line the short why (its host is on the line), its first row the
/// whole of it — the address, the why, the OS's words after ` · `, no parentheses inside the why's.
#[test]
fn a_failed_read_says_its_why_short_on_its_line_and_whole_on_its_first_row() {
    use super::fetch::tests::{way, yours_rules};
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let mut ed = crate::editor::Editor::new(crate::config::Config::default());
    let at = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    ed.workspace.web_way =
        Some(std::sync::Arc::new(move |stop| super::fetch::Way { stop, ..way(at, yours_rules()) }));
    feed(&mut ed, &format!(":web http://localhost:{port}/<ret>"));
    settle_until(&mut ed, none_reading);
    let refused = std::io::Error::from_raw_os_error(libc::ECONNREFUSED).to_string();
    let os = refused.split(" (os error").next().unwrap().to_lowercase();
    let whole =
        format!("http://localhost:{port}/: nothing answers there · {os} · os error {}", libc::ECONNREFUSED);
    assert_eq!(ed.doc().text.to_string(), whole, "row 1: the whole why");
    assert_eq!(ed.doc().selection().primary().head, 0, "at its start");
    let p = super::page::of(ed.doc()).unwrap();
    assert_eq!(super::page::brief(p.failed.as_deref().unwrap()), "nothing answers there", "its line");
    // (what isn't the fetch's form is said as it is)
    assert_eq!(super::page::brief("stopped"), "stopped");
    assert_eq!(super::page::brief("a PDF, not a page tarae reads"), "a PDF, not a page tarae reads");
}

/// A page being read is in sight from the first frame: its empty buffer never shows the start screen in its place.
#[test]
fn a_page_being_read_is_no_start_screen() {
    let mut ed = crate::editor::Editor::new(crate::config::Config::default());
    assert!(ed.welcome());
    super::page::stand_in(&mut ed, "https://page.test/a", "");
    assert_eq!(ed.docs.len(), 1, "in the empty scratch's place");
    assert!(!ed.welcome());
}

/// The page's commands: `:web-reader` reads the page in sight again as another part from its copy (nothing fetched),
/// says the part it reads with no word and when it's that part already; `:reload` fetches it anew into its buffer;
/// `:web-open` hands its own address to the system's opener (none started in tests) — never a saved page's, nor on
/// code. A page that needs JavaScript says `:web-open` on its line.
#[test]
fn a_pages_commands_read_it_again_reload_it_and_hand_it_to_the_browser() {
    use super::fetch::tests::{ok, server, way, yours_rules};
    let html = format!(
        "<title>A</title><p>Before its main.</p><main><h1>A page</h1>{}{}</main>",
        para("one"),
        para("two")
    );
    let (at, heads) = server(true, move |_| ok(&html));
    let root = crate::scratch::Scratch::new("web-commands");
    std::fs::copy(dir().join("pages/spa.html"), root.join("spa.html")).unwrap();
    let mut ed = crate::editor::Editor::new(crate::config::Config::default());
    ed.workspace.root = std::fs::canonicalize(root.path()).unwrap();
    ed.workspace.web_way =
        Some(std::sync::Arc::new(move |stop| super::fetch::Way { stop, ..way(at, yours_rules()) }));
    let said = |ed: &mut crate::editor::Editor, typed: &str, words: &str| {
        let before = toasts(ed).len();
        feed(ed, &format!("{typed}<ret>"));
        let end = std::time::Instant::now() + crate::test_results::PATIENCE;
        while !(landed(ed) && toasts(ed)[before..].iter().any(|t| t.starts_with(words))) {
            let left = end.saturating_duration_since(std::time::Instant::now());
            let ev = ed.events.recv_timeout(left);
            let ev = ev.unwrap_or_else(|| panic!("{typed}: {words:?} in {:?}", &toasts(ed)[before..]));
            ed.handle_event(ev);
        }
        heads.try_iter().count()
    };
    assert_eq!(said(&mut ed, ":web https://page.test/a", "main · "), 1);
    let (page, docs, main) = (ed.doc().id, ed.docs.len(), ed.doc().text.to_string());
    assert_eq!(said(&mut ed, ":web-reader", "usage: :web-reader main|article|page — it reads main now"), 0);
    assert_eq!(said(&mut ed, ":web-reader page", "page · "), 0, "from its copy");
    assert_ne!(ed.doc().text.to_string(), main, "the whole page");
    assert_eq!(said(&mut ed, ":web-reader page", "web: already read as page"), 0);
    assert_eq!(said(&mut ed, ":reload", "page · "), 1, "fetched anew");
    assert_eq!((ed.doc().id, ed.docs.len()), (page, docs), "into its buffer");
    said(&mut ed, ":web-open", "web: https://page.test/a handed to your browser");
    assert_eq!(ed.workspace.web_opened, ["https://page.test/a"]);

    said(
        &mut ed,
        ":web spa.html",
        "web: needs JavaScript: tarae doesn't run a page's scripts — :web-open opens",
    );
    said(&mut ed, ":web-open", "a saved page has no web address");
    ed.new_scratch();
    feed(&mut ed, ":web-open<ret>");
    assert!(toasts(&ed).iter().any(|t| t == "not a web page — :web <url> reads one"), "{:?}", toasts(&ed));
    assert_eq!(ed.workspace.web_opened.len(), 1, "nothing else handed over");
}

/// `:web https://…` through its way (servers and a proxy of the test's own standing in for yours): its buffer made at
/// once, reading, the page in it once it lands; `C-c` on a page being read stops it — its late result dropped, the
/// buffer saying `stopped`, no error — and with nothing under way is the code's (said); two pages read at once, each
/// into its own buffer; a buffer closed mid-read stops its read, its result dropped; `:web` of a page that never
/// landed reads it again.
#[test]
fn web_reads_into_its_buffer_and_c_c_or_closing_it_stops_the_read() {
    use super::fetch::tests::{Reply, ok, path, reply, server, way, yours_rules};
    use std::sync::{Arc, Mutex, mpsc};
    let (release, gate) = mpsc::channel::<()>();
    let gate = Arc::new(Mutex::new(gate));
    let (at, heads) = server(true, move |head| match path(head) {
        "/a" => ok("<title>A</title><main><h1>A page</h1><p>Its text.</p></main>"),
        "/gated" => {
            Reply { gate: Some(gate.clone()), ..ok("<title>Gated</title><main><h1>G</h1><p>t.</p></main>") }
        }
        _ => Reply { hold: true, ..reply("") },
    });
    let mut ed = crate::editor::Editor::new(crate::config::Config::default());
    ed.workspace.web_way = Some(Arc::new(move |stop| super::fetch::Way { stop, ..way(at, yours_rules()) }));
    let dropped = |n: usize| move |ed: &crate::editor::Editor| ed.workspace.web_dropped == n;
    let wait = |what: &str| heads.recv_timeout(crate::test_results::PATIENCE).expect(what);
    let errors = |ed: &crate::editor::Editor| toasts(ed).iter().filter(|t| t.starts_with("web: ")).count();

    feed(&mut ed, ":web https://page.test/a<ret>");
    let a = ed.doc().id;
    let reading =
        |ed: &crate::editor::Editor| super::page::of(ed.doc()).and_then(|p| p.fetching.as_ref()).is_some();
    assert!(reading(&ed) && ed.doc().title.as_deref() == Some("web · page.test"), "made at once");
    let page_layer = super::page::takes;
    assert!(page_layer(&ed), "C-c is the page's");
    settle_until(&mut ed, landed);
    assert_eq!(ed.doc().title.as_deref(), Some("web · page.test › A"));
    assert!(ed.doc().text.to_string().contains("# A page"));
    assert!(!page_layer(&ed), "landed: C-c is the code's");
    assert_eq!(
        ed.workspace.pages.get("https://page.test/a", crate::clock::now()).map(|(_, f)| f.status),
        Some(200)
    );
    wait("a's request");

    // `C-c` mid-read: stopped, said once, its result dropped; the buffer stays, saying so
    feed(&mut ed, ":web https://page.test/held<ret>");
    let held = ed.doc().id;
    wait("the held one came");
    let said = errors(&ed);
    feed(&mut ed, "<C-c>");
    assert!(toasts(&ed).iter().any(|t| t == "web: stopped reading page.test"), "{:?}", toasts(&ed));
    settle_until(&mut ed, dropped(1));
    assert_eq!((ed.doc().id, ed.doc().text.to_string().as_str()), (held, "stopped"));
    assert_eq!(errors(&ed), said + 1, "no error past the stop's word: {:?}", toasts(&ed));
    // (nothing under way: the code's `C-c` — refused on a read-only page, said)
    let before = toasts(&ed).len();
    feed(&mut ed, "<C-c>");
    assert!(toasts(&ed).len() > before && ed.doc().text == "stopped", "{:?}", toasts(&ed));

    // Two at once, each into its own buffer; `:web` of the stopped one reads it again
    feed(&mut ed, ":web https://page.test/held<ret>");
    assert_eq!(ed.doc().id, held, "its buffer");
    wait("held again");
    feed(&mut ed, ":web https://page.test/gated<ret>");
    let gated = ed.doc().id;
    wait("gated came");
    release.send(()).unwrap();
    settle_until(&mut ed, landed);
    assert_eq!((ed.doc().id, ed.doc().title.as_deref()), (gated, Some("web · page.test › Gated")));
    let doc =
        |id| ed.docs.iter().find(|d| d.id == id).and_then(super::page::of).map(|p| p.fetching.is_some());
    assert_eq!(doc(held), Some(true), "held still reading");

    // Its buffer closed: its read stopped, its result dropped
    ed.close_doc(held);
    settle_until(&mut ed, dropped(2));
    assert!(none_reading(&ed));

    // A page open: `:web` of it shows it, nothing fetched
    feed(&mut ed, ":web https://page.test/a<ret>");
    assert_eq!(ed.doc().id, a);
    assert!(landed(&ed) && heads.try_recv().is_err(), "its buffer: nothing fetched");
}

/// A FIFO with nothing writing to it: refused at once and said — never a worker held inside open(2).
#[test]
fn a_fifo_is_refused_without_waiting_for_a_writer() {
    use std::os::unix::ffi::OsStrExt;
    let root = crate::scratch::Scratch::new("web-fifo");
    let fifo = root.join("page.html");
    let c = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
    // SAFETY: a NUL-terminated path in the test's own folder
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    let want = format!("{}: not a file", fifo.display());
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(super::open::bytes_of(&fifo));
    });
    let got = rx.recv_timeout(crate::test_results::PATIENCE).expect("refused without waiting for a writer");
    assert_eq!(got.unwrap_err(), want);
}

/// A saved page's own address keeps its name whole — `#`, `?`, `%` and spaces encoded — so its relative links
/// resolve against its folder and its fragments stay on it.
#[test]
fn a_saved_pages_address_keeps_its_name_and_its_links_their_folder() {
    let url = super::open::file_url(Path::new("/saved/a b?/notes#1.html"));
    assert_eq!(url, "file:///saved/a%20b%3F/notes%231.html");
    assert_eq!(super::open::file_url(Path::new("/s/100%é.html")), "file:///s/100%25%C3%A9.html");
    let html = r##"<p><a href="other.html">other</a> <a href="#x">here</a></p>"##;
    let p = read(html.as_bytes(), &url, Some("text/html"), Prefer::Page);
    let urls: Vec<&str> = p.links.iter().map(|l| l.url.as_str()).collect();
    assert_eq!(urls, ["file:///saved/a%20b%3F/other.html", "file:///saved/a%20b%3F/notes%231.html#x"]);
}

/// A page's buffer is read-only — an edit put back and said, insert mode left — so reading it again replaces it
/// with nothing of yours in it.
#[test]
fn a_pages_buffer_is_read_only_and_read_again_in_place() {
    use crate::key::{Code, Key};
    let root = crate::scratch::Scratch::new("web-read-only");
    std::fs::copy(dir().join("pages/issue.html"), root.join("issue.html")).unwrap();
    let mut ed = crate::editor::Editor::new(crate::config::Config::default());
    ed.workspace.root = std::fs::canonicalize(root.path()).unwrap();
    let first = ed.doc().id;
    feed(&mut ed, ":web issue.html<ret>");
    settle_until(&mut ed, |ed| ed.doc().id != first && landed(ed));
    let text = ed.doc().text.to_string();
    let typed = |ed: &mut crate::editor::Editor, keys: &str| {
        for c in keys.chars() {
            ed.handle_event(crate::event::Event::Key(Key::plain(Code::Char(c))));
        }
    };
    typed(&mut ed, "jdd");
    assert_eq!(ed.doc().text.to_string(), text, "put back");
    assert!(
        toasts(&ed).iter().any(|t| t == "a web page is read-only — :w <path> saves a copy"),
        "{:?}",
        toasts(&ed)
    );
    typed(&mut ed, "ix");
    assert_eq!(ed.mode, crate::editor::Mode::Normal);
    assert_eq!(ed.doc().text.to_string(), text, "nothing typed");

    let (docs, before) = (ed.docs.len(), toasts(&ed).len());
    feed(&mut ed, ":web issue.html page<ret>");
    settle_until(&mut ed, |ed| toasts(ed)[before..].iter().any(|t| t.starts_with("page · ")));
    assert_eq!(ed.docs.len(), docs, "the one buffer, read anew");
    assert!(ed.doc().read_only.is_some() && ed.doc().text != text.as_str());
}

/// `:w` on a page says how to save a copy; `:w <path>` writes its text as shown after a first line saying where it came
/// from and when (an HTML comment: the markdown reads the same) — one there refused but with `:w!`.
#[test]
fn w_saves_a_copy_of_a_page_saying_where_it_came_from() {
    let root = crate::scratch::Scratch::new("web-w");
    std::fs::copy(dir().join("pages/issue.html"), root.join("issue.html")).unwrap();
    let mut ed = crate::editor::Editor::new(crate::config::Config::default());
    ed.workspace.root = std::fs::canonicalize(root.path()).unwrap();
    feed(&mut ed, ":web issue.html https://github.com/ethnw/tarae/issues/12<ret>");
    settle_until(&mut ed, landed);
    let said = |ed: &crate::editor::Editor, start: &str| toasts(ed).iter().any(|t| t.starts_with(start));

    feed(&mut ed, ":w<ret>");
    assert!(said(&ed, "a web page is read-only — :w <path> saves a copy"), "{:?}", toasts(&ed));
    feed(&mut ed, ":w copy.md<ret>");
    settle_until(&mut ed, |ed| said(ed, "saved a copy of the page to copy.md — :open copy.md"));
    let written = std::fs::read_to_string(root.join("copy.md")).unwrap();
    let (head, rest) = written.split_once('\n').unwrap();
    assert!(head.starts_with("<!-- https://github.com/ethnw/tarae/issues/12 · read 20"), "{head}");
    assert!(head.ends_with(" -->"), "{head}");
    assert_eq!(rest, ed.doc().text.to_string());

    std::fs::write(root.join("copy.md"), "mine").unwrap();
    feed(&mut ed, ":w copy.md<ret>");
    settle_until(&mut ed, |ed| toasts(ed).iter().any(|t| t.contains("copy.md is there — :w! ")));
    assert_eq!(std::fs::read_to_string(root.join("copy.md")).unwrap(), "mine", "refused");
    feed(&mut ed, ":w! copy.md<ret>");
    settle_until(&mut ed, |_| std::fs::read_to_string(root.join("copy.md")).unwrap() == written);
}

// ── Times ──

/// The reader in-process, the best of 3 a page (web-plan P0 measured it in the plugin's runtime): the five pages,
/// and a 4 MB one — ISO 639's tables over and over, as P0's 4.1 MB Wikipedia comparison page is. Numbers for
/// `cargo perf`.
#[test]
fn perf_web_read() {
    if crate::test_results::unoptimized("web::tests::perf_web_read") {
        return;
    }
    let table = String::from_utf8(bytes("table")).unwrap();
    let (start, end) = (table.find("<main").unwrap(), table.rfind("</main>").unwrap());
    let inner = &table[start..end];
    let big = format!("<html><body>{}</main></body></html>", inner.repeat((4 << 20) / inner.len() + 1));
    assert!(big.len() > 4 << 20);
    let mut pages: Vec<(&str, Vec<u8>, f64)> =
        [("blog", 15.0), ("docsrs", 25.0), ("issue", 5.0), ("mdn", 25.0), ("spa", 5.0)]
            .into_iter()
            .map(|(n, budget)| (n, bytes(n), budget))
            .collect();
    pages.push(("4mb", big.into_bytes(), 400.0));
    // ECMA-262 (7.6 MB, no `<main>`: the pick scores all of it) isn't a fixture, for its size: by hand, its saved
    // copy named by `TARAE_WEB_ECMA` (.sb/web/more/big.html, from https://tc39.es/ecma262/)
    if let Some(at) = std::env::var_os("TARAE_WEB_ECMA") {
        pages.push(("ecma262", std::fs::read(at).unwrap(), 600.0));
    }
    for (name, b, budget) in pages {
        let mut best = f64::MAX;
        for _ in 0..3 {
            let t = std::time::Instant::now();
            let p = read(&b, "https://example.com/", None, Prefer::Guess);
            best = best.min(t.elapsed().as_secs_f64() * 1000.0);
            assert!(!p.text.is_empty());
        }
        let line = format!("PERF web_read_{name}_ms {best:.2} budget={budget} noise=1\n");
        let _ = std::io::Write::write_all(&mut std::io::stderr(), line.as_bytes());
        assert!(best < budget, "{name}: {best:.1} ms (budget {budget})");
    }
}
