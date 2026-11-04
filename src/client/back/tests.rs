use std::time::{Duration, Instant, SystemTime};

use super::*;

const S: Duration = Duration::from_secs(1);

/// A rule at the clocks' start, both clocks read from one `t` (no sleep) or the wall `slept` ahead.
struct Clocks {
    mono: Instant,
    wall: SystemTime,
}

impl Clocks {
    fn new() -> Clocks {
        Clocks { mono: Instant::now(), wall: SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000) }
    }

    /// Both clocks on by `d`, and the wall by `slept` more (a suspend, which the monotonic clock doesn't count).
    fn on(&mut self, d: Duration, slept: Duration) -> (Instant, SystemTime) {
        self.mono += d;
        self.wall += d + slept;
        (self.mono, self.wall)
    }
}

fn back(c: &Clocks) -> Back {
    Back::new("mac", c.mono, c.wall)
}

#[test]
fn a_wake_is_the_wall_clock_ahead_of_the_monotonic_one() {
    let mut c = Clocks::new();
    let mut b = back(&c);
    let (now, wall) = c.on(3600 * S, Duration::ZERO);
    assert!(!b.woke(now, wall), "an hour awake, both clocks on alike");
    let (now, wall) = c.on(S, WAKE_GAP - S);
    assert!(!b.woke(now, wall), "the wall a little ahead (a clock set by NTP) is no wake");
    let (now, wall) = c.on(S, 20 * 60 * S);
    assert!(b.woke(now, wall), "twenty minutes the monotonic clock didn't count: a sleep");
    let (now, wall) = c.on(S, Duration::ZERO);
    assert!(!b.woke(now, wall), "seen once: the next look starts from it");
    // (the wall set back is no wake)
    let (now, _) = c.on(S, Duration::ZERO);
    assert!(!b.woke(now, SystemTime::UNIX_EPOCH));
}

#[test]
fn a_ping_unanswered_cuts_the_link_and_one_answered_says_nothing() {
    let c = Clocks::new();
    let mut b = back(&c);
    let t = c.mono;
    assert_eq!(b.due(), None, "up and nothing out: no clock (idle sends nothing)");
    let n = b.ping(t).expect("a ping while up");
    assert_eq!(b.ping(t + S), None, "one out at a time");
    assert_eq!(b.due(), Some(t + PONG_WITHIN));
    assert_eq!(b.tick(t + PONG_WITHIN - S), None);
    b.pong(n + 1);
    assert_eq!(b.due(), Some(t + PONG_WITHIN), "another ping's answer isn't this one's");
    b.pong(n);
    assert_eq!((b.due(), b.tick(t + 10 * S), b.is_down()), (None, None, false), "answered: nothing said");
    // (no answer: the link cut, a try at once, and what's measured starts at the ping — the wake)
    let p = t + 20 * S;
    b.ping(p).unwrap();
    assert_eq!(b.tick(p + PONG_WITHIN), Some(Do::Cut));
    assert!(b.is_down() && !b.said(), "down, nothing said yet");
    assert_eq!(b.up(), Some(p), "down since the wake was seen");
}

#[test]
fn a_drop_comes_back_quietly_when_its_first_try_works() {
    let c = Clocks::new();
    let mut b = back(&c);
    let t = c.mono;
    assert_eq!(b.dropped(t), Some(Do::Try { wait: false }));
    assert_eq!(b.dropped(t), None, "down already");
    assert_eq!(b.ping(t), None, "no ping while down");
    assert_eq!(b.due(), Some(t + SAY_AFTER));
    assert_eq!(b.tick(t + SAY_AFTER - Duration::from_millis(1)), None, "nothing said within SAY_AFTER");
    assert!(!b.said(), "keys go nowhere, `q` too, until the line is said");
    assert_eq!(b.up(), Some(t));
    assert!(!b.is_down() && b.due().is_none());
}

#[test]
fn a_slow_try_says_the_line_once_and_a_failed_one_backs_off() {
    let c = Clocks::new();
    let mut b = back(&c);
    let t = c.mono;
    b.dropped(t);
    let Some(Do::Say(line)) = b.tick(t + SAY_AFTER) else { panic!("the line past SAY_AFTER") };
    assert_eq!(line.fit(200), " the link to mac dropped — reconnecting… · q quit");
    assert!(b.said());
    assert_eq!((b.due(), b.tick(t + 9 * S)), (None, None), "said once; the try's own answer next");
    // (failed: again after 1, 2, 4 … s, at most BACKOFF_MAX)
    let mut now = t + 10 * S;
    let mut waits = Vec::new();
    for _ in 0..8 {
        let Some(Do::Say(line)) = b.failed(now, "ssh mac: Connection refused", false) else { panic!() };
        let line = line.fit(200);
        assert!(
            line.starts_with(" the link to mac dropped: ssh mac: Connection refused — again in "),
            "{line}"
        );
        assert!(line.ends_with(" · r now · q quit"), "{line}");
        let next = b.due().unwrap();
        waits.push(next - now);
        assert_eq!(b.tick(next - Duration::from_millis(1)), None);
        assert_eq!(b.tick(next), Some(Do::Try { wait: false }));
        now = next + S;
    }
    assert_eq!(waits, [1, 2, 4, 8, 16, 30, 30, 30].map(|s| s * S));
}

/// A long why (the relay's, a socket's whole path) is cut to the room between the head and the keys: what to press
/// stays in sight, the relay's list after its first line never drawn.
#[test]
fn the_line_keeps_its_keys_and_cuts_its_why() {
    use unicode_width::UnicodeWidthStr;
    let c = Clocks::new();
    let mut b = back(&c);
    b.dropped(c.mono);
    let why = format!(
        "host 4242 on mac: {}/4242.attach: Connection refused\n    4243  ~/other",
        "/state".repeat(30)
    );
    let Some(Do::Say(line)) = b.failed(c.mono, &why, false) else { panic!() };
    let fit = line.fit(80);
    assert!(fit.width() <= 80 && fit.contains('…') && !fit.contains("4243"), "{fit}");
    assert!(
        fit.starts_with(" the link to mac dropped: host 4242")
            && fit.ends_with("— again in 1.0s · r now · q quit")
    );
    assert!(line.fit(400).contains("Connection refused — again"), "wide, the why whole");
}

#[test]
fn r_tries_now_and_a_wake_while_down_tries_at_once() {
    let mut c = Clocks::new();
    let mut b = back(&c);
    let t = c.mono;
    b.dropped(t);
    assert!(!b.now(t), "nothing said yet: `r` is no key of the client's");
    b.failed(t + S, "no host", false);
    for _ in 0..4 {
        b.tick(b.due().unwrap());
        b.failed(b.due().unwrap_or(t), "no host", false);
    }
    let next = b.due().unwrap();
    assert!(b.now(t + 2 * S), "`r` on the line");
    assert!(!b.now(t + 2 * S), "a try under way already");
    b.failed(t + 3 * S, "no host", false);
    // (the lid shut with the line up: on the wake the next try is now, not the backoff's)
    let (now, wall) = c.on(4 * S, 3600 * S);
    assert!(b.woke(now, wall));
    assert!(b.due().unwrap() <= now && next > now - 4 * S);
    assert_eq!(b.tick(now), Some(Do::Try { wait: false }));
}

#[test]
fn a_restart_waits_for_the_host_and_one_refused_is_never_tried_again() {
    let c = Clocks::new();
    let mut b = back(&c);
    let t = c.mono;
    assert_eq!(b.restarting(t), Some(Do::Try { wait: true }));
    let Some(Do::Say(line)) = b.tick(t + SAY_AFTER) else { panic!() };
    assert_eq!(line.fit(200), " the host on mac restarts — attaching again… · q quit");
    let Some(Do::Say(line)) = b.failed(t + 4 * S, "no Welcome within 10 s", false) else { panic!() };
    let line = line.fit(200);
    assert!(line.starts_with(" the host on mac restarted, but: no Welcome"), "{line}");
    // (the try after it asks the relay to wait no more: the host's socket was given its minute)
    assert_eq!(b.tick(b.due().unwrap()), Some(Do::Try { wait: false }));
    let said = "refused — the host speaks protocol 2 (tarae 0.1.0 1b2094f3+), this attach 1 (tarae 0.1.0)";
    let Some(Do::Say(line)) = b.failed(t + 9 * S, said, true) else { panic!() };
    assert_eq!(line.fit(200), format!(" {said} — r again · q quit"));
    assert_eq!(b.due(), None, "a Gone is never tried again by itself");
    let (now, wall) = (t + 3600 * S, c.wall + 7200 * S);
    assert!(b.woke(now, wall));
    assert_eq!((b.due(), b.tick(now)), (None, None), "not on a wake either");
    assert!(b.now(now), "only by `r`");
}
