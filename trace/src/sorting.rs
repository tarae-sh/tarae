//! One sort, as tarae's own (`src/sorting.rs`, clippy.toml): every `sort_by(closure)` is a fresh copy of the
//! standard library's sort; these sort indices through `dyn`, so there is one copy, and put the items in that
//! order. Stable, like `sort_by`. (tarae-trace is outside tarae's workspace, so it can't take tarae's.)

use std::cmp::Ordering;

/// `v.sort_by(cmp)`, without a copy of the sort for it.
pub fn by<T>(v: &mut [T], mut cmp: impl FnMut(&T, &T) -> Ordering) {
    let mut order: Vec<usize> = (0..v.len()).collect();
    sort_indices(&mut order, &mut |a, b| cmp(&v[a], &v[b]));
    permute(v, &mut order);
}

/// `v.sort_by_key(key)`, without a copy of the sort for it.
pub fn by_key<T, K: Ord>(v: &mut [T], mut key: impl FnMut(&T) -> K) {
    by(v, |a, b| key(a).cmp(&key(b)))
}

#[inline(never)]
fn sort_indices(order: &mut [usize], cmp: &mut dyn FnMut(usize, usize) -> Ordering) {
    #[expect(clippy::disallowed_methods, reason = "the one copy of std's sort everything here goes through")]
    order.sort_by(|&a, &b| cmp(a, b));
}

/// Puts `v` in `order` (`v[i]` becomes the old `v[order[i]]`), following each cycle once.
fn permute<T>(v: &mut [T], order: &mut [usize]) {
    for i in 0..v.len() {
        let mut j = i;
        loop {
            let k = order[j];
            order[j] = j;
            if k == i {
                break;
            }
            v.swap(j, k);
            j = k;
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[expect(clippy::disallowed_methods, reason = "the standard sort is what it's checked against")]
    fn sorts_as_the_standard_sort_does_stable_included() {
        let mut seed = 7u64;
        for n in [0, 1, 2, 3, 10, 100, 1000] {
            let v: Vec<(u8, usize)> = (0..n)
                .map(|i| {
                    seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                    ((seed >> 59) as u8, i)
                })
                .collect();
            let mut want = v.clone();
            want.sort_by_key(|p| p.0);
            let mut got = v.clone();
            super::by_key(&mut got, |p| p.0);
            assert_eq!(got, want, "n = {n}");
            let mut got = v.clone();
            super::by(&mut got, |a, b| b.0.cmp(&a.0));
            want.sort_by_key(|a| std::cmp::Reverse(a.0));
            assert_eq!(got, want, "n = {n}, reversed");
        }
    }
}
