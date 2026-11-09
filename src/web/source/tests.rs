use super::*;

fn wiki() -> Wiki {
    let t: toml::Table = toml::from_str(
        r#"v = [{ base = "https://corp.example/confluence", kind = "confluence-dc", token-env = "TARAE_TEST_PAT" }]"#,
    )
    .unwrap();
    crate::web::signed::wikis_of(&t["v"]).unwrap().remove(0)
}

fn api(url: &str) -> Option<String> {
    let w = wiki();
    asked(url, &[&w]).map(|a| a.map(|a| a.api).unwrap_or_else(|e| format!("error: {e}")))
}

/// The table reads, every row whole (one of html and list; short with its check).
#[test]
fn the_table_reads() {
    let t = parse(TABLE).unwrap();
    assert_eq!(kinds().collect::<Vec<_>>(), ["confluence-dc"]);
    assert_eq!(t[0].1.len(), 6);
    assert!(parse("[[k]]\npage = \"/a\"\napi = \"/b\"\ntitle = \"/t\"\n").is_err(), "no html nor list");
}

/// Each address form below the base maps to its API request below the base — a title's `+` and `%xx` read, then
/// encoded for the query; a blog post's date; a page id; a short link decoded; a search's words as CQL.
#[test]
fn each_address_form_maps_to_its_request() {
    let b = "https://corp.example/confluence";
    for (page, want) in [
        (
            "/display/OPS/Run+book%3A+prod",
            "/rest/api/content?spaceKey=OPS&title=Run%20book%3A%20prod&expand=body.view",
        ),
        (
            "/display/TST/2026/11/09/Release+notes",
            "/rest/api/content?spaceKey=TST&title=Release%20notes&type=blogpost&postingDay=2026-11-09&expand=body.view",
        ),
        ("/pages/viewpage.action?pageId=3965072", "/rest/api/content/3965072?expand=body.view"),
        ("/pages/viewpage.action?spaceKey=OPS&pageId=12", "/rest/api/content/12?expand=body.view"),
        ("/x/kIA8", "/rest/api/content/3965072?expand=body.view"),
        ("/x/BIAO", "/rest/api/content/950276?expand=body.view"),
        (
            "/dosearchsite.action?queryString=rollback+\"pg\"",
            "/rest/api/content/search?cql=siteSearch%20~%20%22rollback%20%5C%22pg%5C%22%22&expand=space&limit=10",
        ),
        (
            "/dosearchsite.action?cql=space%3DOPS",
            "/rest/api/content/search?cql=space%3DOPS&expand=space&limit=10",
        ),
    ] {
        assert_eq!(api(&format!("{b}{page}")), Some(format!("{b}{want}")), "{page}");
    }
    // (not a source request: fetched as a page)
    for plain in [
        "https://corp.example/confluence/",
        "https://corp.example/confluence/display/OPS",
        "https://corp.example/confluence/pages/viewpage.action",
        "https://corp.example/display/OPS/Runbook",
        "https://other.example/confluence/display/OPS/Runbook",
    ] {
        assert_eq!(api(plain), None, "{plain}");
    }
    assert!(api("https://corp.example/confluence/x/!!").is_some_and(|e| e.contains("couldn't be resolved")));
    // (a value bound for the API's path: one segment, never a way out of it)
    for stray in ["pageId=1%2F..%2F..%2Fadmin", "pageId=..", "pageId=a%5Cb"] {
        let got = api(&format!("{b}/pages/viewpage.action?{stray}"));
        assert!(
            got.as_deref().is_some_and(|e| e.starts_with("error:") && e.contains("not a page address")),
            "{stray}: {got:?}"
        );
    }
}

/// A short link's id is base64 of the page id's little-endian bytes — Atlassian's two examples.
#[test]
fn a_short_links_id_is_its_pages() {
    assert_eq!(tiny_id("kIA8"), Some(3965072));
    assert_eq!(tiny_id("BIAO"), Some(950276));
    assert_eq!(tiny_id("AQ"), Some(1));
    for no in ["", "AAAA", "!!", "AAAAAAAAAAAA"] {
        assert_eq!(tiny_id(no), None, "{no}");
    }
}

/// The answer's pointers take the page's HTML and title; a 404 or no result is "no such page" (or not one your
/// account can see); a short link whose answer names another is not resolved; a search reads as a list of pages at
/// the wiki.
#[test]
fn the_answer_gives_the_page_or_says_why_not() {
    let w = wiki();
    let one = |url: &str| asked(url, &[&w]).unwrap().unwrap();
    let display = one("https://corp.example/confluence/display/OPS/Runbook");
    let found =
        br#"{"results":[{"title":"Run <book>","body":{"view":{"value":"<p>Step one</p>"}}}],"size":1}"#;
    let html = page_of(&display, "u", 200, found).unwrap();
    assert!(html.contains("<title>Run &lt;book&gt;</title>") && html.contains("<p>Step one</p>"), "{html}");
    let none = "no such page at corp.example, or not one your account can see";
    assert_eq!(page_of(&display, "u", 200, br#"{"results":[],"size":0}"#), Err(none.into()));
    assert_eq!(page_of(&display, "u", 404, b"{}"), Err(none.into()));
    assert!(page_of(&display, "u", 200, b"<html>sign in</html>").unwrap_err().contains("isn't JSON"));
    assert!(page_of(&display, "u", 500, b"{}").unwrap_err().contains("answered 500"));
    assert_eq!(
        page_of(&display, "u", 406, b"").unwrap_err(),
        "the wiki refused the JSON request (406) for u"
    );
    let empty = br#"{"results":[{"title":"Blank","body":{"view":{"value":"  "}}}]}"#;
    let html = page_of(&display, "u", 200, empty).unwrap();
    assert!(html.contains("<p>(the wiki's API gave this page an empty body.view)</p>"), "{html}");
    let short = one("https://corp.example/confluence/x/kIA8");
    let page = |tiny: &str| {
        format!(r#"{{"title":"T","body":{{"view":{{"value":"<p>x</p>"}}}},"_links":{{"tinyui":"{tiny}"}}}}"#)
    };
    assert!(page_of(&short, "u", 200, page("/x/kIA8").as_bytes()).is_ok());
    assert!(
        page_of(&short, "u", 200, page("/x/BIAO").as_bytes()).unwrap_err().contains("couldn't be resolved")
    );
    let search = one("https://corp.example/confluence/dosearchsite.action?queryString=rollback");
    let list = br#"{"results":[{"title":"Rollback","space":{"name":"Ops"},"_links":{"webui":"/display/OPS/Rollback"}}]}"#;
    let html = page_of(&search, "u", 200, list).unwrap();
    assert!(
        html.contains(
            r#"<li><a href="https://corp.example/confluence/display/OPS/Rollback">Rollback</a> · Ops</li>"#
        ),
        "{html}"
    );
    assert!(html.contains("<title>rollback — corp.example/confluence's search</title>"), "{html}");
    assert!(page_of(&search, "u", 200, br#"{"results":[]}"#).unwrap().contains("Nothing found"));
}
