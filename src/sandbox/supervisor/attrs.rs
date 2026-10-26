//! A file's mode and times, for a run (linmeta, `.sb/linmeta-design.md`): Landlock has no right for them,
//! so the served filter hands tarae every call that changes them (`seccomp::serves_attr`), and they're let only where
//! the run may write (`Policy::writes_allow`: its write places, never a protected name) — as macOS's profile has
//! `file-write-mode` and `file-write-times`. An owner call is let only where it changes nothing — each id `-1` or
//! the file's own (`rsync -a`, `cp -a`) — and there as a mode is; made by tarae with `-1`, `-1`, so the
//! kernel's own side effects (its change time, a set-ID bit cleared) are the run's call's. Its extended attributes
//! (D2 B, ACLs too — `install -m`, `cp --preserve=all`, `rsync -AX`) as its mode: `user.` and the two ACLs
//! (`system.posix_acl_access`, `_default`) only where it may write; `security.` and `trusted.` (a program's
//! capabilities, a security module's labels) and every other `system.` refused by name in every place, as root too;
//! a name in no namespace the kernel has is its own `EOPNOTSUPP`, after its write check. The name and the value are
//! read once from the caller's memory, and set by tarae through its own descriptor; an ACL's user and group ids are
//! the run's, read in its user namespace (`acl_ids`: one it doesn't map the kernel's `EINVAL` there).
//!
//! The safe form, as the socket calls': a path is read once from the caller's memory (`wire::c_path`) and opened by
//! the kernel, never walked by tarae — an absolute one from the run's root (`/proc/<tid>/root`, `RESOLVE_IN_ROOT`),
//! a relative one from the run's own folder (its cwd, or the dirfd: tarae's copy, `pidfd_getfd`) — into an `O_PATH`
//! descriptor of tarae's, decided on what that is, and the call made on it (through `/proc/self/fd/M` for a mode:
//! Linux 5.14 has no `fchmodat2`): a link swapped in after the check reaches nothing. What the run sees at the
//! file's name (from its own root) must be that file: a key's cover in tier own is another one, a B9 seal read-only
//! (`EROFS`, as the kernel answers it there), a name gone (` (deleted)`) none. Only what builds name is served: a
//! path through `/proc` or a descriptor's name in `/dev` (`/dev/fd/N/…`, `/dev/stdout`), or a magic link on the way,
//! is refused, said, never followed for the run — but the whole path `/proc/self/fd/N`, which is its descriptor N.
//! A descriptor form is decided on what tarae's copy names, and the run's call is made again on the copy: one that
//! names no file — a pipe, a socket, a file no folder holds any more — is the run's to change, and one open for
//! writing is let wherever its name still leads the run to it (Landlock let the run write that file). tarae acts
//! with its own credentials, the run's user's.

use std::ffi::{CString, OsStr};
use std::io;
use std::mem::MaybeUninit;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use super::super::wire::{
    Mem, Times, c_path, no_name, self_fd, through_proc, times, xattr_args, xattr_name, xattr_value,
};
use super::{Answer, Caller, Listener, Served, errno, refuse, refuse_with, result};
use crate::seccomp::{SYS_REMOVEXATTRAT, SYS_SETXATTRAT};

/// What a call changes.
#[derive(Clone, Debug, PartialEq)]
enum Change {
    Mode(libc::mode_t),
    /// Access and modification times as `utimensat` takes them; None: both now.
    Times(Option<[i64; 4]>),
    /// Owner and group as the call gives them (`u32::MAX`: as it is) — let only where neither changes (`refused`).
    Owner(u32, u32),
    /// An extended attribute: its name, and its value with `XATTR_CREATE`/`XATTR_REPLACE` — None: removed.
    Xattr(CString, Option<(Vec<u8>, i32)>),
}

impl Change {
    /// How a refusal says it (Seatbelt's words, which the report says as "changing …").
    fn op(&self) -> &'static str {
        match self {
            Change::Mode(_) => "file-write-mode",
            Change::Times(_) => "file-write-times",
            Change::Owner(..) => "file-write-owner",
            Change::Xattr(..) => "file-write-xattr",
        }
    }
}

/// How the run's call on a descriptor of its own is made again on tarae's copy of it.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Again {
    /// `fchmod(fd, mode)`.
    Fchmod,
    /// `fchmodat2(fd, "", mode, flags)` (`AT_EMPTY_PATH`).
    Fchmodat2(i32),
    /// `utimensat(fd, NULL or "", times, flags)` — `futimens` (and `futimesat(fd, NULL, …)`), or `AT_EMPTY_PATH`.
    Utimensat { empty: bool, flags: i32 },
    /// The path `/proc/self/fd/N`: the same path through tarae's own (`/proc/self/fd/M`).
    Proc { follow: bool },
    /// `fchown(fd, …)`, `fchownat(fd, "", …, AT_EMPTY_PATH)`: made as every owner call is (`chown_unchanged`).
    Owner,
    /// `fsetxattr(fd, …)`, `fremovexattr(fd, …)`, `setxattrat(fd, "", AT_EMPTY_PATH, …)` and `removexattrat`'s:
    /// the same on tarae's copy (one `O_PATH`: the kernel's own `EBADF`).
    Xattr,
}

/// A call's change as `call` has it: read, or an extended attribute's arguments still in the caller's memory — where
/// this kernel reads them after it finds the file (`Order`).
enum Asked {
    Read(Change),
    Unread(XattrArgs, Reads),
}

impl Asked {
    fn op(&self) -> &'static str {
        match self {
            Asked::Read(change) => change.op(),
            Asked::Unread(..) => "file-write-xattr",
        }
    }

    /// The change, read once the call's file is found and before anything tarae refuses for its own reasons (a bad
    /// argument is the kernel's own answer, unsaid): where the kernel's write check comes before the arguments
    /// (`Reads::AfterWriteCheck`), a read-only mount (`read_only`, asked only there) is its `EROFS` whatever they are
    /// — with good ones the call goes on to `refused`, which says a seal's.
    fn read(self, m: &impl Mem, read_only: impl FnOnce() -> bool) -> Result<Change, i32> {
        match self {
            Asked::Read(change) => Ok(change),
            Asked::Unread(args, reads) => {
                let change = args.read(m);
                if change.is_err() && reads == Reads::AfterWriteCheck && read_only() {
                    return Err(libc::EROFS);
                }
                change
            }
        }
    }
}

/// An extended attribute call's arguments as the run gave them (addresses in its memory, `setxattr`'s flags).
#[derive(Clone, Copy)]
enum XattrArgs {
    Set { name: u64, value: u64, size: u64, flags: i32 },
    Remove { name: u64 },
}

impl XattrArgs {
    fn read(self, m: &impl Mem) -> Result<Change, i32> {
        match self {
            XattrArgs::Set { name, value, size, flags } => set(m, name, value, size, flags),
            XattrArgs::Remove { name } => Ok(Change::Xattr(xattr_name(m, name)?, None)),
        }
    }

    /// `asked_in` this kernel's order (`orders`).
    fn asked(self, m: &impl Mem, by_fd: bool) -> Result<Asked, i32> {
        let order = if matches!(self, XattrArgs::Set { .. }) { orders().set } else { orders().remove };
        self.asked_in(m, by_fd, order)
    }

    /// The arguments read now where a kernel of `order` reads them first for this form (by a descriptor or a path),
    /// else left for after the file is found.
    fn asked_in(self, m: &impl Mem, by_fd: bool, order: Order) -> Result<Asked, i32> {
        match order.reads(by_fd) {
            Reads::First => Ok(Asked::Read(self.read(m)?)),
            reads => Ok(Asked::Unread(self, reads)),
        }
    }
}

/// What comes first in an extended attribute call, its arguments (flags, name, value) or finding its file — as each
/// kernel's `fs/xattr.c` has it (read in v5.14, v6.8–v6.13 and master; `setxattr` and `removexattr` alike).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Order {
    /// To 6.10: the file first — a path's lookup, or the descriptor (`EBADF`) — and its mount's write check
    /// (`EROFS`), then the arguments (`path_setxattr`, `fsetxattr` calling `setxattr` after `mnt_want_write`).
    File,
    /// 6.11 and 6.12: a path's arguments first, then its lookup and write check (`setxattr_copy` moved up); a
    /// descriptor's call looks at the descriptor first (`EBADF`), then the arguments, then the write check.
    Fd,
    /// From 6.13 (every form through `path_setxattrat`, `path_removexattrat`): the arguments first.
    Args,
}

/// Where a call's arguments are read (`Order::reads`).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Reads {
    First,
    /// Once its file is found (a lookup's or a descriptor's error first).
    AfterFile,
    /// Once its file is found and its mount isn't read-only (`EROFS` first).
    AfterWriteCheck,
}

impl Order {
    fn reads(self, by_fd: bool) -> Reads {
        match (self, by_fd) {
            (Order::File, _) => Reads::AfterWriteCheck,
            (Order::Fd, true) => Reads::AfterFile,
            (Order::Fd, false) | (Order::Args, _) => Reads::First,
        }
    }

    /// The order two calls that find no file say, by their answers: one by a path (`""`, `ENOENT` where the lookup
    /// comes first) and one by a descriptor (`-1`, `EBADF` where it comes first), each with arguments the kernel
    /// refuses (`bad`: an unknown flag's `EINVAL`, an empty name's `ERANGE`). None: an answer no kernel gives — a
    /// filter's around tarae.
    fn of(path: i32, fd: i32, bad: i32) -> Option<Order> {
        match (path, fd) {
            (libc::ENOENT, libc::EBADF) => Some(Order::File),
            (p, libc::EBADF) if p == bad => Some(Order::Fd),
            (p, f) if p == bad && f == bad => Some(Order::Args),
            _ => None,
        }
    }
}

/// This kernel's orders for setting and removing an extended attribute.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Orders {
    set: Order,
    remove: Order,
}

/// This kernel's `Orders`, read from it once as Landlock's ABI is (a distribution's kernel carries backports its
/// version doesn't say — Ubuntu's 6.8.0-117 has 6.11's `Order::Fd`, measured): calls that change nothing, by a path that names nothing and a descriptor that's none, with
/// arguments refused — whichever it looks at first answers. An answer no kernel gives: the newest order, logged.
fn orders() -> Orders {
    static ORDERS: std::sync::OnceLock<Orders> = std::sync::OnceLock::new();
    *ORDERS.get_or_init(|| {
        read_orders().unwrap_or_else(|why| {
            crate::log::line(
                "sandbox",
                "warn",
                &format!("extended attribute calls' order unread ({why}): the newest"),
            );
            Orders { set: Order::Args, remove: Order::Args }
        })
    })
}

/// `orders`' calls, made: their orders, or their answers when one says none.
fn read_orders() -> Result<Orders, String> {
    let (name, empty) = (c"user.tarae.order".as_ptr(), c"".as_ptr());
    let said = |r: libc::c_int| if r < 0 { errno_now() } else { 0 };
    // SAFETY: calls on no file — an empty path, descriptor -1 — with a name and a value that live through them.
    let [set, fset, rm, frm] = unsafe {
        [
            said(libc::setxattr(empty, name, c"x".as_ptr().cast(), 1, 4)),
            said(libc::fsetxattr(-1, name, c"x".as_ptr().cast(), 1, 4)),
            said(libc::removexattr(empty, empty)),
            said(libc::fremovexattr(-1, empty)),
        ]
    };
    match (Order::of(set, fset, libc::EINVAL), Order::of(rm, frm, libc::ERANGE)) {
        (Some(set), Some(remove)) => Ok(Orders { set, remove }),
        _ => Err(format!("set {set} {fset}, remove {rm} {frm}")),
    }
}

/// Where a call's file is: a path from a folder of the caller's (`AT_FDCWD`: where it works), or a descriptor.
enum At {
    Path { dirfd: u64, path: Vec<u8>, follow: bool },
    Fd(u64, Again),
}

const NOFOLLOW: i32 = libc::AT_SYMLINK_NOFOLLOW;
const EMPTY: i32 = libc::AT_EMPTY_PATH;

/// A descriptor argument as the kernel reads it: an `int`, its low half.
fn low(v: u64) -> u64 {
    u64::from(v as u32)
}

/// Whether a folder argument is `AT_FDCWD`.
fn cwd(dirfd: u64) -> bool {
    dirfd as i32 == libc::AT_FDCWD
}

/// A call that changes a file's mode, times, owner or extended attributes (`seccomp::serves_attr`), answered.
pub(super) fn call(l: &Listener, n: &libc::seccomp_notif, p: &Served) -> Answer {
    let c = Caller::open(l, n)?;
    let a = n.data.args;
    let path = |at: u64| c_path(&c, at);
    let (at, change) = match n.data.nr as libc::c_long {
        libc::SYS_fchmod => (At::Fd(low(a[0]), Again::Fchmod), Change::Mode(a[1] as libc::mode_t)),
        libc::SYS_fchmodat => {
            (At::Path { dirfd: a[0], path: path(a[1])?, follow: true }, Change::Mode(a[2] as libc::mode_t))
        }
        crate::seccomp::SYS_FCHMODAT2 => {
            let flags = a[3] as i32;
            if flags & !(NOFOLLOW | EMPTY) != 0 {
                return Err(libc::EINVAL);
            }
            let at = with_flags(a[0], path(a[1])?, flags, Again::Fchmodat2(flags))?;
            (at, Change::Mode(a[2] as libc::mode_t))
        }
        libc::SYS_fchown => (At::Fd(low(a[0]), Again::Owner), Change::Owner(a[1] as u32, a[2] as u32)),
        libc::SYS_fchownat => {
            let flags = a[4] as i32;
            if flags & !(NOFOLLOW | EMPTY) != 0 {
                return Err(libc::EINVAL);
            }
            (with_flags(a[0], path(a[1])?, flags, Again::Owner)?, Change::Owner(a[2] as u32, a[3] as u32))
        }
        #[cfg(target_arch = "x86_64")]
        libc::SYS_chown | libc::SYS_lchown => {
            let follow = n.data.nr as libc::c_long == libc::SYS_chown;
            (
                At::Path { dirfd: libc::AT_FDCWD as u64, path: path(a[0])?, follow },
                Change::Owner(a[1] as u32, a[2] as u32),
            )
        }
        libc::SYS_utimensat => {
            let t = times(&c, a[2], Times::Timespecs)?;
            // (both times `UTIME_OMIT`: nothing to do — the kernel looks at neither the path nor the flags)
            if t.is_some_and(|t| t[1] == libc::UTIME_OMIT && t[3] == libc::UTIME_OMIT) {
                return Ok(0);
            }
            let flags = a[3] as i32;
            if flags & !(NOFOLLOW | EMPTY) != 0 {
                return Err(libc::EINVAL);
            }
            let t = Change::Times(t);
            match a[1] {
                // (a path of none: `futimens` — dfd must not be AT_FDCWD and flags must be 0)
                0 if cwd(a[0]) => return Err(libc::EFAULT),
                0 if flags != 0 => return Err(libc::EINVAL),
                0 => (At::Fd(low(a[0]), Again::Utimensat { empty: false, flags: 0 }), t),
                at => (with_flags(a[0], path(at)?, flags, Again::Utimensat { empty: true, flags })?, t),
            }
        }
        #[cfg(target_arch = "x86_64")]
        libc::SYS_chmod => (
            At::Path { dirfd: libc::AT_FDCWD as u64, path: path(a[0])?, follow: true },
            Change::Mode(a[1] as libc::mode_t),
        ),
        #[cfg(target_arch = "x86_64")]
        libc::SYS_utime => (
            At::Path { dirfd: libc::AT_FDCWD as u64, path: path(a[0])?, follow: true },
            Change::Times(times(&c, a[1], Times::Utimbuf)?),
        ),
        #[cfg(target_arch = "x86_64")]
        libc::SYS_utimes => (
            At::Path { dirfd: libc::AT_FDCWD as u64, path: path(a[0])?, follow: true },
            Change::Times(times(&c, a[1], Times::Timevals)?),
        ),
        #[cfg(target_arch = "x86_64")]
        libc::SYS_futimesat => {
            let t = Change::Times(times(&c, a[2], Times::Timevals)?);
            match a[1] {
                0 if cwd(a[0]) => return Err(libc::EFAULT),
                0 => (At::Fd(low(a[0]), Again::Utimensat { empty: false, flags: 0 }), t),
                at => (At::Path { dirfd: a[0], path: path(at)?, follow: true }, t),
            }
        }
        nr => return xattr_call(p, &c, nr, a),
    };
    answered(p, &c, at, Asked::Read(change))
}

/// An extended attribute call: its arguments read where this kernel reads them (`Order`) — the `*xattrat` calls
/// (Linux 6.13) always first.
fn xattr_call(p: &Served, c: &Caller, nr: libc::c_long, a: [u64; 6]) -> Answer {
    let path = |at: u64| c_path(c, at);
    let sets = XattrArgs::Set { name: a[1], value: a[2], size: a[3], flags: a[4] as i32 };
    let removes = XattrArgs::Remove { name: a[1] };
    let (at, change) = match nr {
        libc::SYS_setxattr | libc::SYS_lsetxattr => {
            let x = sets.asked(c, false)?;
            let follow = nr == libc::SYS_setxattr;
            (At::Path { dirfd: libc::AT_FDCWD as u64, path: path(a[0])?, follow }, x)
        }
        libc::SYS_fsetxattr => (At::Fd(low(a[0]), Again::Xattr), sets.asked(c, true)?),
        libc::SYS_removexattr | libc::SYS_lremovexattr => {
            let x = removes.asked(c, false)?;
            let follow = nr == libc::SYS_removexattr;
            (At::Path { dirfd: libc::AT_FDCWD as u64, path: path(a[0])?, follow }, x)
        }
        libc::SYS_fremovexattr => (At::Fd(low(a[0]), Again::Xattr), removes.asked(c, true)?),
        SYS_SETXATTRAT | SYS_REMOVEXATTRAT => {
            if !xattrat_here() {
                return Err(libc::ENOSYS);
            }
            let setting = nr == SYS_SETXATTRAT;
            // (`setxattrat`'s `struct xattr_args` first, then its flags)
            let args = if setting { Some(xattr_args(c, a[4], a[5])?) } else { None };
            let flags = a[2] as i32;
            if flags & !(NOFOLLOW | EMPTY) != 0 {
                return Err(libc::EINVAL);
            }
            let x = match args {
                Some((value, size, xflags)) => set(c, a[3], value, size, xflags)?,
                None => Change::Xattr(xattr_name(c, a[3])?, None),
            };
            // (no path with `AT_EMPTY_PATH`: an empty one; `removexattrat`'s on where the caller works: `EBADF`, as
            // the kernel answers it — `setxattrat`'s there is that folder, as `fchownat`'s)
            let named = match a[1] {
                0 if flags & EMPTY != 0 => Vec::new(),
                at => path(at)?,
            };
            if !setting && named.is_empty() && flags & EMPTY != 0 && cwd(a[0]) {
                return Err(libc::EBADF);
            }
            (with_flags(a[0], named, flags, Again::Xattr)?, Asked::Read(x))
        }
        _ => return Err(libc::ENOSYS),
    };
    answered(p, c, at, change)
}

/// A call whose file is `at`, answered.
fn answered(p: &Served, c: &Caller, at: At, change: Asked) -> Answer {
    let (dirfd, path, follow) = match at {
        At::Fd(fd, again) => return on_fd(p, c, fd, change, again),
        // (an empty path names nothing — `ENOENT`, as the kernel says it; `AT_EMPTY_PATH`'s became `.` or a descriptor)
        At::Path { path, .. } if path.is_empty() => return Err(libc::ENOENT),
        At::Path { dirfd, path, follow } => (dirfd, path, follow),
    };
    if let Some(fd) = self_fd(&path) {
        return on_fd(p, c, fd, change, Again::Proc { follow });
    }
    let (said, op) = (|| String::from_utf8_lossy(&path).into_owned(), change.op());
    if through_proc(&path) {
        unopened(change, c)?;
        return refuse(p, c, op, said());
    }
    match open_in_run(c, dirfd, &path, follow, true) {
        // (a magic link on the way — the same open without the guard gets past it: refused, never followed for the
        // run; a loop of links is the kernel's own ELOOP)
        Err(libc::ELOOP) if open_in_run(c, dirfd, &path, follow, false).err() != Some(libc::ELOOP) => {
            unopened(change, c)?;
            refuse(p, c, op, said())
        }
        file => on_path(p, c, &file?, change),
    }
}

/// A call tarae refuses for its own reasons before it opens a file for the run (a path through `/proc`, a magic link on
/// the way): the kernel would find the file, then read the arguments — a bad one is its own answer, unsaid. Only they
/// decide it: the file isn't the run's to ask about (an unguarded open resolves `/proc/self` as tarae), so neither is
/// its mount.
fn unopened(change: Asked, m: &impl Mem) -> Result<(), i32> {
    change.read(m, || false).map(drop)
}

/// Where a call with `AT_SYMLINK_NOFOLLOW`/`AT_EMPTY_PATH` (`flags`) on `path` from `dirfd` is: an empty path with
/// `AT_EMPTY_PATH` is the folder argument itself (`AT_FDCWD`: where the caller works) — made `again` on a
/// descriptor; without it, `ENOENT`, as the kernel says it.
fn with_flags(dirfd: u64, path: Vec<u8>, flags: i32, again: Again) -> Result<At, i32> {
    let follow = flags & NOFOLLOW == 0;
    Ok(match (path.is_empty(), flags & EMPTY != 0) {
        (false, _) => At::Path { dirfd, path, follow },
        (true, false) => return Err(libc::ENOENT),
        (true, true) if cwd(dirfd) => At::Path { dirfd, path: b".".to_vec(), follow },
        (true, true) => At::Fd(low(dirfd), again),
    })
}

/// `path` from `dirfd` opened `O_PATH` by the kernel as the run would reach it: an absolute one from the run's root
/// (`/proc/<tid>/root`, `RESOLVE_IN_ROOT`); a relative one from where the run works by that folder's name from its
/// root too, while the name leads the run to the folder it works in — else, as from tarae's copy of `dirfd`
/// (`EBADF` where it has none), from the folder itself, where an absolute link met on the way resolves from tarae's
/// root, which in tier own isn't the run's: what it opens is decided only if its name leads the run to it
/// (`refused`). A magic link on the way, `guarded`: `ELOOP` (`RESOLVE_NO_MAGICLINKS`).
fn open_in_run(c: &Caller, dirfd: u64, path: &[u8], follow: bool, guarded: bool) -> Result<OwnedFd, i32> {
    let (from, resolve) = match path.first() {
        Some(b'/') => {
            let root = std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_PATH | libc::O_DIRECTORY)
                .open(format!("/proc/{}/root", c.tid));
            (OwnedFd::from(root.map_err(|_| libc::ESRCH)?), libc::RESOLVE_IN_ROOT)
        }
        _ if cwd(dirfd) => {
            let here = c.open_cwd()?;
            let named = |w: &Path| w.is_absolute() && stat(&here).is_ok_and(|st| named_there(c, w, &st));
            if let Ok(w) = c.cwd()
                && named(&w)
            {
                let at = w.join(OsStr::from_bytes(path));
                return open_in_run(c, libc::AT_FDCWD as u64, at.as_os_str().as_bytes(), follow, guarded);
            }
            (here, 0)
        }
        _ => (c.fd(low(dirfd))?, 0),
    };
    let path = CString::new(path).map_err(|_| libc::EINVAL)?;
    // SAFETY: an all-zero `open_how` is its empty value.
    let mut how: libc::open_how = unsafe { std::mem::zeroed() };
    how.flags = (libc::O_PATH | libc::O_CLOEXEC | if follow { 0 } else { libc::O_NOFOLLOW }) as u64;
    how.resolve = resolve | if guarded { libc::RESOLVE_NO_MAGICLINKS } else { 0 };
    let size = std::mem::size_of::<libc::open_how>();
    // SAFETY: a descriptor of ours, a path and an `open_how` that live through the call.
    let fd =
        unsafe { libc::syscall(libc::SYS_openat2, from.as_raw_fd(), path.as_ptr(), &raw const how, size) };
    if fd < 0 {
        return Err(errno(&io::Error::last_os_error()));
    }
    // SAFETY: a descriptor the call just made, ours alone.
    Ok(unsafe { OwnedFd::from_raw_fd(fd as i32) })
}

/// A call on `file` (opened in the run's view, its last part's link followed or not): refused where the run may not
/// write, or where its name doesn't lead the run to it; else made by tarae on its own descriptor of it.
fn on_path(p: &Served, c: &Caller, file: &OwnedFd, change: Asked) -> Answer {
    let st = stat(file)?;
    let real = name_of(file)?;
    let change = change.read(c, || read_only(file))?;
    if changes_owner(&change, &st) {
        return owner_refused(p, c, file, &real, &st, &change);
    }
    if let Some(no) = xattr_refused(p, c, file, &real, &st, &change) {
        return no;
    }
    let change = acl_ids(c, change)?;
    if let Some(no) = refused(p, c, file, &real, &st, &change, false) {
        return no;
    }
    match change {
        Change::Mode(_) if st.st_mode & libc::S_IFMT == libc::S_IFLNK => Err(libc::EOPNOTSUPP),
        Change::Mode(mode) => chmod_proc(file, mode),
        // (an empty path: the descriptor's own file, a link itself too)
        Change::Times(t) => utimensat(file.as_raw_fd(), c"".as_ptr(), t, EMPTY),
        Change::Owner(uid, gid) => chown_unchanged(file, &Change::Owner(uid, gid), &st),
        // (through its `/proc/self/fd/M`, followed — `fsetxattr` refuses an `O_PATH` descriptor: the file it was
        // opened on, a link itself too, measured)
        Change::Xattr(name, value) => {
            xattr_made(XattrAt::Path(&proc_path(file), true), &name, value.as_ref())
        }
    }
}

/// A call on the caller's descriptor `fd`: decided on what it names — a file by its name (`/proc/self/fd/M`'s link:
/// where it is now), never one the name no longer leads to; no file of the user's (`wire::no_name`: no name left, a
/// pipe, a socket), the run's — and made `again` on tarae's copy.
fn on_fd(p: &Served, c: &Caller, fd: u64, change: Asked, again: Again) -> Answer {
    let file = c.fd(fd)?;
    let st = stat(&file)?;
    let proc = proc_path(&file);
    let name = name_of(&file)?;
    fd_first(&change, again, &file)?;
    let change = change.read(c, || read_only(&file))?;
    if changes_owner(&change, &st) {
        return owner_refused(p, c, &file, &name, &st, &change);
    }
    if let Some(no) = xattr_refused(p, c, &file, &name, &st, &change) {
        return no;
    }
    let change = acl_ids(c, change)?;
    let named = || named_there(c, &name, &st);
    if !no_name(name.as_os_str().as_bytes(), st.st_nlink == 0, named)
        && let Some(no) = refused(p, c, &file, &name, &st, &change, writable(&file))
    {
        return no;
    }
    let f = file.as_raw_fd();
    match (again, change) {
        (_, Change::Owner(uid, gid)) => chown_unchanged(&file, &Change::Owner(uid, gid), &st),
        (Again::Xattr, Change::Xattr(name, value)) => xattr_made(XattrAt::Fd(f), &name, value.as_ref()),
        (Again::Proc { follow }, Change::Xattr(name, value)) => {
            xattr_made(XattrAt::Path(&proc, follow), &name, value.as_ref())
        }
        // SAFETY: plain calls on tarae's own descriptor.
        (Again::Fchmod, Change::Mode(mode)) => result(unsafe { libc::fchmod(f, mode) } as isize),
        (Again::Fchmodat2(flags), Change::Mode(mode)) => {
            // SAFETY: tarae's descriptor and an empty path that lives through the call.
            result(unsafe { libc::syscall(crate::seccomp::SYS_FCHMODAT2, f, c"".as_ptr(), mode, flags) }
                as isize)
        }
        (Again::Utimensat { empty, flags }, Change::Times(t)) => {
            utimensat(f, if empty { c"".as_ptr() } else { std::ptr::null() }, t, flags)
        }
        (Again::Proc { follow: false }, Change::Mode(_)) => Err(libc::EOPNOTSUPP),
        (Again::Proc { follow: true }, Change::Mode(mode)) => chmod_proc(&file, mode),
        (Again::Proc { follow }, Change::Times(t)) => {
            utimensat(libc::AT_FDCWD, proc.as_ptr(), t, if follow { 0 } else { NOFOLLOW })
        }
        _ => Err(libc::EINVAL),
    }
}

/// The refusal of a call on `file`, named `name` (`st` its stat): where the run may not write, or where `name`
/// doesn't lead the run to that file — from its own root, through no link (a cover's, one moved or put there since,
/// a name the kernel says is gone). `EROFS` on a read-only mount (a B9 seal), as the kernel answers it there, else
/// `EPERM`. A descriptor the run holds open for writing (`written`) is let wherever: Landlock let the run write that
/// file when it opened it, so its mode and times follow what it may write in it (a protected name the run made where
/// none was too: the snapshot's card asks about it, G23) — its name must still lead the run to it.
fn refused(
    p: &Served,
    c: &Caller,
    file: &OwnedFd,
    name: &Path,
    st: &libc::stat,
    change: &Change,
    written: bool,
) -> Option<Answer> {
    if (written || p.writes_allow(name)) && named_there(c, name, st) {
        return None;
    }
    let errno = if read_only(file) { libc::EROFS } else { libc::EPERM };
    // (an extended attribute's change the kernel would refuse anyway: its own answer, unsaid — a seal's `EROFS` is
    // the sandbox's, said)
    if let (libc::EPERM, Change::Xattr(n, _)) = (errno, change)
        && let Some(own) = xattr_kernel_refuses(file, st, n.as_bytes())
    {
        return Some(Err(own));
    }
    Some(refuse_with(p, c, errno, change.op(), name.display().to_string()))
}

/// Whether `name` leads the run, from its own root and through no link, to the file `st` is.
fn named_there(c: &Caller, name: &Path, st: &libc::stat) -> bool {
    let there = open_in_run(c, libc::AT_FDCWD as u64, name.as_os_str().as_bytes(), false, true);
    there.and_then(|t| stat(&t)).is_ok_and(|t| (t.st_dev, t.st_ino) == (st.st_dev, st.st_ino))
}

/// Whether an owner call changes the owner or the group — each asked for (not `-1`) and not what the file has: never
/// let, in no place (`rsync -a`'s and `cp -a`'s ask for what the file has, and are).
fn changes_owner(change: &Change, st: &libc::stat) -> bool {
    matches!(*change, Change::Owner(uid, gid)
        if ![u32::MAX, st.st_uid].contains(&uid) || ![u32::MAX, st.st_gid].contains(&gid))
}

/// A real change of owner or group, refused: `EROFS` on a read-only mount (a B9 seal), as the kernel answers it there,
/// else `EPERM` — said, but where the kernel refuses that call anyway (`kernel_lets`): its own `EPERM`, unsaid, as
/// outside (a user's `cp -a` of another's file asks for that owner and goes on without it).
fn owner_refused(
    p: &Served,
    c: &Caller,
    file: &OwnedFd,
    name: &Path,
    st: &libc::stat,
    change: &Change,
) -> Answer {
    let errno = if read_only(file) { libc::EROFS } else { libc::EPERM };
    if errno == libc::EPERM && !kernel_lets(change, st) {
        return Err(libc::EPERM);
    }
    refuse_with(p, c, errno, change.op(), name.display().to_string())
}

/// Whether the kernel lets tarae's credentials — the run's user's — make an owner call on `st`'s file (`chown_ok`,
/// `chgrp_ok`): root any (`CAP_CHOWN`; a root without it is said); else only the file's owner, its owner kept and
/// its group one of the caller's.
fn kernel_lets(change: &Change, st: &libc::stat) -> bool {
    // SAFETY: a plain call.
    lets(unsafe { libc::geteuid() }, change, st, in_group)
}

/// `kernel_lets` for a caller of `euid` in the groups `in_group` says.
fn lets(euid: u32, change: &Change, st: &libc::stat, in_group: impl Fn(u32) -> bool) -> bool {
    let Change::Owner(uid, gid) = *change else { return true };
    let owner = euid == st.st_uid;
    euid == 0
        || (uid == u32::MAX || owner && uid == st.st_uid)
            && (gid == u32::MAX || owner && (gid == st.st_gid || in_group(gid)))
}

/// Whether `gid` is tarae's effective group or one of its supplementary groups (`in_group_p`).
pub(in crate::sandbox) fn in_group(gid: u32) -> bool {
    // SAFETY: plain calls; the buffer holds what the first call counted, and only what the second filled is read.
    unsafe {
        if libc::getegid() == gid {
            return true;
        }
        let n = libc::getgroups(0, std::ptr::null_mut()).max(0);
        let mut groups = vec![0; n as usize];
        let filled = libc::getgroups(n, groups.as_mut_ptr()).max(0) as usize;
        groups[..filled].contains(&gid)
    }
}

/// An owner call that changes nothing, made on tarae's own descriptor of the file it decided on (`-1`, `-1`, through no
/// link): the kernel's own side effects — its change time, a set-user-ID or set-group-ID bit cleared — as the run's
/// call would have had them. But the kernel's own `EPERM` where it refuses that call (`kernel_lets`: an id given
/// as the file's own, on a file not the caller's) — `-1`, `-1` alone it lets anyone make.
fn chown_unchanged(fd: &OwnedFd, change: &Change, st: &libc::stat) -> Answer {
    if !kernel_lets(change, st) {
        return Err(libc::EPERM);
    }
    // SAFETY: tarae's descriptor and an empty path that lives through the call.
    result(unsafe { libc::fchownat(fd.as_raw_fd(), c"".as_ptr(), u32::MAX, u32::MAX, EMPTY) } as isize)
}

/// A descriptor's extended attribute call whose arguments wait for its file (`change`): an `O_PATH` descriptor is none
/// to it (`fdget`) — its `EBADF` before the arguments. (Read first, the call on tarae's copy says it after them.)
fn fd_first(change: &Asked, again: Again, file: &OwnedFd) -> Result<(), i32> {
    if again == Again::Xattr && matches!(change, Asked::Unread(..)) && opened_path(file) {
        return Err(libc::EBADF);
    }
    Ok(())
}

/// Whether tarae's copy of a run's descriptor is an `O_PATH` one.
fn opened_path(fd: &OwnedFd) -> bool {
    // SAFETY: a descriptor of ours.
    let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFL) };
    flags >= 0 && flags & libc::O_PATH != 0
}

/// Whether tarae's copy of a run's descriptor is open for writing (`O_WRONLY`, `O_RDWR`; never `O_PATH`).
fn writable(fd: &OwnedFd) -> bool {
    // SAFETY: a descriptor of ours.
    let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFL) };
    flags >= 0
        && flags & libc::O_PATH == 0
        && matches!(flags & libc::O_ACCMODE, libc::O_WRONLY | libc::O_RDWR)
}

/// `setxattr`'s change: its flags (`XATTR_CREATE`, `XATTR_REPLACE`, else `EINVAL`), then its name at `name` and its
/// value of `size` bytes at `value`, each read once — the kernel's order (`setxattr_copy`).
fn set(m: &impl Mem, name: u64, value: u64, size: u64, flags: i32) -> Result<Change, i32> {
    if flags & !(libc::XATTR_CREATE | libc::XATTR_REPLACE) != 0 {
        return Err(libc::EINVAL);
    }
    let name = xattr_name(m, name)?;
    Ok(Change::Xattr(name, Some((xattr_value(m, value, size)?, flags))))
}

/// Whether this kernel has `setxattrat` and `removexattrat` (Linux 6.13) — asked once, with a size no call takes:
/// `EINVAL` where it has them, `ENOSYS` where not — the run's own answer there.
fn xattrat_here() -> bool {
    static HERE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    // SAFETY: a call that reads nothing: its size is refused first.
    *HERE.get_or_init(
        || unsafe { libc::syscall(SYS_SETXATTRAT, -1, 0, 0, 0, 0, 0) } < 0 && errno_now() != libc::ENOSYS,
    )
}

fn errno_now() -> i32 {
    io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

/// Whose an extended attribute's name is (the kernel's namespaces: `xattr_resolve_name`).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Namespace {
    /// `user.` and the two ACLs: served where the run may write, as a mode.
    Served,
    /// `security.`, `trusted.`, any other `system.`: never — a program's capabilities (`security.capability`) or a
    /// security module's labels, set by tarae, root in a container, would be the run's to give.
    Refused,
    /// No namespace the kernel has: its own `EOPNOTSUPP`, nothing set — after its write check, as it judges the
    /// file first (`xattr_permission`).
    None,
}

/// The two POSIX ACLs' names.
const ACLS: [&[u8]; 2] = [b"system.posix_acl_access", b"system.posix_acl_default"];

fn namespace(name: &[u8]) -> Namespace {
    match name {
        n if ACLS.contains(&n) => Namespace::Served,
        n if n.starts_with(b"user.") => Namespace::Served,
        n if [&b"security."[..], b"trusted.", b"system."].iter().any(|w| n.starts_with(w)) => {
            Namespace::Refused
        }
        _ => Namespace::None,
    }
}

/// The answer to an extended attribute's change by its name alone (`namespace`), wherever its file (`fd`, named
/// `file`, `st` its stat) is: refused and said — but the kernel's own answer, unsaid, where it would refuse that call
/// anyway (`xattr_kernel_refuses`) — or the kernel's `EOPNOTSUPP`; None for one served — decided then as a mode is.
fn xattr_refused(
    p: &Served,
    c: &Caller,
    fd: &OwnedFd,
    file: &Path,
    st: &libc::stat,
    change: &Change,
) -> Option<Answer> {
    let Change::Xattr(name, _) = change else { return None };
    match namespace(name.as_bytes()) {
        Namespace::Served => None,
        Namespace::Refused => Some(match xattr_kernel_refuses(fd, st, name.as_bytes()) {
            Some(own) => Err(own),
            None => refuse(p, c, change.op(), file.display().to_string()),
        }),
        // (the kernel's write check comes first: a file the user may not write is its `EACCES`, an immutable one's
        // `EPERM`, a read-only mount's `EROFS`)
        Namespace::None => Some(Err(write_refused(fd).unwrap_or(libc::EOPNOTSUPP))),
    }
}

/// An ACL's value with its user and group ids, which the run gave in its own user namespace, made tarae's: the kernel
/// reads them in the caller's (`posix_acl_from_xattr`), so an id the run's namespace doesn't map is its own `EINVAL`
/// there — never set as whoever that number is in tarae's (tier own maps one uid and one gid). Any other change, and
/// a value the kernel refuses for its shape, as it is: the kernel answers it.
fn acl_ids(c: &Caller, change: Change) -> Result<Change, i32> {
    match change {
        Change::Xattr(name, Some((value, flags))) if ACLS.contains(&name.as_bytes()) => {
            let map =
                |f: &str| std::fs::read_to_string(format!("/proc/{}/{f}", c.tid)).map_err(|_| libc::ESRCH);
            let value = acl_in(value, &map("uid_map")?, &map("gid_map")?)?;
            Ok(Change::Xattr(name, Some((value, flags))))
        }
        other => Ok(other),
    }
}

/// `acl_ids` on `value` (`posix_acl_xattr_header`, version 2, then entries of tag, permissions and id, little-endian)
/// with the run's `uid_map` and `gid_map` as tarae reads them: `inside outside count` a line, `outside` in tarae's
/// namespace.
fn acl_in(mut value: Vec<u8>, uid_map: &str, gid_map: &str) -> Result<Vec<u8>, i32> {
    const ACL_USER: u16 = 2;
    const ACL_GROUP: u16 = 8;
    if value.len() < 4 || value[..4] != 2u32.to_le_bytes() || !(value.len() - 4).is_multiple_of(8) {
        return Ok(value);
    }
    for e in value[4..].as_chunks_mut::<8>().0 {
        let map = match u16::from_le_bytes([e[0], e[1]]) {
            ACL_USER => uid_map,
            ACL_GROUP => gid_map,
            _ => continue,
        };
        let id = u32::from_le_bytes([e[4], e[5], e[6], e[7]]);
        e[4..].copy_from_slice(&mapped(map, id).ok_or(libc::EINVAL)?.to_le_bytes());
    }
    Ok(value)
}

/// `id` of a namespace as its map (`inside outside count` lines) gives it to the reader; None where the map has none.
fn mapped(map: &str, id: u32) -> Option<u32> {
    map.lines().find_map(|l| {
        let mut f = l.split_whitespace().map(|f| f.parse::<u64>().ok());
        let (inside, outside, count) = (f.next()??, f.next()??, f.next()??);
        let past = u64::from(id).checked_sub(inside).filter(|&d| d < count)?;
        u32::try_from(outside + past).ok()
    })
}

/// Where tarae refuses an extended attribute's change, the kernel's own answer when it would refuse that call anyway
/// with tarae's credentials — the run's user's — as sandbox C's owner calls have it (`kernel_lets`): unsaid, as
/// outside a sandbox (a user's `cp --preserve=all` of another's file goes on without what it can't copy). The kernel's
/// rules (`xattr_permission`, the ACL and `security.capability` handlers): `trusted.` needs `CAP_SYS_ADMIN`;
/// `security.capability` `CAP_SETFCAP`; an ACL the file's owner (or `CAP_FOWNER`); `user.` a file or a folder (a
/// sticky one its owner's), and write permission on it (`faccessat` through tarae's own descriptor, as the kernel
/// judges it: `EACCES`, an immutable file's `EPERM`). None where it would let it, or where only trying could tell —
/// another `security.` name is a security module's to judge, another `system.` a file system's.
fn xattr_kernel_refuses(fd: &OwnedFd, st: &libc::stat, name: &[u8]) -> Option<i32> {
    const CAP_FOWNER: u32 = 3;
    const CAP_SYS_ADMIN: u32 = 21;
    const CAP_SETFCAP: u32 = 31;
    // SAFETY: a plain call.
    let mine = unsafe { libc::geteuid() } == st.st_uid || holds(CAP_FOWNER);
    let kind = st.st_mode & libc::S_IFMT;
    let refused = |no: bool| no.then_some(libc::EPERM);
    match name {
        n if n.starts_with(b"trusted.") => refused(!holds(CAP_SYS_ADMIN)),
        b"security.capability" => refused(!holds(CAP_SETFCAP)),
        n if ACLS.contains(&n) => refused(!mine),
        n if n.starts_with(b"user.") => {
            if ![libc::S_IFREG, libc::S_IFDIR].contains(&kind)
                || kind == libc::S_IFDIR && st.st_mode & libc::S_ISVTX != 0 && !mine
            {
                return Some(libc::EPERM);
            }
            write_refused(fd)
        }
        _ => None,
    }
}

/// The kernel's write check on tarae's own descriptor's file (`inode_permission(MAY_WRITE)`), as
/// `faccessat(W_OK, AT_EACCESS)` through its `/proc/self/fd/M` judges it: `EACCES`, an immutable file's `EPERM`, a
/// read-only mount's `EROFS`; None where it may write.
fn write_refused(fd: &OwnedFd) -> Option<i32> {
    let p = proc_path(fd);
    // SAFETY: a path that lives through the call.
    let r = unsafe { libc::faccessat(libc::AT_FDCWD, p.as_ptr(), libc::W_OK, libc::AT_EACCESS) };
    (r != 0).then(errno_now)
}

/// Whether tarae holds capability `cap` (`/proc/self/status`'s `CapEff`) — what the kernel asks for some calls.
pub(in crate::sandbox) fn holds(cap: u32) -> bool {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let caps = status.lines().find_map(|l| l.strip_prefix("CapEff:"));
    caps.and_then(|v| u64::from_str_radix(v.trim(), 16).ok()).is_some_and(|c| c & (1 << cap) != 0)
}

/// Where tarae makes an extended attribute's change: a path of its own (`/proc/self/fd/M`; followed or not), or its
/// own descriptor.
enum XattrAt<'a> {
    Path(&'a std::ffi::CStr, bool),
    Fd(i32),
}

/// An extended attribute set to `value` (its bytes and flags) or removed (None), by tarae.
fn xattr_made(at: XattrAt, name: &std::ffi::CStr, value: Option<&(Vec<u8>, i32)>) -> Answer {
    let n = name.as_ptr();
    // SAFETY: tarae's own path or descriptor, a name and a value that live through the call (an empty value's
    // address, never read).
    let r = unsafe {
        match (at, value) {
            (XattrAt::Path(p, true), Some((v, f))) => {
                libc::setxattr(p.as_ptr(), n, v.as_ptr().cast(), v.len(), *f)
            }
            (XattrAt::Path(p, false), Some((v, f))) => {
                libc::lsetxattr(p.as_ptr(), n, v.as_ptr().cast(), v.len(), *f)
            }
            (XattrAt::Fd(fd), Some((v, f))) => libc::fsetxattr(fd, n, v.as_ptr().cast(), v.len(), *f),
            (XattrAt::Path(p, true), None) => libc::removexattr(p.as_ptr(), n),
            (XattrAt::Path(p, false), None) => libc::lremovexattr(p.as_ptr(), n),
            (XattrAt::Fd(fd), None) => libc::fremovexattr(fd, n),
        }
    };
    result(r as isize)
}

fn stat(fd: &OwnedFd) -> Result<libc::stat, i32> {
    let mut st = MaybeUninit::<libc::stat>::uninit();
    // SAFETY: a descriptor of ours and a buffer the call fills.
    if unsafe { libc::fstat(fd.as_raw_fd(), st.as_mut_ptr()) } != 0 {
        return Err(errno(&io::Error::last_os_error()));
    }
    // SAFETY: filled by the successful fstat above.
    Ok(unsafe { st.assume_init() })
}

/// Whether `fd`'s mount is read-only (`statvfs`'s flags hold the mount's own, a read-only bind's too).
fn read_only(fd: &OwnedFd) -> bool {
    let mut s = MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: a descriptor of ours and a buffer the call fills; read only once filled.
    unsafe {
        libc::fstatvfs(fd.as_raw_fd(), s.as_mut_ptr()) == 0 && s.assume_init().f_flag & libc::ST_RDONLY != 0
    }
}

/// What tarae's own descriptor `fd` names now (its `/proc/self/fd/M` link): a path, ` (deleted)` after one gone, or
/// `pipe:[…]`, `socket:[…]`.
fn name_of(fd: &OwnedFd) -> Result<std::path::PathBuf, i32> {
    std::fs::read_link(format!("/proc/self/fd/{}", fd.as_raw_fd())).map_err(|e| errno(&e))
}

/// `/proc/self/fd/M`: tarae's own descriptor `fd` as a path — what it reaches is that file, whatever's at its name.
fn proc_path(fd: &OwnedFd) -> CString {
    CString::new(format!("/proc/self/fd/{}", fd.as_raw_fd())).unwrap_or_default()
}

/// The mode of tarae's own descriptor's file, through its `/proc/self/fd/M` (an `O_PATH` one's too, which `fchmod`
/// refuses — Linux 5.14 has no `fchmodat2` for it).
fn chmod_proc(fd: &OwnedFd, mode: libc::mode_t) -> Answer {
    let proc = proc_path(fd);
    // SAFETY: a path that lives through the call.
    result(unsafe { libc::syscall(libc::SYS_fchmodat, libc::AT_FDCWD, proc.as_ptr(), mode) } as isize)
}

/// `utimensat(dirfd, path, times, flags)` raw (glibc refuses a path of none), the times tarae's own copy.
fn utimensat(dirfd: i32, path: *const libc::c_char, t: Option<[i64; 4]>, flags: i32) -> Answer {
    let ts = t.map(|t| {
        [libc::timespec { tv_sec: t[0], tv_nsec: t[1] }, libc::timespec { tv_sec: t[2], tv_nsec: t[3] }]
    });
    let at = ts.as_ref().map_or(std::ptr::null(), |ts| ts.as_ptr());
    // SAFETY: the path (or none) and the times (or none) live through the call.
    result(unsafe { libc::syscall(libc::SYS_utimensat, dirfd, path, at, flags) } as isize)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::sandbox::wire::check::Fake;

    /// A run's memory holding an extended attribute's name (`NAME`), an empty one (`BLANK`) and a value (`VALUE`).
    const NAME: u64 = 0x1000;
    const BLANK: u64 = 0x2000;
    const VALUE: u64 = 0x3000;

    fn memory() -> Fake {
        // (a name's read asks for up to 256 bytes at once: each laid in that much)
        let name = |s: &[u8]| [s, &[0; 256][s.len()..]].concat();
        Fake::default().at(NAME, name(b"user.x")).at(BLANK, name(b"")).at(VALUE, *b"x")
    }

    fn good() -> Change {
        Change::Xattr(c"user.x".into(), Some((b"x".to_vec(), 0)))
    }

    /// #127: each kernel's order, given explicitly (`fs/xattr.c` as v5.14–v6.10, v6.11–v6.12 and v6.13 on have it) —
    /// a call's arguments read at once, or nothing read of the run's memory until its file is found; then, where the
    /// write check comes first, a read-only mount is `EROFS` whatever bad arguments it has, and good ones are the
    /// change on it as anywhere.
    #[test]
    fn an_xattr_calls_arguments_are_read_in_each_kernels_order() {
        let bad_flag = XattrArgs::Set { name: NAME, value: VALUE, size: 1, flags: 4 };
        let empty_name = XattrArgs::Remove { name: BLANK };
        let set = XattrArgs::Set { name: NAME, value: VALUE, size: 1, flags: 0 };
        // (order, by a descriptor, the arguments read at once)
        for (order, by_fd, first) in [
            (Order::File, false, false),
            (Order::File, true, false),
            (Order::Fd, false, true),
            (Order::Fd, true, false),
            (Order::Args, false, true),
            (Order::Args, true, true),
        ] {
            let at = format!("{order:?}, by a descriptor {by_fd}");
            for (args, bad) in [(bad_flag, libc::EINVAL), (empty_name, libc::ERANGE)] {
                let m = memory();
                let asked = args.asked_in(&m, by_fd, order);
                if first {
                    assert_eq!(asked.err(), Some(bad), "{at}: the arguments' error at once");
                    continue;
                }
                let asked = asked.unwrap_or_else(|e| panic!("{at}: read before the file ({e})"));
                assert!(
                    m.reads.borrow().is_empty(),
                    "{at}: nothing of the run's memory read before the file"
                );
                // (the write check first only where the kernel makes it before the arguments: 6.10's both forms)
                let erofs = if order == Order::File { libc::EROFS } else { bad };
                assert_eq!(asked.read(&m, || true).err(), Some(erofs), "{at}: on a read-only mount");
                assert_eq!(
                    args.asked_in(&m, by_fd, order).unwrap().read(&m, || false).err(),
                    Some(bad),
                    "{at}"
                );
            }
            let m = memory();
            let change = set.asked_in(&m, by_fd, order).and_then(|a| a.read(&m, || true));
            assert_eq!(
                change,
                Ok(good()),
                "{at}: good arguments are the change, a read-only mount's left to `refused`"
            );
            let removed = XattrArgs::Remove { name: NAME }
                .asked_in(&m, by_fd, order)
                .and_then(|a| a.read(&m, || false));
            assert_eq!(removed, Ok(Change::Xattr(c"user.x".into(), None)), "{at}");
        }
    }

    /// #127: a path tarae refuses before opening it (through `/proc`, through a magic link) answers a bad argument first
    /// in every order — on 6.10's (`Order::File`), whose arguments wait for the file, too: the kernel would find it,
    /// and only the arguments decide its answer; good ones go on to the refusal.
    #[test]
    fn a_path_refused_before_its_file_is_opened_answers_its_bad_arguments_first() {
        let bad_flag = XattrArgs::Set { name: NAME, value: VALUE, size: 1, flags: 4 };
        let empty_name = XattrArgs::Remove { name: BLANK };
        let set = XattrArgs::Set { name: NAME, value: VALUE, size: 1, flags: 0 };
        let m = memory();
        for (args, want) in [(bad_flag, Err(libc::EINVAL)), (empty_name, Err(libc::ERANGE)), (set, Ok(()))] {
            let asked = args.asked_in(&m, false, Order::File).unwrap();
            assert!(
                matches!(asked, Asked::Unread(_, Reads::AfterWriteCheck)),
                "6.10: read once the file's found"
            );
            assert_eq!(unopened(asked, &m), want);
        }
    }

    /// #127: an `O_PATH` descriptor is none to a descriptor's call (`fdget`): where the kernel looks at the descriptor
    /// before the arguments (to 6.12), its `EBADF` first, whatever they are; where the arguments come first (6.13), their
    /// error; a descriptor open for reading, or a path through `/proc/self/fd/N`, goes on to the arguments.
    #[test]
    fn an_o_path_descriptor_is_ebadf_before_the_arguments_where_the_descriptor_comes_first() {
        let open = |flags| {
            std::fs::OpenOptions::new().read(true).custom_flags(flags).open("/").map(OwnedFd::from).unwrap()
        };
        let (opath, read) = (open(libc::O_PATH), open(0));
        let bad_flag = XattrArgs::Set { name: NAME, value: VALUE, size: 1, flags: 4 };
        let m = memory();
        for order in [Order::File, Order::Fd] {
            let asked = bad_flag.asked_in(&m, true, order).unwrap();
            assert_eq!(fd_first(&asked, Again::Xattr, &opath), Err(libc::EBADF), "{order:?}");
            assert_eq!(fd_first(&asked, Again::Xattr, &read), Ok(()), "{order:?}: read-only");
            assert_eq!(fd_first(&asked, Again::Proc { follow: true }, &opath), Ok(()), "{order:?}: a path");
            assert_eq!(asked.read(&m, || false).err(), Some(libc::EINVAL), "{order:?}: then the arguments");
        }
        assert_eq!(
            bad_flag.asked_in(&m, true, Order::Args).err(),
            Some(libc::EINVAL),
            "6.13: the arguments first"
        );
        let set = XattrArgs::Set { name: NAME, value: VALUE, size: 1, flags: 0 };
        let asked = set.asked_in(&m, true, Order::Args).unwrap();
        assert_eq!(fd_first(&asked, Again::Xattr, &opath), Ok(()), "read: the call on tarae's copy says it");
    }

    /// #127: the answers that tell the three orders apart, and this kernel's — where a filter answers these calls
    /// before the kernel (tier offline's, Docker's default profile, a run's own), it can't tell: said, that part skipped.
    #[test]
    fn the_kernels_order_is_told_by_its_answers() {
        use libc::{EBADF, EINVAL, ENOENT, EPERM, ERANGE};
        for (path, fd, bad, want) in [
            (ENOENT, EBADF, EINVAL, Some(Order::File)),
            (EINVAL, EBADF, EINVAL, Some(Order::Fd)),
            (EINVAL, EINVAL, EINVAL, Some(Order::Args)),
            (ENOENT, EBADF, ERANGE, Some(Order::File)),
            (ERANGE, EBADF, ERANGE, Some(Order::Fd)),
            (ERANGE, ERANGE, ERANGE, Some(Order::Args)),
            // (no kernel looks up a path first and a descriptor last; a filter's answers)
            (ENOENT, EINVAL, EINVAL, None),
            (EPERM, EPERM, EINVAL, None),
        ] {
            assert_eq!(Order::of(path, fd, bad), want, "{path} {fd} {bad}");
        }
        if let Err(why) = read_orders() {
            crate::test_results::said_skip(&format!(
                "sandbox::supervisor::attrs::tests::the_kernels_order_is_told_by_its_answers — this kernel's part: \
                 its answers name no order ({why}), a filter's around this process"
            ));
        }
    }

    #[test]
    fn an_owner_call_is_let_as_the_kernel_lets_it() {
        // SAFETY: an all-zero `stat` is a valid value; only its ids are read.
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        (st.st_uid, st.st_gid) = (1000, 1000);
        let none = u32::MAX;
        // (the caller: 1000 in groups 1000 and 20, or 1001, or root)
        let groups = |g| [1000, 20].contains(&g);
        for (euid, uid, gid, want) in [
            (1000, none, none, true),
            (1000, 1000, 1000, true),
            (1000, none, 20, true),
            (1000, none, 30, false),
            (1000, 1001, none, false),
            // (not the file's: its own ids given are refused, `-1`, `-1` alone let)
            (1001, 1000, none, false),
            (1001, none, 1000, false),
            (1001, none, none, true),
            (0, 1001, 30, true),
        ] {
            assert_eq!(lets(euid, &Change::Owner(uid, gid), &st, groups), want, "{euid}: {uid} {gid}");
        }
    }

    /// D2 B: an ACL's user and group ids are the run's, in its user namespace — made tarae's by its maps, one they
    /// don't hold the kernel's `EINVAL` there; owner, group, mask and other carry none; a value the kernel refuses
    /// for its shape is left for its own answer.
    #[test]
    fn an_acls_ids_are_read_in_the_runs_namespace() {
        let acl = |entries: &[(u16, u32)]| {
            let mut v = 2u32.to_le_bytes().to_vec();
            for &(tag, id) in entries {
                v.extend(tag.to_le_bytes());
                v.extend(4u16.to_le_bytes());
                v.extend(id.to_le_bytes());
            }
            v
        };
        let none = u32::MAX;
        // (tier own's maps, a nested namespace's, the initial one's)
        let (own, nested, host) = ("1000 1000 1\n", "0 100000 65536\n", "0 0 4294967295\n");
        let mine = acl(&[(1, none), (2, 1000), (4, none), (8, 1000), (0x10, none), (0x20, none)]);
        assert_eq!(acl_in(mine.clone(), own, own), Ok(mine.clone()));
        assert_eq!(acl_in(mine.clone(), host, host), Ok(mine));
        assert_eq!(acl_in(acl(&[(2, 0)]), own, own), Err(libc::EINVAL), "root, unmapped");
        assert_eq!(acl_in(acl(&[(8, 1001)]), own, own), Err(libc::EINVAL), "another group");
        assert_eq!(acl_in(acl(&[(2, 5), (8, 7)]), nested, host), Ok(acl(&[(2, 100_005), (8, 7)])));
        assert_eq!(acl_in(acl(&[(2, none)]), host, host), Err(libc::EINVAL), "-1 is no one's");
        assert_eq!(acl_in(acl(&[(1, 7)]), own, own), Ok(acl(&[(1, 7)])));
        let odd = vec![2, 0, 0, 0, 2, 0];
        assert_eq!(acl_in(odd.clone(), own, own), Ok(odd));
        let mut v1 = acl(&[(2, 0)]);
        v1[0] = 1;
        assert_eq!(acl_in(v1.clone(), own, own), Ok(v1));
    }
}
