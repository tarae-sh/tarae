use super::*;
use crate::sandbox::tests::env;

/// A line saying a mount, an unmount or a pivot was refused (`mount_line`): the tools' own words, as their sources
/// write them, the calls' names; never a path holding the word, a mount that failed for another reason, nor a
/// refusal that names no mount — nor a refused write to a file named like a call (#284), in each tool's words.
#[test]
fn a_refused_mount_is_read_from_its_line() {
    for line in [
        // (util-linux's mount and umount: the target, then libmount's message)
        "mount: /mnt: permission denied.",
        "mount: /mnt: fsmount() failed: Operation not permitted.",
        "umount: /x: umount failed: Operation not permitted.",
        "/usr/bin/mount: /mnt: permission denied.",
        // (Go's `os.NewSyscallError("mount", err)`)
        "mount: operation not permitted",
        // (bwrap's, as bubblewrap.c dies with them)
        "bwrap: Can't mount proc on /newroot/proc: Operation not permitted",
        "bwrap: Failed to make / slave: Operation not permitted",
        "bwrap: pivot_root: Operation not permitted",
        "bwrap: pivot_root(/newroot): Operation not permitted",
        "bwrap: unmount old root: Operation not permitted",
        "bwrap: Can't bind mount /oldroot/usr on /newroot/usr: Unable to remount destination \"/newroot/usr\" with \
         correct flags: Operation not permitted",
        "bwrap: Can't remount readonly on /newroot/etc: Operation not permitted",
        // (libfuse's fusermount, as fusermount.c says it)
        "fusermount3: mount failed: Operation not permitted",
        // (unshare's, as unshare.c errs)
        "unshare: cannot change root filesystem propagation: Permission denied",
        "unshare: mount /proc failed: Permission denied",
        // (moby/sys's `mountError`, podman's through containers/storage; its overlay driver's own)
        "Error: mount /var/lib/containers/storage/overlay:/var/lib/containers/storage/overlay, flags: 0x1000: \
         operation not permitted",
        "Error: unmount /run/containers/storage: operation not permitted",
        // (runc's `mountError`, as mount_linux.go writes it; crun's, as linux.c makes them — alone and in podman's line)
        "runc run failed: unable to start container process: error during container init: error mounting \"proc\" to \
         rootfs at \"/proc\": mount src=\"proc\", dst=\"/proc\", flags=MS_NOSUID|MS_NODEV|MS_NOEXEC: operation not \
         permitted",
        "crun: mount `proc` to `/proc`: Operation not permitted",
        "runc create failed: unable to start container process: error during container init: error mounting \
         \"mqueue\" to rootfs at \"/dev/mqueue\": operation not permitted",
        "Error: crun: move mount `/sys` to `/newroot/sys`: Operation not permitted: OCI permission denied",
        "Error: creating overlay mount to /var/lib/containers/storage/overlay/1f/merged, mount_data=\"lowerdir=/l\": \
         operation not permitted",
        // (a call's name, then `(` or `:` — strace's, a program's own)
        "mount(\"proc\", \"/proc\", \"proc\", 0, NULL) = -1 EPERM (Operation not permitted)",
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
        // (#284: a file named like a call is an operand, never the program — Go's `os.PathError` and `os.LinkError`,
        // busybox's, dash's and bash's as `sh`, Python's `PermissionError`)
        "open mount: permission denied",
        "mkdir mount: permission denied",
        "stat pivot_root: permission denied",
        "unlink umount: operation not permitted",
        "rename mount.tmp mount: permission denied",
        "rename mount /tmp/x: permission denied",
        "link mount /tmp/x: permission denied",
        "symlink mount /tmp/x: permission denied",
        "symlink unmount /x/y: operation not permitted",
        // (what no shape decides is a refusal at its paths, as before userns A: moby's `mountError` with no flags, its
        // write grant opening writes there, never the mount)
        "Error: mount /a:/b, data: x: operation not permitted",
        "touch: mount: Permission denied",
        "sh: 1: cannot create mount: Permission denied",
        "sh: line 1: mount: Permission denied",
        "bash: mount: Permission denied",
        // (whatever the program or the script: GNU's tee and cat leave a plain name bare, util-linux's script warns
        // `cannot open %s`)
        "tee: mount: Permission denied",
        "cat: mount: Permission denied",
        "script: cannot open mount: Permission denied",
        "./build.sh: mount: Operation not permitted",
        "build.sh: line 3: mount: Permission denied",
        "./run.sh: 3: mount: Permission denied",
        "PermissionError: [Errno 13] Permission denied: 'mount'",
        // (a word of the family where no tool's message puts it: in a sentence, in a path a tool names)
        "cannot write the mount table: Permission denied",
        "cat: /mount: Permission denied",
        // (a path in a line of a program that mounts: bwrap's `die_with_error ("execvp %s", …)`)
        "bwrap: execvp ./mount: Permission denied",
        "unshare: cannot open /x/mount: Permission denied",
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
    // (a shell's line number between: bash's, a script's as dash says it)
    assert_eq!(said(&p, "sh: line 1: Makefile: Operation not permitted"), ["/Users/u/p/Makefile"]);
    assert_eq!(said(&p, "./build.sh: 3: Makefile: Operation not permitted"), ["/Users/u/p/Makefile"]);
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
