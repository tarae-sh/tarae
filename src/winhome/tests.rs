use super::*;
use crate::cards::Class;

/// WSL's mount table as WSL 2 writes it: a drive is 9p with `aname=drvfs` (its source and `path=` escaped,
/// `C:\134`), or `drvfs` itself, or a virtiofs whose source is the drive; nothing else is one.
#[test]
fn the_drives_are_read_from_the_mount_table() {
    let mounts = "\
none /mnt/wsl tmpfs rw,relatime 0 0
/dev/sdc / ext4 rw,relatime,discard,errors=remount-ro,data=ordered 0 0
C:\\134 /mnt/c 9p rw,noatime,dirsync,aname=drvfs;path=C:\\134;uid=1000;gid=1000;symlinkroot=/mnt/,mmap,access=client,msize=65536,trans=fd,rfd=5,wfd=5 0 0
D:\\134 /mnt/d drvfs rw,noatime,uid=1000,gid=1000 0 0
E:\\134 /win/e\\040drive virtiofs rw,relatime 0 0
drvfs /mnt/g 9p rw,noatime,aname=drvfs;path=G:\\134;uid=1000 0 0
someserver:/x /mnt/nfs nfs4 rw 0 0
drvfs /mnt/wslg/doc 9p rw,aname=other 0 0
";
    let d = drives_in(mounts);
    let want = [('c', "/mnt/c"), ('d', "/mnt/d"), ('e', "/win/e drive"), ('g', "/mnt/g")];
    let want: Vec<Drive> = want.iter().map(|(l, at)| Drive { letter: *l, at: PathBuf::from(at) }).collect();
    assert_eq!(d, want);
    assert!(drives_in("").is_empty() && drives_in("/dev/disk1s1 / apfs rw 0 0").is_empty());
}

fn on_c(at: &Path) -> Homes {
    Homes { drives: vec![Drive { letter: 'c', at: at.to_path_buf() }], ..Homes::default() }
}

/// A Windows profile's keys are secret by its path alone, in any case — a Linux home's places and Windows' own —
/// any user's profile, found or not; not what's beside them, not `Public`'s, not a path off the drives.
#[test]
fn a_windows_profiles_keys_are_secret_in_any_case() {
    let h = on_c(Path::new("/mnt/c"));
    for p in [
        "/mnt/c/Users/x/.ssh/id_ed25519",
        "/mnt/c/Users/x/.SSH/config",
        "/mnt/c/users/X/.aws/credentials",
        "/mnt/c/Users/x/.kube/config",
        "/mnt/c/Users/x/.azure",
        "/mnt/c/Users/x/.config/gcloud/credentials.db",
        "/mnt/c/Users/x/AppData/Roaming/Microsoft/Credentials/ABC",
        "/mnt/c/Users/x/appdata/local/google/chrome/User Data/Default/Cookies",
        "/mnt/c/Users/x/AppData/Roaming/Mozilla/Firefox/Profiles/a.default/logins.json",
        "/mnt/c/Users/other/.ssh",
    ] {
        assert!(h.secret(Path::new(p)), "{p}");
    }
    for p in [
        "/mnt/c/Users/x/Documents/a.txt",
        "/mnt/c/Users/x/AppData/Local/Temp/x",
        "/mnt/c/Users/x",
        "/mnt/c/Users/Public/.ssh/id_rsa",
        "/mnt/c/Windows/System32/config",
        "/home/u/x/Users/x/.ssh",
    ] {
        assert!(!h.secret(Path::new(p)), "{p}");
    }
    assert!(!Homes::default().secret(Path::new("/mnt/c/Users/x/.ssh")), "no drives, no Windows home");
    for p in ["/", "/mnt", "/mnt/c", "/mnt/c/Users", "/mnt/c/Users/x", "/mnt/c/users"] {
        assert!(h.holds_a_home(Path::new(p)), "{p}");
    }
    for p in ["/mnt/c/Users/x/Documents", "/mnt/c/Windows", "/mnt/c/Users/Public", "/mnt/d"] {
        assert!(!h.holds_a_home(Path::new(p)), "{p}");
    }
}

/// A stand-in drive: profiles `Wq` (`AppData` with Chrome's in another case) and `other`, `Public` and `Default`
/// (no one's), a link (`All Users`) — and a file at `Users`' top.
fn drive(s: &Path) -> PathBuf {
    let c = s.join("mnt/c");
    for d in ["Users/Wq/.ssh", "Users/Wq/AppData/Local/google/chrome/User Data", "Users/Wq/AppData/Roaming"] {
        std::fs::create_dir_all(c.join(d)).unwrap();
    }
    for d in ["Users/other/AppData", "Users/Public", "Users/Default", "Windows"] {
        std::fs::create_dir_all(c.join(d)).unwrap();
    }
    std::os::unix::fs::symlink(c.join("Users/Default"), c.join("Users/All Users")).unwrap();
    std::fs::write(c.join("Users/desktop.ini"), "").unwrap();
    c
}

/// The profiles are the drive's `Users` folders but no one's and links; yours by `USERPROFILE` (a Windows path, or
/// WSLENV's Linux one, in any case), else by the Linux user's name, else the one whose `AppData` lists; what's kept
/// spelled as it is on the drive, Windows' own only where it is.
#[test]
fn the_profiles_and_yours_are_found_on_the_drives() {
    let s = crate::scratch::Scratch::new("winhome-find");
    let c = drive(&s);
    let d = || vec![Drive { letter: 'c', at: c.clone() }];
    let (wq, other) = (c.join("Users/Wq"), c.join("Users/other"));
    let h = find(d(), None, Some("nobody"));
    assert_eq!(h.found, vec![wq.clone(), other.clone()]);
    assert!(h.kept.contains(&wq.join(".ssh")) && h.kept.contains(&wq.join(".kube")), "{:?}", h.kept);
    assert!(
        h.kept.contains(&wq.join("AppData/Local/google/chrome/User Data")),
        "as spelled there: {:?}",
        h.kept
    );
    assert!(!h.kept.iter().any(|k| k.ends_with("Firefox/Profiles")), "Windows' own only where it is");
    assert!(h.kept.contains(&other.join(".aws")), "another's too");
    for (profile, user, want) in [
        (Some("C:\\Users\\wq"), None, Some(&wq)),
        (Some("c:\\Users\\other\\"), Some("wq"), Some(&other)),
        (Some(wq.to_str().unwrap()), None, Some(&wq)),
        (Some("C:\\Users\\Public"), Some("other"), Some(&other)),
        (Some("D:\\Users\\wq"), Some("WQ"), Some(&wq)),
        (None, Some("Other"), Some(&other)),
    ] {
        assert_eq!(find(d(), profile, user).yours.as_ref(), want, "{profile:?} {user:?}");
    }
    // (both `AppData`s list: not told apart — then only `other`'s does)
    assert_eq!(find(d(), None, None).yours, None);
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(other.join("AppData"), std::fs::Permissions::from_mode(0o000)).unwrap();
    let closed = std::fs::read_dir(other.join("AppData")).is_err();
    let yours = find(d(), None, None).yours;
    std::fs::set_permissions(other.join("AppData"), std::fs::Permissions::from_mode(0o700)).unwrap();
    if closed {
        assert_eq!(yours, Some(wq.clone()), "the one profile this user lists");
    }
    assert_eq!(find(Vec::new(), Some("C:\\Users\\wq"), Some("wq")), Homes::default(), "no drives: none");
}

/// Claude's reads: your Windows profile is home, another's another's home, their keys secret — never a volume's,
/// none read unasked; no answer about a profile, a folder holding one or its keys is kept past once.
#[test]
fn claudes_reads_take_a_windows_home_as_a_home() {
    let home = Path::new("/home/u");
    let mut windows = on_c(Path::new("/mnt/c"));
    windows.yours = Some(PathBuf::from("/mnt/c/Users/x"));
    let _in = stand_in(windows.clone());
    // (what runs never read there, as `here` finds it: a folder holding one is still a home's)
    let kept = ["/mnt/c/Users/x/.ssh", "/mnt/c/Users/x/AppData/Roaming/Microsoft/Credentials"];
    let d = crate::allow::Defaults { kept: kept.map(PathBuf::from).to_vec(), ..Default::default() };
    let caps =
        crate::caps::Caps { windows: windows.clone(), ..crate::caps::Caps::of_defaults(d, Some(home)) };
    let at = |p: &str| crate::cards::class(Some(Path::new(p)), &caps);
    for (p, c) in [
        ("/mnt/c/Users/x/Documents/a.txt", Class::Home),
        ("/mnt/c/Users/x", Class::Home),
        ("/mnt/c/Users/x/AppData/Roaming", Class::Home),
        ("/mnt/c", Class::Home),
        ("/mnt/c/Users/x/.kube/config", Class::Secret),
        ("/mnt/c/Users/X/.SSH", Class::Secret),
        ("/mnt/c/Users/x/AppData/Local/Microsoft/Edge/User Data/Default/Login Data", Class::Secret),
        ("/mnt/c/Users/other/Documents", Class::OtherHome),
        ("/mnt/c/Users/other/.aws/credentials", Class::Secret),
        ("/mnt/c/Windows/System32", Class::Volume),
        ("/mnt/c/Users/Public/x", Class::Volume),
        ("/opt/x", Class::Elsewhere),
    ] {
        assert_eq!(at(p), c, "{p}");
        assert_eq!(crate::reads::by_default(at(p), &caps), c == Class::Elsewhere, "{p}");
    }
    // (yours not told apart: every profile another's)
    let caps = crate::caps::Caps { windows: on_c(Path::new("/mnt/c")), ..caps };
    assert_eq!(crate::cards::class(Some(Path::new("/mnt/c/Users/x/a")), &caps), Class::OtherHome);
    assert_eq!(crate::cards::class(Some(Path::new("/mnt/c")), &caps), Class::OtherHome);
    for p in
        ["/mnt", "/mnt/c", "/mnt/c/Users", "/mnt/c/Users/x", "/mnt/c/Users/x/.kube", "/mnt/c/Users/o/.ssh/k"]
    {
        assert!(!crate::allow::keepable(Path::new(p), Some(home)), "{p}");
        assert!(!crate::allow::keepable(Path::new(p), None), "{p}, no home");
    }
    assert!(crate::allow::keepable(Path::new("/mnt/c/Users/x/Documents/proj"), Some(home)));
    assert!(crate::allow::keepable(Path::new("/mnt/c/Windows"), Some(home)));
}

/// What every run never reads holds each profile's keys — as the drive spells them — beside home's.
#[test]
fn every_run_keeps_a_windows_homes_keys() {
    let s = crate::scratch::Scratch::new("winhome-kept");
    let c = drive(&s);
    let windows = find(vec![Drive { letter: 'c', at: c.clone() }], None, Some("wq"));
    let home = s.join("home");
    let here = crate::sandbox::Here { windows, ..crate::sandbox::Here::default() };
    let kept = crate::sandbox::kept_places(&home, &here);
    let real = |p: &str| crate::sandbox::profile_path(&c.join(p));
    for p in [
        "Users/Wq/.ssh",
        "Users/Wq/.aws",
        "Users/Wq/AppData/Local/google/chrome/User Data",
        "Users/other/.kube",
    ] {
        assert!(kept.contains(&real(p)), "{p}: {kept:?}");
    }
    assert!(!kept.iter().any(|k| k.starts_with(real("Users/Public"))), "{kept:?}");
    let none = crate::sandbox::kept_places(&home, &crate::sandbox::Here::default());
    assert!(!none.iter().any(|k| k.starts_with(&c)), "no Windows homes, none of theirs");
}
