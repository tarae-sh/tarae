use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use tarae_testkit::scratch::{Scratch, write_script};

use super::*;

/// The stand-in `docker`: its `run` logs `run <scene>` and waits until the test lets that scene go (its `release-`
/// file), at most about `PATIENCE`.
const STAND_IN: &str = "#!/bin/sh\ncase \"$1\" in\n  run) echo \"run $XTASK_SCENE_NAME\" >> \"$FAKE_DIR/log\"; i=0; \
     while [ ! -e \"$FAKE_DIR/release-$XTASK_SCENE_NAME\" ] && [ $i -lt 1200 ]; do sleep 0.1; i=$((i + 1)); done ;;\n\
     esac\n";

/// A Linux check as `linux::run` takes its turn, in a process of its own: the lock for its endpoint, said on stderr,
/// then the stand-in `docker run` its parent put first on PATH.
#[test]
#[ignore = "run by one_linux_check_at_a_time_on_a_docker in a process of its own"]
fn turn_scene() {
    let Ok(dir) = std::env::var("XTASK_LOCK_DIR") else { return };
    let var = |n: &str| std::env::var(n).unwrap_or_default();
    let checkout = var("XTASK_SCENE_CHECKOUT");
    let held =
        take_in(Path::new(&dir), &var("XTASK_SCENE_ENDPOINT"), Path::new(&checkout), &mut std::io::stderr());
    assert!(held.is_some(), "the lock taken");
    let st = Command::new("docker").args(["run", "--rm", "rust:1"]).status();
    eprintln!("scene: docker run ended {st:?}");
}

struct Scene {
    dir: Scratch,
    path: String,
}

impl Scene {
    fn new() -> Scene {
        let dir = Scratch::new("xtask-dockerlock");
        write_script(&dir.join("docker"), STAND_IN);
        std::fs::create_dir(dir.join("locks")).unwrap();
        let path = format!("{}:{}", dir.display(), std::env::var("PATH").unwrap_or_default());
        Scene { dir, path }
    }

    /// A check named `name` for checkout `/stand-in/<checkout>` on `endpoint`'s Docker.
    fn start(&self, name: &str, checkout: &str, endpoint: &str) -> Child {
        let err = std::fs::File::create(self.dir.join(format!("{name}.err"))).unwrap();
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "dockerlock::tests::turn_scene", "--ignored", "--nocapture"])
            .env("PATH", &self.path)
            .env("FAKE_DIR", &*self.dir)
            .env("XTASK_LOCK_DIR", self.dir.join("locks"))
            .env("XTASK_SCENE_NAME", name)
            .env("XTASK_SCENE_CHECKOUT", format!("/stand-in/{checkout}"))
            .env("XTASK_SCENE_ENDPOINT", endpoint)
            .stdout(Stdio::null())
            .stderr(err)
            .spawn()
            .unwrap()
    }

    fn ran(&self, name: &str) -> bool {
        let log = std::fs::read_to_string(self.dir.join("log")).unwrap_or_default();
        log.lines().any(|l| l == format!("run {name}"))
    }

    fn said(&self, name: &str) -> String {
        std::fs::read_to_string(self.dir.join(format!("{name}.err"))).unwrap_or_default()
    }

    fn release(&self, name: &str) {
        std::fs::write(self.dir.join(format!("release-{name}")), "").unwrap();
    }

    fn until(&self, what: &str, done: impl Fn() -> bool) {
        let by = Instant::now() + crate::PATIENCE;
        while !done() {
            assert!(
                Instant::now() < by,
                "{what} never came — log: {:?}",
                std::fs::read_to_string(self.dir.join("log"))
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

fn ended(child: &mut Child) -> std::process::ExitStatus {
    let by = Instant::now() + crate::PATIENCE;
    loop {
        if let Some(st) = child.try_wait().unwrap() {
            return st;
        }
        if Instant::now() > by {
            let _ = child.kill();
            panic!("a scene didn't end in time");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// #352: two Linux checks on one Docker go one at a time — the second waits, says so with the first's checkout and
/// time (the line the rust pack's `[[waits]]` reads), and goes once the first ends; another Docker's check doesn't
/// wait on them; and a holder killed lets the lock go with it (no `docker run` it started keeps it: close-on-exec).
/// Each check is a process of its own against a stand-in `docker`.
#[test]
fn one_linux_check_at_a_time_on_a_docker() {
    const COLIMA: &str = "unix:///stand-in/colima/tarae-test/docker.sock";
    let s = Scene::new();
    let mut a = s.start("a", "fixes", COLIMA);
    s.until("a's docker run", || s.ran("a"));
    let mut b = s.start("b", "collab", COLIMA);
    s.until("b saying it waits", || s.said("b").contains(&format!("{WAITS} (fixes, ")));
    assert!(s.said("b").contains(&format!("one at a time on {COLIMA}")), "{}", s.said("b"));
    // (it tries again every POLL: a few of them, and it still waits)
    std::thread::sleep(POLL * 4);
    assert!(!s.ran("b"), "b runs while a holds the lock: {}", s.said("b"));
    // Another Docker's check goes at once
    let mut lima = s.start("lima", "ci", "unix:///stand-in/lima/kernel-5.14/docker.sock");
    s.until("the other Docker's run", || s.ran("lima"));
    assert!(!s.said("lima").contains(WAITS), "{}", s.said("lima"));
    s.release("lima");
    assert!(ended(&mut lima).success());
    // a ends: b's turn
    s.release("a");
    assert!(ended(&mut a).success(), "{}", s.said("a"));
    s.until("b's docker run", || s.ran("b"));
    assert!(s.said("b").contains("for another Linux check — its turn now"), "{}", s.said("b"));
    s.release("b");
    assert!(ended(&mut b).success(), "{}", s.said("b"));
    // The holder killed (its stand-in `docker run` living on): the next one goes
    let mut c = s.start("c", "panes4", COLIMA);
    s.until("c's docker run", || s.ran("c"));
    let mut d = s.start("d", "sandbox", COLIMA);
    s.until("d saying it waits", || s.said("d").contains(&format!("{WAITS} (panes4, ")));
    c.kill().unwrap();
    let _ = c.wait();
    s.until("d's docker run once c was killed", || s.ran("d"));
    s.release("c");
    s.release("d");
    assert!(ended(&mut d).success(), "{}", s.said("d"));
}

/// One lock a Docker: an endpoint's own file, the same for the same endpoint; `endpoint` takes `DOCKER_HOST` first,
/// else what the context says, a socket by its real path.
#[test]
fn each_docker_has_its_own_lock() {
    let dir = Path::new("/locks");
    let colima = path_in(dir, "unix:///a/colima/tarae-test/docker.sock");
    assert_eq!(colima, path_in(dir, "unix:///a/colima/tarae-test/docker.sock"));
    assert_ne!(colima, path_in(dir, "unix:///a/lima/k/docker.sock"));
    let name = colima.file_name().unwrap().to_string_lossy().into_owned();
    assert!(name.starts_with("tarae-docker-") && name.ends_with(".lock"), "{name}");
    let s = Scratch::new("xtask-dockerlock-endpoint");
    std::fs::write(s.join("docker.sock"), "").unwrap();
    std::os::unix::fs::symlink(s.join("docker.sock"), s.join("link.sock")).unwrap();
    let real = std::fs::canonicalize(s.join("docker.sock")).unwrap();
    let mut docker = Command::new("tarae-no-such-docker");
    docker.env("DOCKER_HOST", format!("unix://{}", s.join("link.sock").display()));
    assert_eq!(endpoint(&docker), format!("unix://{}", real.display()), "DOCKER_HOST, its real path");
    let fake = s.join("docker");
    write_script(
        &fake,
        "#!/bin/sh\n[ \"$1 $2\" = 'context inspect' ] && echo \"tcp://$DOCKER_CONTEXT:2375\"\n",
    );
    let mut docker = Command::new(&fake);
    docker.env_remove("DOCKER_HOST").env("DOCKER_CONTEXT", "lima-k");
    assert_eq!(endpoint(&docker), "tcp://lima-k:2375", "the context's");
    let mut docker = Command::new("tarae-no-such-docker");
    docker.env_remove("DOCKER_HOST");
    assert_eq!(endpoint(&docker), DEFAULT, "nothing names one");
}

/// A file at the lock's place that isn't this user's own plain file is never used: said, and no turn waited for.
#[test]
fn a_lock_file_not_ones_own_is_said_and_passed_over() {
    let s = Scratch::new("xtask-dockerlock-planted");
    let lock = path_in(&s, "unix:///x.sock");
    std::os::unix::fs::symlink(s.join("elsewhere"), &lock).unwrap();
    let mut said = Vec::new();
    assert!(take_in(&s, "unix:///x.sock", Path::new("/c"), &mut said).is_none());
    let said = String::from_utf8(said).unwrap();
    assert!(said.contains("wasn't taken") && said.contains("doesn't wait its turn"), "{said}");
    assert!(!s.join("elsewhere").exists(), "nothing made through the link");
    // Its own: taken, its holder written
    let mut said = Vec::new();
    let held = take_in(&s, "unix:///y.sock", Path::new("/stand-in/fixes"), &mut said).expect("taken");
    assert!(said.is_empty(), "no wait, nothing said");
    let h = Holder::read(&held.0).expect("its holder");
    assert_eq!((h.pid, h.checkout.as_path()), (std::process::id(), Path::new("/stand-in/fixes")));
}
