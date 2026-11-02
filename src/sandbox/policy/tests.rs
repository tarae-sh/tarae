use super::*;
use crate::sandbox::tests::env;

/// A line saying a mount, an unmount or a pivot was refused (`mount_line`): the tools' own words, the calls' names;
/// never a path holding the word, a mount that failed for another reason, nor a refusal that names no mount.
#[test]
fn a_refused_mount_is_read_from_its_line() {
    for line in [
        "mount: /mnt: permission denied.",
        "umount: /x: Operation not permitted",
        "bwrap: Can't mount proc on /newroot/proc: Operation not permitted",
        "bwrap: Failed to make / slave: Operation not permitted",
        "unshare: cannot change root filesystem propagation: Permission denied (mount)",
        "pivot_root: failed to change root from `.' to `old': Operation not permitted",
        "Error: mount_setattr: Operation not permitted",
    ] {
        assert!(mount_line(line), "{line}");
    }
    for line in [
        "cat: /proc/mounts: Permission denied",
        "ls: cannot open directory /mount: Permission denied",
        "cat: /proc/self/mountinfo: Operation not permitted",
        "mount: /mnt: special device /dev/x does not exist.",
        "touch: Makefile: Operation not permitted",
        "the mounts here: Permission denied",
        // (a name the tool was given, quoted: a write refused, not a mount)
        "touch: cannot touch 'mount': Permission denied",
        "mkdir: cannot create directory ‘umount’: Permission denied",
        // (a daemon's own mount, outside the run)
        "docker: Error response from daemon: error while mounting volume '/var/lib/docker/volumes/v/_data': \
         failed to mount local volume: mount /src:/var/lib/docker/volumes/v/_data, flags: 0x1000: permission denied.",
    ] {
        assert!(!mount_line(line), "{line}");
    }
}

/// frictions29 30: macOS's tools say a refusal as `<program>: <name>: <refusal>` — `touch: Makefile: Operation not
/// permitted`, all a run's output says when the kernel's own report is lost: read as the protected name refused, from
/// where the run ran, not "the report names no cause". One the rules let it write, or a name with no program before
/// it, names nothing.
#[test]
fn a_tools_own_name_then_the_file_is_read_as_its_refusal() {
    let p = Policy::for_project(&env(Path::new("/Users/u/p")));
    let said = |p: &Policy, line: &str| -> Vec<String> {
        p.said_in_output(&[(false, line.to_string())], &[]).into_iter().map(|d| d.target).collect()
    };
    assert_eq!(said(&p, "touch: Makefile: Operation not permitted"), ["/Users/u/p/Makefile"]);
    // (each system's words: Landlock's "Permission denied" is a refusal only on Linux)
    let why = if cfg!(target_os = "linux") { "Permission denied" } else { "Operation not permitted" };
    assert_eq!(said(&p, &format!("mkdir: .github: {why}")), ["/Users/u/p/.github"]);
    let sub = Policy { cwd: Some(PathBuf::from("/Users/u/p/sub")), ..p.clone() };
    assert_eq!(said(&sub, "touch: Makefile: Operation not permitted"), ["/Users/u/p/sub/Makefile"]);
    assert_eq!(said(&sub, "touch: ../build.rs: Operation not permitted"), ["/Users/u/p/build.rs"]);
    for line in [
        "touch: src/a.rs: Operation not permitted",
        "Makefile: Operation not permitted",
        "touch: two words: Operation not permitted",
    ] {
        assert!(said(&p, line).is_empty(), "{line}");
    }
}

#[test]
fn writes_allow_checks_dev_shm_against_kept_and_caches() {
    let env = Env { root: PathBuf::from("/tmp/proj"), ..Default::default() };
    let mut p = Policy::for_project(&env);

    // By default, own_shm is false on Policy::for_project
    assert!(!p.own_shm, "policy.own_shm is false by default");
    assert!(!p.writes_allow(Path::new("/dev/shm/foo")), "dev/shm refused when own_shm is false");

    // (a) own_shm on/off
    p.own_shm = true;
    assert!(p.writes_allow(Path::new("/dev/shm/foo")), "dev/shm allowed when own_shm is true");

    p.own_shm = false;
    assert!(!p.writes_allow(Path::new("/dev/shm/foo")), "dev/shm refused when own_shm is false again");

    // (b) kept or cache path under /dev/shm is excluded even with own_shm on
    p.own_shm = true;
    p.keep = vec![PathBuf::from("/dev/shm/kept")];
    p.caches = vec![PathBuf::from("/dev/shm/cache")];

    assert!(!p.writes_allow(Path::new("/dev/shm/kept/bar")), "kept path under /dev/shm is excluded");
    assert!(!p.writes_allow(Path::new("/dev/shm/cache/baz")), "cache path under /dev/shm is excluded");
    assert!(p.writes_allow(Path::new("/dev/shm/other")), "other path under /dev/shm remains allowed");

    // (c) a project in /dev/shm keeps its protected names with own_shm on: the project's rule comes first
    // (which run has own_shm on — `Keeper::start_served`'s — is `keeper::tests::dev_shm_is_a_runs_to_change_…`)
    let shm = PathBuf::from("/dev/shm/proj");
    let p = Policy { own_shm: true, ..Policy::for_project(&Env { root: shm.clone(), ..Default::default() }) };
    assert!(!p.writes_allow(&shm.join("Cargo.toml")), "a protected name in a /dev/shm project");
    assert!(p.writes_allow(&shm.join("src/main.rs")), "the rest of it");
}
