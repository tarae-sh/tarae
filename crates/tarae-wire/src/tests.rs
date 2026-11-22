use crate::check;
use crate::codec::{Decoder, Error, MSG_MAX, Message, encode};
use crate::mailbox::{IN_FLIGHT, Next, Order, Slot};
use crate::msg::*;
use tarae_testkit::mutate::{self, Rng};

fn bytes<M: Message>(m: &M) -> Vec<u8> {
    let mut out = Vec::new();
    encode(m, &mut out).unwrap();
    out
}

fn decode_all<M: Message>(b: &[u8]) -> Result<Vec<M>, Error> {
    let mut d = Decoder::<M>::new();
    d.push(b);
    let mut out = Vec::new();
    while let Some(m) = d.message()? {
        out.push(m);
    }
    Ok(out)
}

/// A message of `kind` with `body`, framed.
fn raw(kind: u8, body: &[u8]) -> Vec<u8> {
    let mut b = ((body.len() + 1) as u32).to_le_bytes().to_vec();
    b.push(kind);
    b.extend_from_slice(body);
    b
}

fn refused<M: Message + std::fmt::Debug>(b: &[u8]) -> String {
    match decode_all::<M>(b) {
        Err(e) => e.said,
        Ok(m) => panic!("read {m:?} from {b:?}"),
    }
}

#[test]
fn every_message_reads_back_as_it_was_written() {
    for m in check::to_host() {
        assert_eq!(decode_all::<ToHost>(&bytes(&m)).unwrap(), vec![m.clone()], "{m:?}");
    }
    for m in check::to_interface() {
        assert_eq!(decode_all::<ToInterface>(&bytes(&m)).unwrap(), vec![m.clone()], "{m:?}");
    }
    // (the samples cover every kind of each side: a new message is a sample too)
    let kinds = |k: Vec<u8>| k.into_iter().collect::<std::collections::BTreeSet<_>>();
    assert_eq!(kinds(check::to_host().iter().map(Message::kind).collect()), (1..=8).collect());
    assert_eq!(kinds(check::to_interface().iter().map(Message::kind).collect()), (0x81..=0x86).collect());
}

#[test]
fn a_stream_read_a_byte_at_a_time_reads_the_same_messages() {
    let (host, iface) = check::streams();
    for chunk in [1, 2, 3, 7, 64, host.len()] {
        let mut d = Decoder::<ToHost>::new();
        let mut got = Vec::new();
        for piece in host.chunks(chunk) {
            d.push(piece);
            while let Some(m) = d.message().unwrap() {
                got.push(m);
            }
        }
        assert_eq!(got, check::to_host(), "{chunk} at a time");
        assert_eq!(d.pending(), 0);
        check::decode::<ToInterface>(&iface, chunk);
    }
}

#[test]
fn the_decoder_reads_from_a_stream_and_says_a_message_cut_at_its_end() {
    let (host, _) = check::streams();
    let cut = &host[..host.len() - 1];
    let mut r = std::io::Cursor::new(cut);
    let mut d = Decoder::<ToHost>::new();
    let mut got = 0;
    while d.read_from(&mut r).unwrap() > 0 {
        while d.message().unwrap().is_some() {
            got += 1;
        }
    }
    assert_eq!(got, check::to_host().len() - 1, "the last message (a Ping) cut");
    assert_eq!(d.pending(), 8, "the Ping's length, kind and 3 of its 4 bytes");
}

#[test]
fn every_malformed_message_ends_the_stream_with_what_was_wrong() {
    let hello = bytes(&check::to_host()[0]);
    let key = |code: &[u8]| {
        let mut body = vec![0; 16];
        body.push(0);
        body.extend_from_slice(code);
        body.extend_from_slice(&[0, 0, 0]);
        raw(0x02, &body)
    };
    let cases: Vec<(Vec<u8>, &str)> = vec![
        (raw(0x09, &[]), "kind 0x09"),
        (raw(0x81, &[]), "kind 0x81, none such from an interface"),
        (vec![0, 0, 0, 0], "an empty message"),
        (((MSG_MAX + 1) as u32).to_le_bytes().to_vec(), "past the 16 MiB"),
        (raw(0x04, &[1, 2, 3]), "cut short"),
        (raw(0x04, &[0; 9]), "1 bytes left over"),
        (raw(0x07, &[0]), "1 bytes left over"),
        (key(&[0, 0, 0xD8, 0, 0]), "character of 0xd800"),
        (key(&[0, 0, 0, 0x11, 0]), "character of 0x110000"),
        (key(&[16]), "a key of tag 16"),
        (raw(0x02, &[[0; 16].as_slice(), &[5]].concat()), "an input of tag 5"),
        (raw(0x02, &[[0; 16].as_slice(), &[1, 10, 0, 0, 0, 0, 0, 0]].concat()), "a mouse action of tag 10"),
        (raw(0x02, &[[0; 16].as_slice(), &[4, 2]].concat()), "a yes or no of 2"),
        (raw(0x02, &[[0; 16].as_slice(), &[3, 2, 0, 0, 0, 0xC3, 0x28]].concat()), "isn't UTF-8"),
        (raw(0x02, &[[0; 16].as_slice(), &[3, 255, 255, 255, 255]].concat()), "cut short"),
        (raw(0x05, &[0, 0, 0, 0, 5]), "an answer of tag 5"),
        (raw(0x05, &[0, 0, 0, 0, 0, 3]), "a copy's way of tag 3"),
        (raw(0x05, &[0, 0, 0, 0, 2, 2]), "a reason of tag 2"),
    ];
    for (b, says) in cases {
        let said = refused::<ToHost>(&b);
        assert!(said.contains(says), "{b:?}: {said:?}, wanted {says:?}");
        // (the same after a good message, and again on the next call)
        let mut d = Decoder::<ToHost>::new();
        d.push(&[hello.clone(), b].concat());
        assert!(d.message().unwrap().is_some());
        assert_eq!(d.message().unwrap_err().said, said);
        assert_eq!(d.message().unwrap_err().said, said);
    }
    // A Hello's ground and clipboard
    let mut bad = hello.clone();
    let ground = 4 + 1 + 2 + 4 + 5 + 4;
    bad[ground] = 3;
    assert!(refused::<ToHost>(&bad).contains("a ground of tag 3"));
    let mut bad = hello.clone();
    // (past the ground, undercurl and `taken`'s length — empty in the sample)
    bad[ground + 6] = 3;
    assert!(refused::<ToHost>(&bad).contains("a clipboard of tag 3"));

    let frame = |tail: &[u8]| raw(0x82, &[[0; 20].as_slice(), tail].concat());
    let cases: Vec<(Vec<u8>, &str)> = vec![
        (raw(0x01, &[]), "kind 0x01, none such from a host"),
        (frame(&[9, 0, 0, 0, 1]), "cut short"),
        (raw(0x83, &[0, 0, 0, 0, 3]), "an ask of tag 3"),
        (raw(0x84, &[3]), "a sound of tag 3"),
        (raw(0x84, &[1, 0, 0, 0, 0xC0, 0x7F, 0, 0, 0, 0, 0, 0, 0, 0]), "isn't finite"),
        (raw(0x84, &[1, 0, 0, 0, 0x80, 0x7F, 0, 0, 0, 0, 0, 0, 0, 0]), "isn't finite"),
        (raw(0x85, &[6, 0, 0, 0, 0]), "a reason to go of tag 6"),
        (raw(0x85, &[0]), "cut short"),
    ];
    for (b, says) in cases {
        let said = refused::<ToInterface>(&b);
        assert!(said.contains(says), "{b:?}: {said:?}, wanted {says:?}");
    }
}

#[test]
fn a_length_past_the_limit_is_refused_before_its_body_comes() {
    // (four bytes are enough to say no: nothing is waited for, nothing made of that size)
    let mut d = Decoder::<ToInterface>::new();
    d.push(&u32::MAX.to_le_bytes());
    assert!(d.message().unwrap_err().said.contains("4294967295 bytes"));
    // and a message that would be past it isn't written
    let big = ToInterface::Ask { id: 1, ask: Ask::Copy("x".repeat(MSG_MAX)) };
    let mut out = b"kept".to_vec();
    assert!(encode(&big, &mut out).unwrap_err().said.contains("past the 16 MiB"));
    assert_eq!(out, b"kept");
}

/// The decoder's never-panic loop: the sample streams mutated, read every way the fuzz target reads them.
#[test]
fn the_decoder_never_panics_on_mutated_streams() {
    let (host, iface) = check::streams();
    let words: &[&[u8]] =
        &[&[0xff, 0xff, 0xff, 0xff], &[0, 0, 0, 0], &[1, 0, 0, 0], &[0x82], &[0xD8], &[0xC3]];
    let mut rng = Rng(0x7a7a_e3e3);
    for i in 0..4000 {
        let mut b = if i % 2 == 0 { host.clone() } else { iface.clone() };
        mutate::bytes(&mut rng, &mut b, words);
        b.insert(0, rng.next() as u8 & !1 | (i % 2) as u8);
        check::decode_input(&b);
    }
}

#[test]
#[ignore = "writes fuzz/parsers' seeds: run by hand when a message changes"]
fn writes_fuzz_seeds() {
    let (host, iface) = check::streams();
    mutate::fuzz_seeds(
        "wire_decode",
        [
            ("to-host".into(), [&[6u8][..], &host].concat()),
            ("to-interface".into(), [&[7u8][..], &iface].concat()),
        ],
    );
}

fn frames(slot: &mut Slot<&'static str, &'static str>) -> Vec<String> {
    let mut out = Vec::new();
    loop {
        match slot.take() {
            Next::Frame(n, f) => out.push(format!("{n}:{f}")),
            Next::Msg(m) => out.push(m.to_string()),
            Next::Wait => break,
            Next::Closed => {
                out.push("closed".into());
                break;
            }
        }
    }
    out
}

#[test]
fn a_frame_not_sent_is_replaced_and_frames_wait_for_credit() {
    let mut s = Slot::default();
    assert_eq!(s.put_frame(1, "a"), None);
    assert_eq!(s.put_frame(2, "b"), Some("a"), "replaced, its buffer back");
    assert_eq!(frames(&mut s), ["2:b"]);
    s.put_frame(3, "c");
    assert_eq!(frames(&mut s), ["3:c"]);
    assert_eq!(s.in_flight(), IN_FLIGHT);
    s.put_frame(4, "d");
    assert_eq!(s.put_frame(5, "e"), Some("d"));
    assert_eq!(frames(&mut s), Vec::<String>::new(), "no credit");
    s.ack(2);
    assert_eq!(frames(&mut s), ["5:e"], "the newest only");
    assert_eq!(s.skipped(), 2);
    s.ack(5);
    assert_eq!(s.in_flight(), 0);
}

#[test]
fn an_ask_goes_after_its_frame_and_a_sound_at_once() {
    let mut s = Slot::default();
    s.put_frame(1, "a");
    s.put_frame(2, "b");
    assert_eq!(frames(&mut s), ["2:b"]);
    s.put_frame(3, "c");
    s.push("copy", Order::AfterFrame, 4);
    s.push("wake", Order::AtOnce, 1);
    assert_eq!(s.pending(), 5);
    assert_eq!(frames(&mut s), ["wake", "3:c", "copy"]);
    s.put_frame(4, "d");
    s.push("paste", Order::AfterFrame, 0);
    s.put_frame(5, "e");
    s.push("play", Order::AtOnce, 0);
    assert_eq!(frames(&mut s), ["play"], "no credit: the ask waits behind its frame, the sound doesn't");
    s.ack(3);
    assert_eq!(frames(&mut s), ["5:e", "paste"], "after the frame that replaced its own");
    assert_eq!(s.pending(), 0);
}

#[test]
fn closed_the_queue_goes_and_the_frame_doesnt() {
    let mut s = Slot::default();
    s.put_frame(1, "a");
    s.put_frame(2, "b");
    assert_eq!(frames(&mut s), ["2:b"]);
    s.put_frame(3, "c");
    s.put_frame(4, "d");
    s.push("gone", Order::AfterFrame, 0);
    s.close();
    assert_eq!(s.put_frame(5, "e"), Some("e"));
    assert_eq!(frames(&mut s), ["gone", "closed"]);
    assert_eq!(s.take_spare(), Some("d"));
}

#[test]
fn the_mailbox_wakes_its_writer() {
    let m = std::sync::Arc::new(crate::Mailbox::<u64, &'static str>::new());
    let w = std::thread::spawn({
        let m = m.clone();
        move || {
            let mut got = Vec::new();
            loop {
                match m.wait() {
                    Next::Frame(n, _) => {
                        got.push(n);
                        m.with(|s| s.ack(n));
                    }
                    Next::Msg(_) => {}
                    Next::Closed => return got,
                    Next::Wait => unreachable!("wait returns only what goes"),
                }
            }
        }
    });
    for n in 1..=50 {
        m.with(|s| s.put_frame(n, n));
    }
    // (the last frame goes before close: once the writer took it)
    while m.with(|s| s.last_sent()) != 50 {
        std::thread::yield_now();
    }
    m.with(|s| s.close());
    let got = w.join().unwrap();
    assert!(got.windows(2).all(|p| p[0] < p[1]), "in order: {got:?}");
    assert_eq!(got.last(), Some(&50));
}

// ── Build stamp (`stamp.rs`, #684 option D) ──────────────────────────────────────

mod stamp_tests {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    use crate::stamp::{GitStamp, inspect, watch_paths};
    use tarae_testkit::git::test_git;
    use tarae_testkit::scratch::Scratch;

    fn paths(list: &[&str]) -> Vec<PathBuf> {
        list.iter().map(PathBuf::from).collect()
    }

    fn git_ok(dir: &Path, args: &[&str]) -> String {
        let out = test_git(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
            .args(args)
            .output()
            .expect("spawn git");
        assert!(
            out.status.success(),
            "git {args:?} failed in {}: {}",
            dir.display(),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn run_inspect(root: &Path, sources: &[&str]) -> Option<GitStamp> {
        inspect(root, sources, |args| {
            let out = test_git(root)
                .args(["-c", "core.fsmonitor=", "--no-optional-locks"])
                .args(args)
                .env("GIT_OPTIONAL_LOCKS", "0")
                .output()
                .ok()?;
            out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
        })
    }

    fn short_head(dir: &Path) -> String {
        git_ok(dir, &["rev-parse", "--short=8", "HEAD"])
    }

    fn assert_all_exist(watches: &[PathBuf], label: &str) {
        assert!(!watches.is_empty(), "{label}: expected at least one watch path");
        for p in watches {
            assert!(p.exists(), "{label}: watch path does not exist: {}", p.display());
        }
    }

    fn assert_covered(watches: &[PathBuf], target: &Path, label: &str) {
        let hit = watches.iter().any(|w| if w.is_dir() { target.starts_with(w) } else { target == w });
        assert!(hit, "{label}: {} is not covered by watches {watches:?}", target.display());
    }

    fn init_repo(scratch: &Scratch, name: &str) -> PathBuf {
        let root = std::fs::canonicalize(scratch.path()).unwrap().join(name);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/lib.rs"), "// init\n").unwrap();
        git_ok(&root, &["init", "-q", "-b", "main"]);
        git_ok(&root, &["add", "-A"]);
        git_ok(&root, &["commit", "-qm", "init"]);
        root
    }

    /// Pure table for `watch_paths` across every git state (loose, packed, nested packed with/without prefix dir,
    /// unborn, detached, linked worktree loose & packed, and reftable main & worktree).
    #[test]
    fn each_git_state_watches_only_files_that_exist_and_move_with_head() {
        struct Case {
            name: &'static str,
            dir: &'static str,
            common: &'static str,
            symref: Option<&'static str>,
            present: &'static [&'static str],
            want: &'static [&'static str],
        }
        let cases = [
            Case {
                name: "loose_no_packed_refs",
                dir: "/r/.git",
                common: "/r/.git",
                symref: Some("refs/heads/main"),
                present: &["/r/.git/HEAD", "/r/.git/refs/heads", "/r/.git/refs/heads/main"],
                want: &["/r/.git/HEAD", "/r/.git/refs/heads/main"],
            },
            Case {
                name: "loose_shadows_packed_refs",
                dir: "/r/.git",
                common: "/r/.git",
                symref: Some("refs/heads/main"),
                present: &[
                    "/r/.git/HEAD",
                    "/r/.git/packed-refs",
                    "/r/.git/refs/heads",
                    "/r/.git/refs/heads/main",
                ],
                want: &["/r/.git/HEAD", "/r/.git/refs/heads/main"],
            },
            Case {
                name: "packed_top_level",
                dir: "/r/.git",
                common: "/r/.git",
                symref: Some("refs/heads/main"),
                present: &["/r/.git/HEAD", "/r/.git/packed-refs", "/r/.git/refs", "/r/.git/refs/heads"],
                want: &["/r/.git/HEAD", "/r/.git/packed-refs", "/r/.git/refs/heads"],
            },
            Case {
                name: "packed_nested_with_prefix_dir",
                dir: "/r/.git",
                common: "/r/.git",
                symref: Some("refs/heads/feat/foo"),
                present: &[
                    "/r/.git/HEAD",
                    "/r/.git/packed-refs",
                    "/r/.git/refs",
                    "/r/.git/refs/heads",
                    "/r/.git/refs/heads/feat",
                ],
                want: &["/r/.git/HEAD", "/r/.git/packed-refs", "/r/.git/refs/heads/feat"],
            },
            Case {
                name: "packed_nested_pruned_prefix_dir",
                dir: "/r/.git",
                common: "/r/.git",
                symref: Some("refs/heads/feat/foo"),
                present: &["/r/.git/HEAD", "/r/.git/packed-refs", "/r/.git/refs", "/r/.git/refs/heads"],
                want: &["/r/.git/HEAD", "/r/.git/packed-refs", "/r/.git/refs/heads"],
            },
            Case {
                name: "packed_nested_only_refs_dir",
                dir: "/r/.git",
                common: "/r/.git",
                symref: Some("refs/heads/feat/foo"),
                present: &["/r/.git/HEAD", "/r/.git/packed-refs", "/r/.git/refs"],
                want: &["/r/.git/HEAD", "/r/.git/packed-refs", "/r/.git/refs"],
            },
            Case {
                name: "unborn_no_packed_refs",
                dir: "/r/.git",
                common: "/r/.git",
                symref: Some("refs/heads/main"),
                present: &["/r/.git/HEAD", "/r/.git/refs", "/r/.git/refs/heads"],
                want: &["/r/.git/HEAD", "/r/.git/refs/heads"],
            },
            Case {
                name: "detached_head",
                dir: "/r/.git",
                common: "/r/.git",
                symref: None,
                present: &["/r/.git/HEAD", "/r/.git/packed-refs", "/r/.git/refs/heads"],
                want: &["/r/.git/HEAD"],
            },
            Case {
                name: "worktree_loose",
                dir: "/r/.git/worktrees/wt",
                common: "/r/.git",
                symref: Some("refs/heads/wt"),
                present: &[
                    "/r/.git/worktrees/wt/HEAD",
                    "/r/.git/packed-refs",
                    "/r/.git/refs/heads",
                    "/r/.git/refs/heads/wt",
                ],
                want: &["/r/.git/worktrees/wt/HEAD", "/r/.git/refs/heads/wt"],
            },
            Case {
                name: "worktree_packed",
                dir: "/r/.git/worktrees/wt",
                common: "/r/.git",
                symref: Some("refs/heads/wt"),
                present: &["/r/.git/worktrees/wt/HEAD", "/r/.git/packed-refs", "/r/.git/refs/heads"],
                want: &["/r/.git/worktrees/wt/HEAD", "/r/.git/packed-refs", "/r/.git/refs/heads"],
            },
            Case {
                name: "reftable_main_deduped",
                dir: "/r/.git",
                common: "/r/.git",
                symref: Some("refs/heads/main"),
                present: &["/r/.git/HEAD", "/r/.git/reftable/tables.list"],
                want: &["/r/.git/reftable/tables.list"],
            },
            Case {
                name: "reftable_worktree_both",
                dir: "/r/.git/worktrees/wt",
                common: "/r/.git",
                symref: Some("refs/heads/wt"),
                present: &[
                    "/r/.git/worktrees/wt/HEAD",
                    "/r/.git/worktrees/wt/reftable/tables.list",
                    "/r/.git/reftable/tables.list",
                ],
                want: &["/r/.git/worktrees/wt/reftable/tables.list", "/r/.git/reftable/tables.list"],
            },
            Case {
                name: "reftable_worktree_common_only",
                dir: "/r/.git/worktrees/wt",
                common: "/r/.git",
                symref: Some("refs/heads/wt"),
                present: &["/r/.git/worktrees/wt/HEAD", "/r/.git/reftable/tables.list"],
                want: &["/r/.git/reftable/tables.list"],
            },
            Case {
                name: "ancestor_never_climbs_to_common",
                dir: "/r/.git",
                common: "/r/.git",
                symref: Some("refs/heads/main"),
                present: &["/r/.git", "/r/.git/HEAD"],
                want: &["/r/.git/HEAD"],
            },
        ];

        for c in cases {
            let set: BTreeSet<PathBuf> = c.present.iter().map(PathBuf::from).collect();
            let got = watch_paths(Path::new(c.dir), Path::new(c.common), c.symref, |p| set.contains(p));
            assert_eq!(got, paths(c.want), "case {}", c.name);
        }
    }

    /// Real-repo states 1–3: loose (no `packed-refs`), packed (`git pack-refs --all`), commit after packing,
    /// and nested packed `feat/foo` then a commit.
    #[test]
    fn a_commit_moves_the_stamp_whether_its_branch_is_loose_or_packed() {
        let scratch = Scratch::new("wire-stamp-123");
        let root = init_repo(&scratch, "repo");
        let git_dir = root.join(".git");

        // State 1: fresh loose ref, no packed-refs
        assert!(!git_dir.join("packed-refs").exists());
        let s1 = run_inspect(&root, &["src"]).expect("in git repo");
        assert_all_exist(&s1.watches, "state 1 loose");
        assert_eq!(s1.watches, vec![git_dir.join("HEAD"), git_dir.join("refs/heads/main")]);
        assert_eq!(s1.suffix.as_deref(), Some(short_head(&root).as_str()));

        // Dirty tracked source adds '+'
        std::fs::write(root.join("src/lib.rs"), "// dirty\n").unwrap();
        let dirty = run_inspect(&root, &["src"]).unwrap();
        assert_eq!(dirty.suffix, Some(format!("{}+", short_head(&root))));
        std::fs::write(root.join("src/lib.rs"), "// init\n").unwrap();

        // Empty commit updates loose ref (covered by watches) and next inspect gives new sha
        let sha1 = short_head(&root);
        git_ok(&root, &["commit", "--allow-empty", "-qm", "two"]);
        let sha2 = short_head(&root);
        assert_ne!(sha1, sha2);
        assert_covered(&s1.watches, &git_dir.join("refs/heads/main"), "state 1 commit");
        let s1_after = run_inspect(&root, &["src"]).unwrap();
        assert_eq!(s1_after.suffix.as_deref(), Some(sha2.as_str()));

        // State 2: all packed (loose ref deleted)
        git_ok(&root, &["pack-refs", "--all"]);
        assert!(!git_dir.join("refs/heads/main").exists());
        assert!(git_dir.join("packed-refs").exists());
        let s2 = run_inspect(&root, &["src"]).unwrap();
        assert_all_exist(&s2.watches, "state 2 packed");
        assert_eq!(
            s2.watches,
            vec![git_dir.join("HEAD"), git_dir.join("packed-refs"), git_dir.join("refs/heads")]
        );

        // State 3a: commit after packing on main creates loose ref under watched dir and narrows back
        git_ok(&root, &["commit", "--allow-empty", "-qm", "three"]);
        let sha3 = short_head(&root);
        assert_ne!(sha2, sha3);
        assert_covered(&s2.watches, &git_dir.join("refs/heads/main"), "state 3a commit");
        let s3a = run_inspect(&root, &["src"]).unwrap();
        assert_all_exist(&s3a.watches, "state 3a after");
        assert_eq!(s3a.suffix.as_deref(), Some(sha3.as_str()));
        assert_eq!(s3a.watches, vec![git_dir.join("HEAD"), git_dir.join("refs/heads/main")]);

        // State 3b: nested packed feat/foo then a commit
        git_ok(&root, &["checkout", "-qb", "feat/foo"]);
        git_ok(&root, &["commit", "--allow-empty", "-qm", "feat-1"]);
        let feat1 = short_head(&root);
        git_ok(&root, &["pack-refs", "--all"]);
        assert!(!git_dir.join("refs/heads/feat/foo").exists());
        let s3b = run_inspect(&root, &["src"]).unwrap();
        assert_all_exist(&s3b.watches, "state 3b packed feat/foo");
        git_ok(&root, &["commit", "--allow-empty", "-qm", "feat-2"]);
        let feat2 = short_head(&root);
        assert_ne!(feat1, feat2);
        assert_covered(&s3b.watches, &git_dir.join("refs/heads/feat/foo"), "state 3b feat/foo commit");
        let s3b_after = run_inspect(&root, &["src"]).unwrap();
        assert_all_exist(&s3b_after.watches, "state 3b after");
        assert_eq!(s3b_after.suffix.as_deref(), Some(feat2.as_str()));
        assert_eq!(s3b_after.watches, vec![git_dir.join("HEAD"), git_dir.join("refs/heads/feat/foo")]);
    }

    /// Real-repo states 4–6: detached HEAD (+ commit), linked worktree (loose / packed / commit), and no git.
    #[test]
    fn a_detached_head_or_a_linked_worktree_moves_the_stamp_and_no_git_watches_nothing() {
        let scratch = Scratch::new("wire-stamp-456");
        let base = std::fs::canonicalize(scratch.path()).unwrap();

        // State 6: no git
        let plain = base.join("no-git");
        std::fs::create_dir_all(&plain).unwrap();
        assert_eq!(run_inspect(&plain, &["src"]), None);

        // State 4: detached HEAD (no packed-refs) + commit
        let root = init_repo(&scratch, "repo");
        let git_dir = root.join(".git");
        git_ok(&root, &["checkout", "-q", "--detach", "HEAD"]);
        assert!(!git_dir.join("packed-refs").exists());
        let s4 = run_inspect(&root, &["src"]).unwrap();
        assert_all_exist(&s4.watches, "state 4 detached");
        assert_eq!(s4.watches, vec![git_dir.join("HEAD")]);
        let d1 = short_head(&root);
        git_ok(&root, &["commit", "--allow-empty", "-qm", "detached"]);
        let d2 = short_head(&root);
        assert_ne!(d1, d2);
        assert_covered(&s4.watches, &git_dir.join("HEAD"), "state 4 detached commit");
        assert_eq!(run_inspect(&root, &["src"]).unwrap().suffix.as_deref(), Some(d2.as_str()));

        // State 5: linked worktree (loose -> commit -> packed -> commit)
        let wt = base.join("wt");
        git_ok(&root, &["worktree", "add", "-q", "-b", "wt-branch", wt.to_str().unwrap()]);
        let wt_dir = PathBuf::from(git_ok(&wt, &["rev-parse", "--absolute-git-dir"]));
        let common = PathBuf::from(git_ok(&wt, &["rev-parse", "--path-format=absolute", "--git-common-dir"]));

        // Loose worktree watches exact loose ref only, never common/refs/heads
        let s5_loose = run_inspect(&wt, &["src"]).unwrap();
        assert_all_exist(&s5_loose.watches, "state 5 loose wt");
        assert_eq!(s5_loose.watches, vec![wt_dir.join("HEAD"), common.join("refs/heads/wt-branch")]);
        assert!(!s5_loose.watches.contains(&common.join("refs/heads")));
        let w1 = short_head(&wt);
        git_ok(&wt, &["commit", "--allow-empty", "-qm", "wt-1"]);
        let w2 = short_head(&wt);
        assert_ne!(w1, w2);
        assert_covered(&s5_loose.watches, &common.join("refs/heads/wt-branch"), "state 5 loose commit");
        assert_eq!(run_inspect(&wt, &["src"]).unwrap().suffix.as_deref(), Some(w2.as_str()));

        // Packed worktree watches HEAD + packed-refs + refs/heads, catches commit, narrows back
        git_ok(&wt, &["pack-refs", "--all"]);
        assert!(!common.join("refs/heads/wt-branch").exists());
        let s5_packed = run_inspect(&wt, &["src"]).unwrap();
        assert_all_exist(&s5_packed.watches, "state 5 packed wt");
        assert_eq!(
            s5_packed.watches,
            vec![wt_dir.join("HEAD"), common.join("packed-refs"), common.join("refs/heads")]
        );
        git_ok(&wt, &["commit", "--allow-empty", "-qm", "wt-2"]);
        let w3 = short_head(&wt);
        assert_ne!(w2, w3);
        assert_covered(&s5_packed.watches, &common.join("refs/heads/wt-branch"), "state 5 packed commit");
        let s5_after = run_inspect(&wt, &["src"]).unwrap();
        assert_all_exist(&s5_after.watches, "state 5 after");
        assert_eq!(s5_after.suffix.as_deref(), Some(w3.as_str()));
        assert_eq!(s5_after.watches, vec![wt_dir.join("HEAD"), common.join("refs/heads/wt-branch")]);
    }

    /// Edge cases: `core.logAllRefUpdates=false` (no reflogs) and unborn repo -> first commit.
    #[test]
    fn the_first_commit_moves_the_stamp_even_without_reflogs() {
        let scratch = Scratch::new("wire-stamp-edges");
        let base = std::fs::canonicalize(scratch.path()).unwrap();

        // Edge 1: core.logAllRefUpdates=false
        let no_log = init_repo(&scratch, "no-log");
        let git_dir = no_log.join(".git");
        git_ok(&no_log, &["config", "core.logAllRefUpdates", "false"]);
        let _ = std::fs::remove_dir_all(git_dir.join("logs"));
        git_ok(&no_log, &["pack-refs", "--all"]);
        assert!(!git_dir.join("logs/HEAD").exists());
        let before = run_inspect(&no_log, &["src"]).unwrap();
        assert_all_exist(&before.watches, "no-reflog packed");
        let c1 = short_head(&no_log);
        git_ok(&no_log, &["commit", "--allow-empty", "-qm", "no-log-2"]);
        let c2 = short_head(&no_log);
        assert_ne!(c1, c2);
        assert_covered(&before.watches, &git_dir.join("refs/heads/main"), "no-reflog commit");
        assert_eq!(run_inspect(&no_log, &["src"]).unwrap().suffix.as_deref(), Some(c2.as_str()));

        // Edge 2: unborn repo (0 commits) -> suffix is None (version alone), watches emitted before rev-parse
        let unborn = base.join("unborn");
        std::fs::create_dir_all(unborn.join("src")).unwrap();
        std::fs::write(unborn.join("src/lib.rs"), "// unborn\n").unwrap();
        git_ok(&unborn, &["init", "-q", "-b", "main"]);
        let u_git = unborn.join(".git");
        let u_before = run_inspect(&unborn, &["src"]).expect("unborn is still a git repo");
        assert_eq!(u_before.suffix, None, "unborn repo emits version alone (no suffix)");
        assert_all_exist(&u_before.watches, "unborn");
        assert_eq!(u_before.watches, vec![u_git.join("HEAD"), u_git.join("refs/heads")]);

        // First commit creates refs/heads/main under watched refs/heads and yields sha
        git_ok(&unborn, &["add", "-A"]);
        git_ok(&unborn, &["commit", "-qm", "first"]);
        let first = short_head(&unborn);
        assert_covered(&u_before.watches, &u_git.join("refs/heads/main"), "unborn first commit");
        let u_after = run_inspect(&unborn, &["src"]).unwrap();
        assert_all_exist(&u_after.watches, "unborn after first commit");
        assert_eq!(u_after.suffix.as_deref(), Some(first.as_str()));
        assert_eq!(u_after.watches, vec![u_git.join("HEAD"), u_git.join("refs/heads/main")]);
    }

    /// Real reftable repository via `git init --ref-format=reftable` (skipped out loud if the installed git lacks
    /// reftable support).
    #[test]
    fn a_reftable_repository_moves_the_stamp_through_its_tables_list() {
        let scratch = Scratch::new("wire-stamp-reftable");
        let base = std::fs::canonicalize(scratch.path()).unwrap();
        let root = base.join("rt");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/lib.rs"), "// rt\n").unwrap();

        let init = test_git(&root)
            .args(["init", "-q", "-b", "main", "--ref-format=reftable"])
            .output()
            .expect("spawn git");
        if !init.status.success() {
            let ver = test_git(&root)
                .arg("--version")
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_else(|_| "unknown git".into());
            let why = String::from_utf8_lossy(&init.stderr).trim().to_string();
            eprintln!(
                "tarae-test: skipped a_reftable_repository_moves_the_stamp_through_its_tables_list — {ver} has no \
                 --ref-format=reftable: {why}"
            );
            return;
        }

        let git_dir = root.join(".git");
        let tables = git_dir.join("reftable/tables.list");
        assert!(tables.exists(), "reftable tables.list missing");

        // Unborn reftable repo
        let unborn = run_inspect(&root, &["src"]).unwrap();
        assert_eq!(unborn.suffix, None);
        assert_all_exist(&unborn.watches, "reftable unborn");
        assert_eq!(unborn.watches, vec![tables.clone()]);

        // First commit + follow-up commit
        git_ok(&root, &["add", "-A"]);
        git_ok(&root, &["commit", "-qm", "rt-1"]);
        let r1 = short_head(&root);
        assert_covered(&unborn.watches, &tables, "reftable first commit");
        let s1 = run_inspect(&root, &["src"]).unwrap();
        assert_eq!(s1.suffix.as_deref(), Some(r1.as_str()));
        assert_eq!(s1.watches, vec![tables.clone()]);

        git_ok(&root, &["commit", "--allow-empty", "-qm", "rt-2"]);
        let r2 = short_head(&root);
        assert_ne!(r1, r2);
        assert_covered(&s1.watches, &tables, "reftable second commit");
        assert_eq!(run_inspect(&root, &["src"]).unwrap().suffix.as_deref(), Some(r2.as_str()));

        // Linked worktree on a reftable repo
        let wt = base.join("rt-wt");
        git_ok(&root, &["worktree", "add", "-q", "-b", "rt-wt", wt.to_str().unwrap()]);
        let wt_dir = PathBuf::from(git_ok(&wt, &["rev-parse", "--absolute-git-dir"]));
        let s_wt = run_inspect(&wt, &["src"]).unwrap();
        assert_all_exist(&s_wt.watches, "reftable worktree");
        assert!(s_wt.watches.contains(&tables), "common tables.list watched: {:?}", s_wt.watches);
        if wt_dir.join("reftable/tables.list").exists() {
            assert!(s_wt.watches.contains(&wt_dir.join("reftable/tables.list")));
        }
        git_ok(&wt, &["commit", "--allow-empty", "-qm", "rt-wt-1"]);
        let rw = short_head(&wt);
        assert_ne!(r2, rw);
        assert_covered(&s_wt.watches, &tables, "reftable worktree commit");
        assert_eq!(run_inspect(&wt, &["src"]).unwrap().suffix.as_deref(), Some(rw.as_str()));
    }
}
