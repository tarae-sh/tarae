use super::*;

fn wikis(toml: &str) -> Result<Vec<Wiki>, String> {
    let t: toml::Table = toml::from_str(&format!("v = {toml}")).unwrap();
    wikis_of(&t["v"])
}

fn wiki() -> Wiki {
    wikis(r#"[{ base = "https://Corp.Example/confluence/", kind = "confluence-dc", token-env = "TARAE_TEST_PAT" }]"#)
        .unwrap()
        .remove(0)
}

/// A wiki is named by its base — https, a host by its name, a path —, a kind the sources know and a variable's
/// whole name; anything else is refused in words, never taken as something near it.
#[test]
fn a_wiki_is_a_base_a_kind_and_a_variable() {
    let w = wiki();
    assert_eq!(
        (w.base.as_str(), w.host.as_str(), w.port, w.prefix.as_str(), w.token_env.as_str()),
        ("https://corp.example/confluence", "corp.example", 443, "/confluence", "TARAE_TEST_PAT")
    );
    assert_eq!(w.shown(), "corp.example/confluence");
    let at_root = wikis(r#"[{ base = "https://wiki.test:8443", kind = "confluence-dc", token-env = "T" }]"#);
    assert_eq!(at_root.unwrap()[0].shown(), "wiki.test:8443");
    for (written, said) in [
        (r#"[{ base = "http://corp.example", kind = "confluence-dc", token-env = "T" }]"#, "https://"),
        (
            r#"[{ base = "https://localhost/w", kind = "confluence-dc", token-env = "T" }]"#,
            "beyond this machine",
        ),
        (r#"[{ base = "https://corp.example/?a", kind = "confluence-dc", token-env = "T" }]"#, "no query"),
        (
            r#"[{ base = "https://corp.example", kind = "notion", token-env = "T" }]"#,
            "a kind the sources know",
        ),
        (r#"[{ base = "https://corp.example", kind = "confluence-dc", token-env = "PATH" }]"#, "whole name"),
        (r#"[{ base = "https://corp.example", kind = "confluence-dc", token-env = "T_*" }]"#, "whole name"),
        (
            r#"[{ base = "https://corp.example", kind = "confluence-dc", token-env = "TERM" }]"#,
            "not one every run",
        ),
        (
            r#"[{ base = "https://corp.example", kind = "confluence-dc", token-env = "LANG" }]"#,
            "not one every run",
        ),
        (
            r#"[{ base = "https://corp.example", kind = "confluence-dc", token-env = "LC_ALL" }]"#,
            "not one every run",
        ),
        (
            r#"[{ base = "https://corp.example", kind = "confluence-dc", token-env = "HTTPS_PROXY" }]"#,
            "not one every run",
        ),
        (
            r#"[{ base = "https://corp.example", kind = "confluence-dc", token-env = "ANTHROPIC_API_KEY" }]"#,
            "Claude Code's own",
        ),
        (
            r#"[{ base = "https://corp.example", kind = "confluence-dc", token-env = "CLAUDE_CODE_OAUTH_TOKEN" }]"#,
            "Claude Code's own",
        ),
        (r#"[{ base = "https://corp.example", kind = "confluence-dc" }]"#, "token-env: missing"),
        (
            r#"[{ base = "https://corp.example", kind = "confluence-dc", token-env = "T", token = "x" }]"#,
            "token:",
        ),
        (r#""https://corp.example""#, "a list of tables"),
    ] {
        let e = wikis(written).unwrap_err();
        assert!(e.contains(said), "{written}: {e}");
    }
}

/// The token goes only with a request over https, beyond this machine, to the base's host and port, at or below its
/// path — never a sibling path sharing its letters, never one a dot segment takes out of it.
#[test]
fn the_token_goes_only_under_the_base() {
    let w = wiki();
    let under = |url: &str| w.under(&crate::web::http::target(url).unwrap());
    for yes in [
        "https://corp.example/confluence",
        "https://corp.example/confluence/display/OPS/Runbook",
        "https://corp.example/confluence?x=1",
        "http://corp.example/confluence/x", // (made https before it's sent: `http::target`)
    ] {
        assert!(under(yes), "{yes}");
    }
    for no in [
        "https://corp.example/",
        "https://corp.example/confluencex/a",
        "https://corp.example/other/confluence",
        "https://corp.example:8443/confluence/a",
        "https://other.example/confluence/a",
        "https://corp.example.evil.test/confluence/a",
        "https://corp.example/confluence/../admin",
        "https://corp.example/confluence/%2e%2e/admin",
        "https://corp.example/confluence/..;/admin",
        "https://corp.example/confluence/%2E.;x=1/admin",
        "https://corp.example/confluence/x%2f..%2f..%2fadmin",
        "https://corp.example/confluence/x%5C..%5Cadmin",
        "http://localhost/confluence",
    ] {
        assert!(!under(no), "{no}");
    }
}

/// The token is read from the environment given, one visible-ASCII word; unset or holding what can't go in a header
/// it's said in words (Claude's ending "ask the user") — and its value is in no `Debug`.
#[test]
fn the_token_comes_from_the_variable_and_is_never_printed() {
    let w = wiki();
    let env = |v: &str| HashMap::from([("TARAE_TEST_PAT".to_string(), OsString::from(v))]);
    let got = of(std::slice::from_ref(&w), &env(" tarae-fake-token\n"), false);
    assert_eq!(got[0].token.as_ref().map(Token::as_str), Ok("tarae-fake-token"));
    let printed = format!("{got:?}");
    assert!(!printed.contains("tarae-fake-token") && printed.contains("Token(…)"), "{printed}");
    let unset = of(std::slice::from_ref(&w), &HashMap::new(), false);
    assert_eq!(
        unset[0].token.as_ref().unwrap_err(),
        "TARAE_TEST_PAT isn't set in tarae's environment — export it and start tarae again"
    );
    let claudes = of(std::slice::from_ref(&w), &env(""), true);
    assert!(claudes[0].token.as_ref().unwrap_err().ends_with("start tarae again — ask the user"));
    for bad in ["tarae fake", "tarae-fake\r\nX-Other: 1", "tarae-фейк"] {
        let e = of(std::slice::from_ref(&w), &env(bad), false).remove(0).token.unwrap_err();
        assert!(e.contains("holds what a token can't") && !e.contains(bad), "{bad}: {e}");
    }
    assert_eq!(w.refused(401), "corp.example refused the token in TARAE_TEST_PAT — expired or revoked?");
    assert_eq!(
        w.refused(403),
        "corp.example refused the token in TARAE_TEST_PAT — your account isn't allowed this with it"
    );
}

/// Where the setting is applied (`with_env`) and where a fetch reads the token (`of`) judge a value alike: a wiki is
/// missing its token, and why, exactly when the fetch would say so in the same words.
#[test]
fn the_setting_and_the_fetch_judge_a_token_alike() {
    for value in
        [None, Some(""), Some(" \t\n"), Some(" tarae-fake-token\n"), Some("tarae fake"), Some("tarae-фейк")]
    {
        let env: HashMap<String, OsString> =
            value.map(|v| ("TARAE_TEST_PAT".to_string(), OsString::from(v))).into_iter().collect();
        let w = with_env(vec![wiki()], |n| env.get(n).cloned()).remove(0);
        let fetched = of(std::slice::from_ref(&w), &env, true).remove(0).token.map(|_| ());
        assert_eq!(
            w.token_missing.map(|m| m.words(&w.token_env, true)).map_or(Ok(()), Err),
            fetched,
            "{value:?}"
        );
    }
}

/// A page that echoes the token has every copy of it replaced before anyone reads it.
#[test]
fn an_echoed_token_is_scrubbed() {
    let env = HashMap::from([("TARAE_TEST_PAT".to_string(), OsString::from("tarae-fake-token"))]);
    let signed = of(&[wiki()], &env, false);
    let mut page = b"<p>Bearer tarae-fake-token</p><p>tarae-fake-tokentarae-fake-token</p>".to_vec();
    assert!(scrubbed(&mut page, &signed));
    assert_eq!(page, b"<p>Bearer [token]</p><p>[token][token]</p>");
    assert!(!scrubbed(&mut page, &signed), "nothing left");
}

/// The chat's one line (W4): none without a wiki — the prompt as it was, to the byte —; with one, where it is and
/// how its search reads, short (it's in every request of a chat started with it: counted).
#[test]
fn the_chats_line_names_the_wiki_and_its_search() {
    assert_eq!(prompt(&[]), "");
    let line = prompt(&[wiki()]);
    assert_eq!(
        line,
        " The user's wiki: https://corp.example/confluence (search: read \
         https://corp.example/confluence/dosearchsite.action?queryString=<words>) — read its pages by their addresses."
    );
    // (~4 characters a token: about 45 tokens for one wiki)
    assert!(line.len() < 200, "{} bytes", line.len());
    let two = prompt(&[wiki(), Wiki { base: "https://docs.corp.example".into(), ..wiki() }]);
    assert!(two.starts_with(" The user's wikis: https://corp.example/confluence (search: "), "{two}");
    assert!(
        two.contains(", https://docs.corp.example (search: read https://docs.corp.example/dosearchsite"),
        "{two}"
    );
}
