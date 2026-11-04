use std::path::PathBuf;

use super::*;

fn url(s: &str) -> Url {
    parse(s).unwrap_or_else(|e| panic!("{s}: {e}"))
}

/// The address's parts: user, host, port, the path as the relay takes it (the home's by `/~/`, %-decoded), the pid.
#[test]
fn an_ssh_address_is_read_into_its_parts() {
    let u = url("ssh://w%40q@mac:2222/~/my%20dir?pid=41");
    assert_eq!(u.user.as_deref(), Some("w@q"));
    assert_eq!(
        (u.host.as_str(), u.port, u.path.as_deref(), u.pid),
        ("mac", Some(2222), Some("~/my dir"), Some(41))
    );
    let u = url("ssh://mac");
    assert_eq!((u.user, u.port, u.path, u.pid), (None, None, None, None));
    assert_eq!(url("ssh://mac/").path, None, "/ alone: the relay's pick");
    assert_eq!(url("ssh://mac/~").path.as_deref(), Some("~"));
    assert_eq!(url("ssh://mac/~/").path.as_deref(), Some("~"));
    assert_eq!(url("ssh://mac/srv/app").path.as_deref(), Some("/srv/app"));
    assert_eq!(url("ssh://mac/~x").path.as_deref(), Some("/~x"), "another user's ~ isn't the home's");
    let v6 = url("ssh://u@[fe80::1]:22/~/x");
    assert_eq!((v6.host.as_str(), v6.port), ("fe80::1", Some(22)));
    assert_eq!(url("ssh://[::1]").host, "::1");
    assert_eq!(url("ssh://mac?pid=7").path, None);
}

/// Each rule of the address broken once, refused saying what.
#[test]
fn each_rule_of_the_address_refuses_what_breaks_it() {
    for (s, says) in [
        ("mac", "not an ssh:// address"),
        ("http://mac", "not an ssh:// address"),
        ("ssh://mac#x", "a # part"),
        ("ssh://u:pw@mac", "a password in the address"),
        ("ssh://@mac", "no user"),
        ("ssh://-oProxyCommand=x@mac", "starts with -"),
        ("ssh://-oProxyCommand=x", "starts with -"),
        ("ssh://", "no host"),
        ("ssh:///~/x", "no host"),
        ("ssh://m%20ac", "holds a blank"),
        ("ssh://u%0A@mac", "control character"),
        ("ssh://mac:0", "the port \"0\""),
        ("ssh://mac:65536", "the port \"65536\""),
        ("ssh://mac:+22", "the port \"+22\""),
        ("ssh://mac:", "the port \"\""),
        ("ssh://[::1", "an unclosed ["),
        ("ssh://[::1]x", "only :PORT"),
        ("ssh://mac/a%2", "a % not followed by two hex digits"),
        ("ssh://mac/a%zz", "a % not followed by two hex digits"),
        ("ssh://mac/a%ff", "isn't UTF-8"),
        ("ssh://mac/a%0Ab", "the path holds a control character"),
        ("ssh://mac?user=x", "?user=x isn't read"),
        ("ssh://mac?pid=0", "pid=0 isn't a process number"),
        ("ssh://mac?pid=07", "pid=07 isn't a process number"),
        ("ssh://mac?pid=x", "pid=x isn't a process number"),
        ("ssh://mac?pid=1&pid=2", "pid given twice"),
    ] {
        let e = parse(s).expect_err(s);
        assert!(e.contains(says), "{s}: {e:?} doesn't say {says:?}");
    }
}

/// ssh's line: no pty, no escape character, the link watched, `BatchMode` only when asked; the port and user as
/// flags, the host alone, the remote command one word after `--`.
#[test]
fn the_ssh_line_is_the_plans() {
    let u = url("ssh://w@mac:2222/~/x");
    let line = args(&u, "REMOTE", false).join(" ");
    assert_eq!(
        line,
        "-T -e none -o ServerAliveInterval=15 -o ServerAliveCountMax=3 -p 2222 -l w mac -- REMOTE"
    );
    assert!(args(&u, "REMOTE", true).join(" ").contains(" -o BatchMode=yes "));
    assert_eq!(relay_args(&u, None, None), ["~/x"]);
    assert_eq!(relay_args(&u, Some(9), Some(Duration::from_secs(60))), ["--wait", "60", "--pid", "9"]);
    assert_eq!(relay_args(&url("ssh://mac/~/x?pid=4"), None, None), ["--pid", "4"]);
    assert!(relay_args(&url("ssh://mac"), None, None).is_empty());
}

/// Words no shell may read as written: blanks, quotes, `$`, backticks, globs, `;`, a backslash (inside fish's single
/// quotes `\\` and `\'` are escapes, inside sh's nothing is), a newline.
const HARD: &[&str] = &[
    "plain",
    "/a dir/with  two",
    "it's",
    "''",
    "$HOME $(echo no) `echo no` ${x}",
    "*.rs ? [a] ~ ; & | > <",
    "back\\slash",
    "\\'",
    "\\\\",
    "end\\",
    "\"dq\" \\\"",
    "line\nbreak",
    "",
    "é — 한글",
];

/// The remote line read by each login shell there is — sh always, fish (the Mac's), bash and zsh where installed — as
/// both of ssh's shells read it (the login shell the line, `$SHELL -lc` its argument): the stand-in tarae gets every
/// word as written.
#[test]
fn the_remote_line_reads_the_same_in_sh_and_fish() {
    let dir = crate::scratch::Scratch::new("ssh-quote");
    let home = dir.join("home");
    std::fs::create_dir_all(&home).unwrap();
    // (the stand-in tarae: each argument and a NUL)
    let tarae = dir.join("my tarae's");
    crate::scratch::write_script(&tarae, "#!/bin/sh\nfor a; do printf '%s\\0' \"$a\"; done\n");
    let words: Vec<String> = HARD.iter().map(|s| s.to_string()).collect();
    let line = remote(&tarae.to_string_lossy(), &words);
    let mut want = b"attach\0--stdio\0".to_vec();
    for w in &words {
        want.extend_from_slice(w.as_bytes());
        want.push(0);
    }
    let mut read_by = Vec::new();
    for shell in ["sh", "fish", "bash", "zsh"] {
        let Some(path) = installed(shell) else {
            assert_ne!(shell, "sh", "no sh");
            eprintln!(
                "the_remote_line_reads_the_same_in_sh_and_fish: {shell} isn't installed — not read by it"
            );
            continue;
        };
        let out = std::process::Command::new(&path)
            .args(["-c", &line])
            .env_clear()
            .env("SHELL", &path)
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("PATH", "/usr/bin:/bin")
            .output()
            .unwrap();
        assert!(out.status.success(), "{shell}: {}\n{line}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&want),
            "{shell} read the line otherwise:\n{line}"
        );
        read_by.push(shell);
    }
    assert!(read_by.contains(&"sh"), "{read_by:?}");
}

/// `name` on the PATH or in Homebrew's folders.
fn installed(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .chain(["/opt/homebrew/bin", "/usr/local/bin", "/bin", "/usr/bin"].map(PathBuf::from))
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

/// A quote is single quotes with `'` and `\` alone outside them — nothing else is ever outside.
#[test]
fn a_quote_leaves_only_quotes_and_backslashes_outside() {
    assert_eq!(quote("a b"), "'a b'");
    assert_eq!(quote("it's"), "'it'\\''s'");
    assert_eq!(quote("a\\b"), "'a'\\\\'b'");
    assert_eq!(quote(""), "''");
    assert_eq!(
        remote("tarae", &["~/x".into()]),
        "exec \"$SHELL\" -lc ''\\''tarae'\\'' attach --stdio '\\''~/x'\\'''"
    );
}

/// The preamble found past noise of any bytes and length up to `NOISE_MAX`, its `ok` read and nothing after it taken
/// (the protocol's first bytes stay); `no` with its hosts.
#[test]
fn the_preamble_is_found_past_any_noise() {
    let mut rng = crate::mutate::Rng(0x005e_ed55);
    for round in 0..200 {
        let len = match round {
            0 => 0,
            1 => NOISE_MAX,
            _ => rng.below(if round % 10 == 0 { NOISE_MAX } else { 300 }),
        };
        let mut noise: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
        // (a near miss in it too: the magic's start, cut)
        if round % 3 == 0 && len > MAGIC.len() {
            let at = rng.below(len - MAGIC.len());
            noise[at..at + MAGIC.len() - 1].copy_from_slice(&MAGIC[..MAGIC.len() - 1]);
        }
        let mut s = noise.clone();
        s.extend_from_slice(MAGIC);
        s.extend_from_slice(b"ok 4242 /Users/u/a b\n\x00\x01protocol");
        let mut r = io::Cursor::new(s);
        let said = preamble(&mut r).unwrap_or_else(|e| panic!("round {round}, {len} bytes of noise: {e:?}"));
        assert_eq!(said, Said::Ok { pid: 4242, project: "/Users/u/a b".into() }, "round {round}");
        let mut rest = Vec::new();
        r.read_to_end(&mut rest).unwrap();
        assert_eq!(rest, b"\x00\x01protocol", "round {round}: the protocol's bytes stay");
    }
    let mut no = MAGIC.to_vec();
    no.extend_from_slice(b"no no tarae host runs on mac\n   12  /a\n\n   13  /b, /c\n");
    assert_eq!(
        preamble(&mut &no[..]).unwrap(),
        Said::No {
            why: "no tarae host runs on mac".into(),
            hosts: vec!["   12  /a".into(), "   13  /b, /c".into()]
        }
    );
}

/// Each way the preamble fails, told apart: the stream ended before it (what came kept), past `NOISE_MAX` of noise,
/// a word after it that isn't the relay's.
#[test]
fn each_way_the_preamble_fails_is_its_own() {
    match preamble(&mut &b"Last login: today\n"[..]) {
        Err(Before::Ended(n)) => assert_eq!(n, b"Last login: today\n"),
        other => panic!("{other:?}"),
    }
    let mut long = vec![b'x'; NOISE_MAX + MAGIC.len()];
    long.extend_from_slice(MAGIC);
    match preamble(&mut &long[..]) {
        Err(Before::TooMuch(n)) => assert_eq!(n.len(), NOISE_MAX),
        other => panic!("{other:?}"),
    }
    for word in ["maybe 12 /a", "ok twelve /a", ""] {
        let mut s = MAGIC.to_vec();
        s.extend_from_slice(word.as_bytes());
        s.push(b'\n');
        match preamble(&mut &s[..]) {
            Err(Before::Garbled(l)) => assert_eq!(l, word),
            other => panic!("{word}: {other:?}"),
        }
    }
}

/// Why ssh ended before the relay spoke, in its own words each: ssh's (255), no tarae (127 — the PATH's or the one
/// named), an older tarae (`unknown: --stdio`), anything else with its code and what it printed.
#[test]
fn each_end_before_the_relay_is_said_in_its_own_words() {
    let u = url("ssh://mac/~/x");
    let said = |tarae, code, last: &[&str], noise: &[u8]| {
        let last: Vec<String> = last.iter().map(|s| s.to_string()).collect();
        before_said(&u, tarae, code, &last, noise)
    };
    assert_eq!(
        said("tarae", Some(255), &["warning: x", "w@mac: Permission denied (publickey)."], b""),
        "ssh mac: w@mac: Permission denied (publickey)."
    );
    let none = said("tarae", Some(127), &["fish: Unknown command: tarae"], b"");
    assert!(none.starts_with("no tarae on mac") && none.contains("--remote-tarae PATH"), "{none}");
    let named = said("/opt/t", Some(127), &["sh: /opt/t: not found"], b"");
    assert_eq!(named, "no tarae at /opt/t on mac (sh: /opt/t: not found)");
    let older = said("tarae", Some(1), &["Error: unknown: --stdio", "usage: tarae attach [--pid PID]"], b"");
    assert!(older.starts_with("the tarae on mac is older than this one"), "{older}");
    let other = said("tarae", Some(3), &[], b"motd");
    assert_eq!(
        other,
        "ssh mac ended (exit 3) before the relay spoke: no word on its stderr; it printed:\nmotd"
    );
    assert!(said("tarae", None, &["x"], b"").contains("(a signal)"));
}

/// ssh's stderr: its lines kept (the last `STDERR_KEPT`, blank ones and a CR's end left out), a line cut short kept
/// as its stream ends.
#[test]
fn sshs_stderr_keeps_its_last_lines() {
    let mut e = Stderr::default();
    for i in 0..30 {
        e.heard(format!("line {i}\r\n\n").as_bytes());
    }
    e.heard(b"cut");
    e.line();
    assert_eq!(e.last.len(), STDERR_KEPT);
    assert_eq!(e.last.front().map(String::as_str), Some("line 11"));
    assert_eq!(e.last.back().map(String::as_str), Some("cut"));
}
