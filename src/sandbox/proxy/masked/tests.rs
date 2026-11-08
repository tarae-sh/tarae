//! The registry and a tunnel's head edits: sentinels as U1 makes them, kept and forgotten with the session; what
//! can be masked; two names of one value (l); a secret's host's head edited only as named, the target's value
//! percent-encoded (d), `Accept-Encoding: identity` (U2), `TE` dropped (n); another secret's sentinel refused in a
//! head; an answer's head scrubbed but its framing; no value in a `Debug`. Bodies: `stage`'s tests and `swap`'s.

use super::*;

/// Stand-in values — no provider's form (`concat!` in parts).
const REAL_A: &str = concat!("tarae-fake-", "token-0123456789abcdef");
const REAL_B: &str = concat!("tarae-fake-", "key+/=0123456789");

/// The session numbers these tests use, apart from every other test's (the registry is the process's).
const SESSION: u64 = 0x3a5c_0000;

fn env(vars: &[(&str, &str)]) -> HashMap<String, OsString> {
    vars.iter().map(|(k, v)| (k.to_string(), OsString::from(v))).collect()
}

fn table(names: &[&str]) -> BTreeMap<String, Vec<String>> {
    names.iter().map(|n| (n.to_string(), vec!["api.example.test".to_string()])).collect()
}

/// U1: a value of 20 bytes or more gets a sentinel exactly as long — `fake_value_` then srt's alphabet; a shorter one
/// srt's 47-byte `fake_value_<uuid4>`. Kept across the session's runs, another session's its own, a new one once the
/// value's length calls for it, gone with the session. A name tarae's environment lacks is said, never minted.
#[test]
fn sentinels_are_as_long_as_their_values_and_kept_with_the_session() {
    let short = "short-v8";
    let names: Vec<String> = ["A", "S", "T"].map(String::from).to_vec();
    let vars = env(&[("A", REAL_A), ("S", short), ("T", &"t".repeat(20))]);
    let (m, said) = of_config(SESSION + 1, &table(&["A", "S", "T", "GONE"]), &names, &vars);
    let m = m.unwrap();
    assert_eq!(
        said,
        ["GONE (llm.sandbox-secrets) isn't in tarae's environment — your commands don't get it"]
    );
    let of = |m: &Masked, n: &str| m.sentinels().find(|(k, _)| *k == n).unwrap().1.to_string();
    let (a, s, t) = (of(&m, "A"), of(&m, "S"), of(&m, "T"));
    assert_eq!((a.len(), s.len(), t.len()), (REAL_A.len(), 47, 20));
    for x in [&a, &t] {
        assert!(x.starts_with("fake_value_") && x[11..].bytes().all(|c| ALPHABET.contains(&c)), "{x}");
    }
    let uuid: Vec<usize> = s[11..].split('-').map(str::len).collect();
    assert_eq!(uuid, [8, 4, 4, 4, 12], "{s}");
    assert_eq!(&s[25..26], "4", "a version 4 UUID: {s}");
    // The next run of the session: the same; another session: its own
    let again = of_config(SESSION + 1, &table(&["A", "S"]), &names, &vars).0.unwrap();
    assert_eq!((of(&again, "A"), of(&again, "S")), (a.clone(), s.clone()));
    let other = of_config(SESSION + 2, &table(&["A"]), &names, &vars).0.unwrap();
    assert_ne!(of(&other, "A"), a);
    // A value whose length changed: a sentinel as long as it
    let longer = format!("{REAL_A}xyz");
    let changed = of_config(SESSION + 1, &table(&["A"]), &names, &env(&[("A", &longer)])).0.unwrap();
    assert_eq!(of(&changed, "A").len(), longer.len());
    // Not maskable (`names` left it out, said where the environment was made): nothing
    assert!(of_config(SESSION + 1, &table(&["S"]), &[], &vars).0.is_none());
    // The session ended: new ones
    forget(SESSION + 1);
    let after = of_config(SESSION + 1, &table(&["S"]), &names, &vars).0.unwrap();
    assert_ne!(of(&after, "S"), s);
    forget(SESSION + 1);
    forget(SESSION + 2);
}

/// No sentinel of a session inside another, nor inside a real value: a thousand minted, all apart.
#[test]
fn sentinels_are_unique_and_none_inside_another() {
    let names: Vec<String> = (0..40).map(|k| format!("N{k}")).collect();
    let vars: HashMap<String, OsString> =
        names.iter().enumerate().map(|(k, n)| (n.clone(), OsString::from("v".repeat(20 + k % 3)))).collect();
    let t: BTreeMap<String, Vec<String>> = names.iter().map(|n| (n.clone(), vec!["a.test".into()])).collect();
    let m = of_config(SESSION + 3, &t, &names, &vars).0.unwrap();
    let all: Vec<&str> = m.sentinels().map(|(_, s)| s).collect();
    for (i, a) in all.iter().enumerate() {
        for (j, b) in all.iter().enumerate() {
            assert!(i == j || !a.contains(b), "{a} holds {b}");
        }
    }
    forget(SESSION + 3);
}

/// e and m: visible ASCII only, 8 bytes at least — each refusal in a report's words.
#[test]
fn a_value_masked_is_visible_ascii_of_8_bytes_or_more() {
    assert_eq!(maskable(b"12345678"), Ok(()));
    assert_eq!(maskable(REAL_B.as_bytes()), Ok(()));
    assert_eq!(maskable(b"1234567"), Err("too short to mask safely — its card can pass the real value"));
    for bad in
        [&b"has a space"[..], b"line\r\nInjected: 1", b"tab\there12", b"caf\xc3\xa9-value-1", b"nul\0value12"]
    {
        assert!(maskable(bad).unwrap_err().starts_with("a byte that isn't visible ASCII"), "{bad:?}");
    }
}

/// l: two names of one value (GH_TOKEN and GITHUB_TOKEN often are) — the value is scrubbed once, to the sentinel of the
/// secret whose hosts hold the tunnel's host, else the first by name; the longest value first.
#[test]
fn two_names_of_one_value_scrub_to_the_hosts_own() {
    let m = Masked::of(&[
        ("B_TOKEN", "fake_value_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", REAL_A, &["api.b.test"]),
        ("A_TOKEN", "fake_value_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", REAL_A, &["api.a.test"]),
        (
            "LONGER",
            "fake_value_lllllllllllllllllllllllllllllllllll",
            concat!("tarae-fake-", "token-0123456789abcdefXYZ"),
            &["x.test"],
        ),
    ]);
    let scrubbed = |host: &str| -> Vec<(String, String)> {
        let v = m.toward(host, 443);
        v.reals
            .iter()
            .map(|(r, s, _)| (String::from_utf8_lossy(r).into(), String::from_utf8_lossy(s).into()))
            .collect()
    };
    let b = scrubbed("api.b.test");
    assert_eq!(b.len(), 2, "one value once");
    assert!(b[0].0.ends_with("XYZ"), "the longest first");
    assert_eq!(b[1], (REAL_A.into(), "fake_value_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into()), "the host's own");
    assert_eq!(scrubbed("api.a.test")[1].1, "fake_value_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    assert_eq!(
        scrubbed("elsewhere.test")[1].1,
        "fake_value_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "else the first by name"
    );
}

/// The registry for the head tests: A toward `api.a.test` (as long as its value), B toward `api.b.test`.
fn two() -> Masked {
    Masked::of(&[
        ("A_TOKEN", "fake_value_0000000000000000000000", REAL_A, &["api.a.test"]),
        ("B_KEY", "fake_value_11111111111111111111111111111111111", REAL_B, &["api.b.test"]),
    ])
}

/// Toward a secret's host its sentinel is swapped in every field's value (a `Basic` credential decoded, swapped,
/// encoded again), in the path and the query (percent-encoded but what's unreserved — B's `+ / =` too, so a path's
/// segment stays one), every `Accept-Encoding` one `identity` in the first one's place, `TE` dropped — nothing else
/// changes, not a space nor a field's order.
#[test]
fn a_secrets_host_gets_its_head_edited_as_named_and_nothing_else() {
    let m =
        Masked::of(&[("B_KEY", "fake_value_11111111111111111111111111111111111", REAL_B, &["api.b.test"])]);
    let v = m.toward("api.b.test", 443);
    let s = "fake_value_11111111111111111111111111111111111";
    let basic = parse::base64(format!("x-access-token:{s}").as_bytes());
    let head = format!(
        "GET /repos/{s}/x?token={s}&a=1 HTTP/1.1\r\nHost: api.b.test\r\nAuthorization:   Basic {basic}\r\n\
         Accept-Encoding: gzip, br\r\nX-Key: k={s};\r\nTE: gzip\r\naccept-encoding: deflate\r\nX-Other:  as is \r\n\r\n"
    );
    let got = String::from_utf8(v.request_head(head.as_bytes()).unwrap().bytes).unwrap();
    let enc = "tarae-fake-key%2B%2F%3D0123456789";
    let basic_real = parse::base64(format!("x-access-token:{REAL_B}").as_bytes());
    assert_eq!(
        got,
        format!(
            "GET /repos/{enc}/x?token={enc}&a=1 HTTP/1.1\r\nHost: api.b.test\r\nAuthorization:   Basic {basic_real}\r\n\
             Accept-Encoding: identity\r\nX-Key: k={REAL_B};\r\nX-Other:  as is \r\n\r\n"
        )
    );
    // None sent: one added, last
    let plain = "GET / HTTP/1.1\r\nHost: api.b.test\r\n\r\n";
    let got = String::from_utf8(v.request_head(plain.as_bytes()).unwrap().bytes).unwrap();
    assert_eq!(got, "GET / HTTP/1.1\r\nHost: api.b.test\r\nAccept-Encoding: identity\r\n\r\n");
    // A host no secret goes to: as it came
    let other = m.toward("elsewhere.test", 443);
    assert_eq!(other.request_head(plain.as_bytes()).unwrap().bytes, plain.as_bytes());
    // Its body's coding said: not searched
    let gz = "POST / HTTP/1.1\r\nHost: api.b.test\r\nContent-Encoding: gzip\r\nContent-Length: 3\r\n\r\n";
    assert_eq!(v.request_head(gz.as_bytes()).unwrap().encoded.as_deref(), Some("gzip"));
}

/// D5: another secret's sentinel toward a host it doesn't go to — in a field, the target, a `Basic` credential — is
/// refused, said with its name; toward its own host it's swapped.
#[test]
fn another_secrets_sentinel_is_refused_in_a_head() {
    let m = two();
    let v = m.toward("api.b.test", 443);
    let a = "fake_value_0000000000000000000000";
    let basic = parse::base64(format!("u:{a}").as_bytes());
    for head in [
        format!("GET / HTTP/1.1\r\nHost: api.b.test\r\nAuthorization: Bearer {a}\r\n\r\n"),
        format!("GET /x?k={a} HTTP/1.1\r\nHost: api.b.test\r\n\r\n"),
        format!("GET / HTTP/1.1\r\nHost: api.b.test\r\nauthorization: basic {basic}\r\n\r\n"),
    ] {
        assert_eq!(
            v.request_head(head.as_bytes()).unwrap_err(),
            "a run sent A_TOKEN's masked value toward api.b.test:443 — it goes only to A_TOKEN's own hosts",
            "{head}"
        );
    }
    let own = m.toward("api.a.test", 443);
    let head = format!("GET / HTTP/1.1\r\nHost: api.a.test\r\nAuthorization: Bearer {a}\r\n\r\n");
    let got = String::from_utf8(own.request_head(head.as_bytes()).unwrap().bytes).unwrap();
    assert!(got.contains(&format!("Bearer {REAL_A}\r\n")), "{got}");
}

/// P2b: a card's mask goes only in the sign-in header — toward its host its sentinel is swapped in `Authorization`
/// (Bearer, and a `Basic` credential decoded), never in the target, another field or the body: left as its fake
/// there, each said. Your own `llm.sandbox-secrets` entry beside it still goes everywhere (P2a).
#[test]
fn a_cards_mask_is_swapped_only_in_the_sign_in_header() {
    // (each as long as its value: a body's swap keeps its length)
    let (a, b) = ("fake_value_0000000000000000000000", "fake_value_1111111111111111");
    let m = Masked::of(&[
        ("A_TOKEN", a, REAL_A, &["api.a.test"]),
        // (a config entry toward the same host: P2a's every-field swap)
        ("B_KEY", b, REAL_B, &["api.a.test"]),
    ])
    .card_marked(&["A_TOKEN"]);
    let v = m.toward("api.a.test", 443);
    // Authorization: swapped, Bearer and Basic
    let bearer = format!("GET / HTTP/1.1\r\nHost: api.a.test\r\nAuthorization: Bearer {a}\r\n\r\n");
    let h = v.request_head(bearer.as_bytes()).unwrap();
    let got = String::from_utf8(h.bytes).unwrap();
    assert!(got.contains(&format!("Bearer {REAL_A}\r\n")) && h.left.is_empty(), "{got}");
    let basic = parse::base64(format!("x-access-token:{a}").as_bytes());
    let head = format!("GET / HTTP/1.1\r\nHost: api.a.test\r\nauthorization: Basic {basic}\r\n\r\n");
    let got = String::from_utf8(v.request_head(head.as_bytes()).unwrap().bytes).unwrap();
    let real = parse::base64(format!("x-access-token:{REAL_A}").as_bytes());
    assert!(got.contains(&format!("Basic {real}\r\n")), "{got}");
    // The target and another field: left as its fake, said — the config entry's swapped there all the same
    let head =
        format!("GET /gists?t={a}&k={b} HTTP/1.1\r\nHost: api.a.test\r\nX-Token: {a}\r\nX-Key: {b}\r\n\r\n");
    let h = v.request_head(head.as_bytes()).unwrap();
    let got = String::from_utf8(h.bytes).unwrap();
    assert!(got.contains(&format!("t={a}&")) && got.contains(&format!("X-Token: {a}\r\n")), "{got}");
    assert!(!got.contains(REAL_A), "{got}");
    assert!(
        got.contains("k=tarae-fake-key%2B%2F%3D0123456789 ") && got.contains(&format!("X-Key: {REAL_B}\r\n"))
    );
    assert_eq!(h.left.len(), 2, "{:?}", h.left);
    assert!(
        h.left[0].starts_with("A_TOKEN's masked value in the request's target went as its fake"),
        "{:?}",
        h.left
    );
    assert!(h.left[1].contains("goes only in the Authorization header"), "{:?}", h.left);
    // The body: left as its fake, said; the config entry's swapped
    let mut scan = v.request_scan().expect("a scan");
    let mut out = Vec::new();
    scan.data(format!("{{\"content\":\"{a}\",\"key\":\"{b}\"}}").as_bytes(), &mut out).unwrap();
    scan.end(&mut out).unwrap();
    let body = String::from_utf8(out).unwrap();
    assert_eq!(body, format!("{{\"content\":\"{a}\",\"key\":\"{REAL_B}\"}}"));
    let left = scan.left();
    assert_eq!(left.len(), 1, "{left:?}");
    let said = v.left_said(true, left[0].0, &left[0].1);
    assert!(said.starts_with("A_TOKEN's masked value in a request body went as its fake"), "{said}");
    // The same name as your own entry: everywhere
    let mine = Masked::of(&[("A_TOKEN", a, REAL_A, &["api.a.test"])]);
    let h = mine.toward("api.a.test", 443).request_head(head.as_bytes()).unwrap();
    assert!(
        String::from_utf8(h.bytes).unwrap().contains(&format!("X-Token: {REAL_A}\r\n")) && h.left.is_empty()
    );
}

/// D3: an answer's head scrubbed of every real value — its status line, `Location`, any field — but its framing's fields;
/// its coding said.
#[test]
fn an_answers_head_is_scrubbed_but_its_framing() {
    let m = two();
    let v = m.toward("api.a.test", 443);
    let head = format!(
        "HTTP/1.1 302 {REAL_A}\r\nLocation: https://x.test/?t={REAL_A}\r\nX-Echo: {REAL_B} and {REAL_A}\r\n\
         Content-Encoding: br\r\nContent-Length: 0\r\n\r\n"
    );
    let h = v.answer_head(head.as_bytes());
    let (a, b) = ("fake_value_0000000000000000000000", "fake_value_11111111111111111111111111111111111");
    assert_eq!(
        String::from_utf8(h.bytes).unwrap(),
        format!(
            "HTTP/1.1 302 {a}\r\nLocation: https://x.test/?t={a}\r\nX-Echo: {b} and {a}\r\n\
             Content-Encoding: br\r\nContent-Length: 0\r\n\r\n"
        )
    );
    assert_eq!(h.encoded.as_deref(), Some("br"));
}

/// No value in a `Debug` — the registry's, a tunnel's view, a run's rules carrying it: names and counts only.
#[test]
fn no_value_in_a_debug() {
    let m = Arc::new(two());
    let rules = super::super::egress::Egress { masked: Some(m.clone()), ..Default::default() };
    let shown = format!("{m:?} {:?} {rules:?} {:?}", m.toward("api.a.test", 443), m.secrets());
    for value in [REAL_A, REAL_B, "fake_value_0000", "fake_value_1111"] {
        assert!(!shown.contains(value), "{value} in {shown}");
    }
    assert!(shown.contains("A_TOKEN") && shown.contains("B_KEY"), "{shown}");
}
