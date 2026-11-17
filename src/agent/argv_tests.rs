//! Landing 4's proof: every claude tarae starts gets the argv it got before Claude's flags moved into its wire.

use crate::agent::{CLAUDE, Start};
use crate::billing::Credential;
use crate::llm::LlmConfig;

/// A process's argv as one line (`llm.command`'s on `cfg`, `extra` after): each argument quoted, the system prompt
/// after its flag as `<system>`.
fn line(cfg: &LlmConfig, extra: &[String]) -> String {
    let extra: Vec<&str> = extra.iter().map(String::as_str).collect();
    let s = Start::on(&CLAUDE, crate::agent::Role::Draft, cfg);
    let dir = std::env::temp_dir();
    quoted(
        &crate::agent::claude::command(&s, &cfg.args, &extra, &dir, None, &Credential::Subscription).unwrap(),
    )
}

/// `line` for a start, as `agent::command` makes its process.
fn line_of(s: &Start) -> String {
    quoted(&crate::agent::command(s, &std::env::temp_dir(), None, Some(&Credential::Subscription)).unwrap())
}

fn quoted(cmd: &std::process::Command) -> String {
    let args: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
    let mut out = Vec::new();
    for (i, a) in args.iter().enumerate() {
        let system = i > 0 && matches!(args[i - 1].as_str(), "--append-system-prompt" | "--system-prompt");
        out.push(if system { "<system>".to_string() } else { format!("{a:?}") });
    }
    out.join(" ")
}

/// `llm.args` as given (the default), and one that asks for what each start leaves out or sets its own way.
fn bases() -> [(&'static str, LlmConfig); 2] {
    let mut busy = LlmConfig::default();
    busy.args.extend(
        [
            "--resume",
            "x",
            "--continue",
            "--session-id",
            "y",
            "--fork-session",
            "--settings",
            r#"{"model":"m"}"#,
            "--append-system-prompt",
            "mine",
            "--tools",
            "Read",
            "--max-turns",
            "99",
        ]
        .map(String::from),
    );
    (busy.model, busy.effort) = (Some("opus".into()), "high".into());
    busy.command = "true".into();
    [("default", LlmConfig { command: "true".into(), ..LlmConfig::default() }), ("busy", busy)]
}

/// Every start's argv, by its role and base.
fn argvs() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (name, base) in bases() {
        // The chat, as `chat::ensure_proc` starts it
        for (case, resume, fork, no_thought) in [
            ("new", None, false, false),
            ("resumed", Some("c-1"), false, false),
            ("fork", Some("c-1"), true, false),
            ("no thought", None, false, true),
        ] {
            for settings in [true, false] {
                let mut ed = crate::editor::Editor::new(crate::config::Config::default());
                ed.config.llm = LlmConfig { chat_settings: settings, ..base.clone() };
                let (cfg, extra) =
                    crate::chat::launched_as(&mut ed, resume, fork, no_thought).expect("launched");
                out.push((
                    format!("chat {case}{} · {name}", if settings { "" } else { " no settings" }),
                    line(&cfg, &extra),
                ));
            }
        }
        // `space i`'s
        out.push((format!("ask · {name}"), line_of(&crate::ask::agent_start(&CLAUDE, &base))));
        // The reviewer's, each role
        for role in
            [crate::reviewer::Role::One, crate::reviewer::Role::Angle(0), crate::reviewer::Role::Skeptic(0)]
        {
            out.push((
                format!("reviewer {role:?} · {name}"),
                line_of(&crate::reviewer::start(&CLAUDE, &base, role)),
            ));
        }
        // A commit draft's
        out.push((format!("draft · {name}"), line_of(&crate::llm::draft_start(&CLAUDE, &base))));
        // The web search's helper
        let web = crate::web::search::config(base.clone(), "");
        out.push((format!("web search · {name}"), line_of(&crate::web::search::helper(&CLAUDE, &web))));
    }
    // Auto cards' reviewer (its own flags, whatever `llm.args` says)
    let cards = LlmConfig {
        args: crate::autocards::args(),
        model: Some(crate::autocards::MODEL.into()),
        effort: String::new(),
        command: "true".into(),
        ..LlmConfig::default()
    };
    out.push(("card review".into(), line(&cards, &[])));
    out
}

/// Agents beside Claude, landing 4: Claude's flags moved into its wire, and every process tarae starts gets the argv
/// it got before — each role's (the chat's as it starts, the others' by `agent::command` on their own sites'
/// builders, so a site that named the wrong role fails it), on the default `llm.args` and on one asking for what each start leaves out or sets
/// itself (`argv_golden.txt`, written from main fe0d2a22's code with `TARAE_BLESS=1`; the chat's through
/// `ensure_proc` as it starts one). Its system prompt is `<system>`: the text is each role's, passed through.
#[test]
fn every_start_gets_the_argv_it_got() {
    let now: String = argvs().into_iter().map(|(case, argv)| format!("{case} => {argv}\n")).collect();
    let golden = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/agent/argv_golden.txt");
    if std::env::var_os("TARAE_BLESS").is_some() {
        std::fs::write(&golden, &now).unwrap();
    }
    let was = std::fs::read_to_string(&golden).unwrap();
    for (now, was) in now.lines().zip(was.lines()) {
        assert_eq!(now, was, "a start's argv changed");
    }
    assert_eq!(now.lines().count(), was.lines().count(), "the starts compared");
}
