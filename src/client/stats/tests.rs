use std::time::{Duration, Instant};

use super::*;

const MS: Duration = Duration::from_millis(1);

#[test]
fn a_key_is_timed_to_the_first_frame_that_handled_it() {
    let t = Instant::now();
    let mut s = Stats::new(t);
    s.presented(0, 5000, t + 300 * MS);
    s.sent(1, t + 400 * MS);
    s.sent(2, t + 401 * MS);
    // (a frame drawn before either was handled answers neither)
    s.presented(0, 100, t + 402 * MS);
    // (one frame for both: each timed from its own send, its bytes shared)
    s.presented(2, 600, t + 404 * MS);
    s.sent(3, t + 500 * MS);
    s.presented(3, 300, t + 510 * MS);
    assert_eq!(s.keys, [4 * MS, 3 * MS, 10 * MS]);
    assert_eq!(
        s.said(),
        [
            "tarae attach --stats: the first frame 300.00 ms from the start (ssh's handshake in it)",
            "tarae attach --stats: 3 keys, each to its frame: median 4.00 ms, p90 10.00 ms, 300 B a key",
        ]
    );
}

#[test]
fn a_key_sent_as_the_link_went_down_is_let_go() {
    let t = Instant::now();
    let mut s = Stats::new(t);
    s.sent(1, t);
    s.down();
    // (the new link's first frame, whatever its input, answers nothing sent before)
    s.presented(5, 9000, t + 2000 * MS);
    s.back(Duration::from_micros(61_250));
    s.back(3100 * MS);
    assert!(s.keys.is_empty());
    assert_eq!(
        s.said()[1..],
        [
            "tarae attach --stats: no key answered",
            "tarae attach --stats: back 2 times, the link down to a frame presented: 61.25 ms, 3100.00 ms",
        ]
    );
}

#[test]
fn the_median_and_p90_are_nearest_rank() {
    let v: Vec<Duration> = (1..=10).map(|i| i * MS).collect();
    assert_eq!((rank(&v, 0.5), rank(&v, 0.9)), (5 * MS, 9 * MS));
    assert_eq!((rank(&[7 * MS], 0.5), rank(&[7 * MS], 0.9)), (7 * MS, 7 * MS));
}
