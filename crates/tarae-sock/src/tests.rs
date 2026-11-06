use super::*;
use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use tarae_testkit::scratch::{Scratch, long_socket};

/// #383: a socket at a 110-byte path is bound and connected from its folder — the path as it is never reaches it.
#[test]
fn a_socket_past_sun_path_is_bound_and_reached() {
    let dir = Scratch::new("sock-long");
    let path = long_socket(dir.path(), "7.sock");
    let listener = at(&path, |p: &Path| UnixListener::bind(p)).expect("bound from its folder");
    assert!(path.exists(), "{} is there", path.display());
    assert!(UnixStream::connect(&path).is_err(), "as it is it's past sun_path (else this proves nothing)");
    let mut c = connect(&path).expect("reached from its folder");
    let (mut s, _) = listener.accept().unwrap();
    c.write_all(b"hi").unwrap();
    let mut got = [0u8; 2];
    s.read_exact(&mut got).unwrap();
    assert_eq!(&got, b"hi");
}

/// Its name alone past `sun_path` can't be reached any way: said, never tried.
#[test]
fn a_name_alone_past_sun_path_is_said() {
    let e = connect(&Path::new("/x").join("n".repeat(SUN_PATH))).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
    assert!(e.to_string().contains("its name alone"), "{e}");
}

/// The folder moves for the connecting thread alone: another thread looking all the while sees the process's.
#[test]
fn no_other_thread_sees_the_folder_move() {
    // (read first: the bind below is one of the calls that mustn't move it)
    let before = std::env::current_dir().unwrap();
    let dir = Scratch::new("sock-apart");
    let path = long_socket(dir.path(), "8.sock");
    let _listener = at(&path, |p: &Path| UnixListener::bind(p)).unwrap();
    assert_eq!(std::env::current_dir().unwrap(), before, "a bind leaves the caller's folder");
    let done = std::sync::atomic::AtomicBool::new(false);
    let moved = std::thread::scope(|s| {
        let looker = std::thread::Builder::new()
            .name("looker".into())
            .spawn_scoped(s, || {
                let mut moved = 0;
                while !done.load(std::sync::atomic::Ordering::SeqCst) {
                    moved += usize::from(std::env::current_dir().ok().as_ref() != Some(&before));
                }
                moved
            })
            .unwrap();
        let real = std::fs::canonicalize(&path).unwrap();
        for _ in 0..200 {
            let named = at(&path, |p: &Path| std::fs::canonicalize(p)).unwrap();
            assert_eq!(named, real, "the name given reaches the socket");
        }
        done.store(true, std::sync::atomic::Ordering::SeqCst);
        looker.join().unwrap()
    });
    assert_eq!(moved, 0, "another thread saw the folder move {moved} times");
    assert_eq!(std::env::current_dir().unwrap(), before, "the caller's stays");
}
