use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use super::*;

/// A kubeconfig named as the real one, in a stand-in home (no cluster, no credential: an exec plugin's name only)
fn config(context: &str) -> String {
    format!(
        "apiVersion: v1\nkind: Config\nclusters:\n- name: c\n  cluster: {{server: \"https://eks.invalid\"}}\n\
         contexts:\n- name: {context}\n  context: {{cluster: c, user: u}}\ncurrent-context: {context}\n\
         users:\n- name: u\n  user:\n    exec: {{command: aws, args: [eks, get-token]}}\n"
    )
}

/// A stand-in home `h` (`~/.kube/config` sets `prod-eks`, `~/none.yaml` none) and project `p` (`dev.yaml` sets
/// `dev`), in the test's own folder.
fn places(name: &str) -> (crate::scratch::Scratch, PathBuf, PathBuf) {
    let s = crate::scratch::Scratch::new(name);
    let (home, cwd) = (s.join("h"), s.join("p"));
    std::fs::create_dir_all(home.join(".kube")).unwrap();
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::write(home.join(".kube/config"), config("prod-eks")).unwrap();
    std::fs::write(cwd.join("dev.yaml"), config("\"dev\"")).unwrap();
    std::fs::write(home.join("none.yaml"), "apiVersion: v1\ncurrent-context: \"\"\n").unwrap();
    (s, home, cwd)
}

/// `said` where no run writes.
fn said_in(cmd: &str, cwd: &Path, env: Option<&str>, home: &Path) -> Option<Said> {
    let place = Place { cwd, env: env.map(OsStr::new), home: Some(home), writes: Some(&[]) };
    said(cmd, &place)
}

/// A path as the card shows it.
fn at(p: &Path) -> String {
    crate::allow::tilde(p)
}

#[test]
fn a_kubectl_runs_card_names_the_context_kubectl_would_take() {
    let (_s, home, cwd) = places("kubeconfig");
    let said = |cmd: &str, env: Option<&str>| said_in(cmd, &cwd, env, &home).map(|s| s.line);
    let at = |p: &str| at(p.as_ref());
    // ~/.kube/config by default
    let default =
        format!("kubectl's context: prod-eks ({})", at(&home.join(".kube/config").to_string_lossy()));
    assert_eq!(said("kubectl get pods", None).as_deref(), Some(default.as_str()));
    assert_eq!(said("kubectl get pods -A", None).as_deref(), Some(default.as_str()));
    // --context first, either spelling; --kubeconfig over KUBECONFIG, relative to where it runs
    for cmd in ["kubectl --context staging get pods", "kubectl get pods --context=staging"] {
        assert_eq!(said(cmd, None).as_deref(), Some("kubectl's context: staging (its --context)"), "{cmd}");
    }
    let dev = format!("kubectl's context: dev ({})", at(&cwd.join("dev.yaml").to_string_lossy()));
    assert_eq!(
        said("kubectl --kubeconfig dev.yaml get pods", Some("~/none.yaml")).as_deref(),
        Some(dev.as_str())
    );
    // KUBECONFIG's list: the first that sets one (the line's own first); one that sets none said so
    let env_list = "~/none.yaml:dev.yaml";
    assert_eq!(said("kubectl get pods", Some(env_list)).as_deref(), Some(dev.as_str()));
    assert_eq!(
        said("KUBECONFIG=~/.kube/config kubectl get pods", Some(env_list)).as_deref(),
        Some(default.as_str())
    );
    assert_eq!(said("KUBECONFIG=dev.yaml kubectl get ns", None).as_deref(), Some(dev.as_str()));
    let none = said("kubectl get pods", Some("~/none.yaml")).unwrap();
    assert_eq!(
        none,
        format!(
            "kubectl's context: can't be told — {}: none set",
            at(&home.join("none.yaml").to_string_lossy())
        )
    );
    // Unreadable: said with the OS's words, never guessed
    let gone = said("kubectl --kubeconfig gone.yaml get pods", None).unwrap();
    assert!(
        gone.starts_with("kubectl's context: can't be told — ")
            && gone.ends_with("gone.yaml: No such file or directory"),
        "{gone}"
    );
    // No kubectl in the line: nothing
    assert_eq!(said("echo kubectl", None), None);
    assert_eq!(said("helm list", None), None);
}

/// Told only for one kubectl running one of its own verbs but `config` (reviewer round 4 #1) — wrappers, its own
/// `KUBECONFIG=` and the options before its verb aside; any other variable set for it or `env` option (round 5 #2)
/// can't be told. Any line of more than one command (what runs besides it may
/// change the kubeconfig or the context), `kubectl config …` (options before `config` too), a plugin's verb
/// (`kubectl ctx` runs `kubectl-ctx`), an option before the verb whose value can't be told, or the cluster named
/// itself: can't be told, said why, in warning.
#[test]
fn only_one_kubectl_of_its_own_verbs_is_told() {
    let (_s, home, cwd) = places("kubeconfig-one");
    let said = |cmd: &str| said_in(cmd, &cwd, None, &home).unwrap();
    let default = format!("kubectl's context: prod-eks ({})", at(&home.join(".kube/config")));
    for cmd in [
        "kubectl get pods",
        "kubectl get pods;",
        "kubectl -n prod get pods",
        "kubectl -nprod get pods",
        "kubectl --insecure-skip-tls-verify --request-timeout=5s get pods",
        "kubectl --namespace prod delete ns x",
    ] {
        let s = said(cmd);
        assert_eq!(s.line, default, "{cmd}");
        assert!(s.named && !s.warns(), "{cmd}");
    }
    let cant = |why: &str| format!("kubectl's context: can't be told — {why}");
    for (cmd, why) in [
        ("kubectl ctx prod && kubectl delete ns x", "the line runs 2 commands, not kubectl alone"),
        (
            "kubectl config --kubeconfig f use-context prod; kubectl delete ns x",
            "the line runs 2 commands, not kubectl alone",
        ),
        (
            "aws eks update-kubeconfig --name x && kubectl get pods",
            "the line runs 2 commands, not kubectl alone",
        ),
        (
            "az aks get-credentials -n x -g y && kubectl get pods",
            "the line runs 2 commands, not kubectl alone",
        ),
        ("source env.sh; kubectl get pods", "the line runs 2 commands, not kubectl alone"),
        ("KUBECONFIG=dev.yaml; kubectl get pods", "the line runs 2 commands, not kubectl alone"),
        ("kubectl get pods | grep x", "the line runs 2 commands, not kubectl alone"),
        (
            "kubectl get ns && kubectl --context prod delete ns x",
            "the line runs 2 commands, not kubectl alone",
        ),
        (
            "kubectl get ns && ./switch.sh && kubectl delete ns x",
            "the line runs 3 commands, not kubectl alone",
        ),
        ("kubectl ctx prod", "kubectl ctx runs a plugin (kubectl-ctx), not kubectl itself"),
        (
            "kubectl --context prod ns kube-system",
            "kubectl ns runs a plugin (kubectl-ns), not kubectl itself",
        ),
        ("kubectl config view", "kubectl config reads or changes the kubeconfig itself"),
        (
            "kubectl config --kubeconfig f use-context prod",
            "kubectl config reads or changes the kubeconfig itself",
        ),
        (
            "kubectl --kubeconfig dev.yaml config use-context prod",
            "kubectl config reads or changes the kubeconfig itself",
        ),
        (
            "kubectl -n x config set-context --current",
            "kubectl config reads or changes the kubeconfig itself",
        ),
        ("kubectl --foo bar get pods", "--foo before its verb: whether it takes a value isn't known"),
        ("kubectl --server https://x.invalid get pods", "its --server names the cluster itself"),
        ("X=1 kubectl get pods", "the line sets X for kubectl"),
        ("HOME=/tmp/x kubectl get pods", "the line sets HOME for kubectl"),
        ("KUBECONFIG=dev.yaml HOME=/tmp/x kubectl get pods", "the line sets HOME for kubectl"),
        ("kubectl --kubeconfig $KC get pods", "$KC: a variable's — not read"),
    ] {
        let s = said(cmd);
        assert_eq!(s.line, cant(why), "{cmd}");
        assert!(!s.named && s.warns(), "{cmd}");
    }
    // The command's own `KUBECONFIG=`: its own
    let dev = format!("kubectl's context: dev ({})", at(&cwd.join("dev.yaml")));
    assert_eq!(said("KUBECONFIG=dev.yaml kubectl get ns").line, dev);
}

/// Told only where the line's program is kubectl itself (reviewer round 6): anything in front of it — `sudo` or
/// `doas` (root's home, `KUBECONFIG` dropped), `env` with or without options, `nice`/`timeout`, a script before a
/// `kubectl` word — can't be told, said by what's in front; its own `KUBECONFIG=` before that changes nothing.
#[test]
fn anything_in_front_of_kubectl_means_it_cant_be_told() {
    let (_s, home, cwd) = places("kubeconfig-front");
    let said = |cmd: &str| said_in(cmd, &cwd, None, &home).unwrap();
    for (cmd, front) in [
        ("sudo kubectl get pods", "sudo"),
        ("sudo -E kubectl get pods", "sudo"),
        ("doas kubectl delete ns x", "doas"),
        ("env kubectl get pods", "env"),
        ("env KUBECONFIG=dev.yaml kubectl get ns", "env"),
        ("env -u KUBECONFIG kubectl get pods", "env"),
        ("env -i PATH=/usr/bin kubectl get pods", "env"),
        ("env bash x.sh kubectl get pods", "env"),
        ("nice ./switch.sh kubectl get pods", "nice"),
        ("timeout 60 kubectl get ns", "timeout"),
        ("KUBECONFIG=dev.yaml timeout 60 kubectl get ns", "timeout"),
        ("exec kubectl get pods", "exec"),
    ] {
        let s = said(cmd);
        let want = format!(
            "kubectl's context: can't be told — kubectl runs under {front}, not as the line's own program"
        );
        assert_eq!(s.line, want, "{cmd}");
        assert!(!s.named && s.warns(), "{cmd}");
    }
    // (kubectl's own options twice, either spelling: the one it takes isn't the first read — one rule with the
    // variable's below)
    for (cmd, o) in [
        ("kubectl --context a --context b get pods", "--context"),
        ("kubectl --context=a get pods --context b", "--context"),
        ("kubectl --kubeconfig dev.yaml --kubeconfig=other.yaml get pods", "--kubeconfig"),
    ] {
        let s = said(cmd);
        assert_eq!(s.line, format!("kubectl's context: can't be told — its {o} is given twice"), "{cmd}");
        assert!(!s.named && s.warns(), "{cmd}");
    }
    // (set twice: the shell takes the last — not the first that was read)
    for cmd in [
        "KUBECONFIG=dev.yaml KUBECONFIG=~/.kube/prod kubectl get pods",
        "KUBECONFIG=dev.yaml KUBECONFIG=dev.yaml kubectl get pods",
    ] {
        let s = said(cmd);
        assert_eq!(s.line, "kubectl's context: can't be told — the line sets KUBECONFIG twice", "{cmd}");
        assert!(!s.named && s.warns(), "{cmd}");
    }
    for (cmd, name) in [("HOME=/tmp/x kubectl get pods", "HOME"), ("KUBECONFIG=a X=1 kubectl get ns", "X")] {
        let s = said(cmd);
        assert_eq!(
            s.line,
            format!("kubectl's context: can't be told — the line sets {name} for kubectl"),
            "{cmd}"
        );
    }
}

/// kubectl's own options end at `--` (reviewer round 3 #3): past it are the words of the program it runs.
#[test]
fn the_words_past_a_double_dash_are_the_programs_not_kubectls() {
    let (_s, home, cwd) = places("kubeconfig-dashes");
    let said = |cmd: &str| said_in(cmd, &cwd, None, &home).unwrap().line;
    let default = format!("kubectl's context: prod-eks ({})", at(&home.join(".kube/config")));
    for cmd in [
        "kubectl exec p -- ls -s /",
        "kubectl exec p -- app --context staging --server x",
        "kubectl exec p -- kubectl config use-context x",
    ] {
        assert_eq!(said(cmd), default, "{cmd}");
    }
    assert_eq!(said("kubectl --context staging exec p -- ls"), "kubectl's context: staging (its --context)");
}

/// Only a plain file is read (reviewer #1): never a terminal, a device or a FIFO — one would take the user's keys
/// or hold the worker for ever — nor one past the size; a link is followed to what it leads to, as kubectl does.
#[test]
fn only_a_plain_file_is_read_as_a_kubeconfig() {
    let (s, home, cwd) = places("kubeconfig-plain");
    let said = |f: &Path| said_in(&format!("kubectl --kubeconfig {} get ns", f.display()), &cwd, None, &home);
    let refused = |f: &Path| {
        let line = said(f).unwrap().line;
        let want = format!("kubectl's context: can't be told — {}: not a plain file", at(f));
        assert!(line.starts_with(&want), "{}: {line}", f.display());
    };
    refused(Path::new("/dev/null"));
    let fifo = s.join("fifo");
    let c = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
    // SAFETY: a path of the test's own folder, live for the call.
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    refused(&fifo);
    let to_null = s.join("to-null");
    std::os::unix::fs::symlink("/dev/null", &to_null).unwrap();
    refused(&to_null);
    let link = s.join("link.yaml");
    std::os::unix::fs::symlink(cwd.join("dev.yaml"), &link).unwrap();
    assert_eq!(said(&link).unwrap().line, format!("kubectl's context: dev ({})", at(&link)));
    let big = s.join("big.yaml");
    std::fs::write(&big, format!("current-context: x\n{}", "#".repeat(READ_MAX))).unwrap();
    assert_eq!(
        said(&big).unwrap().line,
        format!("kubectl's context: can't be told — {}: larger than 1 MB — not read", at(&big))
    );
    let folder = s.join("p");
    assert!(said(&folder).unwrap().line.ends_with(": a folder — not read"));
}

/// A kubeconfig Claude's runs can write (reviewer #4) — in the project, the temp folders — names a context that's
/// Claude's: said so on the line, never as decided; home's isn't. Runs not sandboxed write anywhere.
#[test]
fn a_kubeconfig_claudes_runs_can_write_is_said_so_never_decided() {
    let (_s, home, cwd) = places("kubeconfig-written");
    let writes = [cwd.clone()];
    let said = |cmd: &str, writes: Option<&[PathBuf]>| {
        said(cmd, &Place { cwd: &cwd, env: None, home: Some(&home), writes }).unwrap()
    };
    let dev = at(&cwd.join("dev.yaml"));
    let s = said("kubectl --kubeconfig dev.yaml get pods", Some(&writes));
    assert_eq!(s.line, format!("kubectl's context: dev ({dev} — Claude's runs can write it)"));
    assert!(s.written && s.warns());
    let s = said("kubectl --kubeconfig dev.yaml --context prod get pods", Some(&writes));
    assert_eq!(s.line, format!("kubectl's context: prod (its --context — Claude's runs can write {dev})"));
    assert!(s.written && s.warns(), "its clusters are the file's");
    // A link in the project to home's: the link is Claude's to change
    std::os::unix::fs::symlink(home.join(".kube/config"), cwd.join("kc")).unwrap();
    assert!(said("kubectl --kubeconfig kc get pods", Some(&writes)).written);
    let s = said("kubectl get pods", Some(&writes));
    assert!(!s.written && !s.warns(), "home's: {}", s.line);
    let s = said("kubectl get pods", None);
    assert!(s.written && s.warns(), "no sandbox: {}", s.line);
}

#[test]
fn only_a_plain_top_level_name_is_read_as_the_context() {
    assert_eq!(current_in("current-context: a # b\n"), Ok(Some("a".into())));
    assert_eq!(
        current_in("current-context: 'arn:aws:eks:r:1:cluster/x'\n"),
        Ok(Some("arn:aws:eks:r:1:cluster/x".into()))
    );
    assert_eq!(
        current_in("contexts:\n- name: x\n  current-context: y\n"),
        Ok(None),
        "indented: not the top level"
    );
    assert_eq!(current_in("current-context:\n"), Ok(None));
    assert!(current_in("{\"current-context\": \"x\"}").is_err(), "JSON: said, not guessed");
    assert!(current_in("current-context: \"a\\\"b\"\n").is_err(), "an escape: not a plain name");
}
