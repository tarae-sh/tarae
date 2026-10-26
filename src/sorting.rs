//! tarae's one sort is a crate of its own (`crates/tarae-sorting`), so tarae's crates share its one copy of std's
//! sort; `crate::sorting::…` stays its name here. What stays is its measure, which needs tarae's build.

pub use tarae_sorting::*;

#[cfg(test)]
mod tests {
    /// What `listing` adds to a walk (ci5: `outside_keys` sorts each folder before it queues it): the same folders
    /// listed whole, unsorted as `read_dir` gives them and through `listing` — `WALK_MAX`'s 20,000 entries in
    /// folders of 99 (`perf_a_walk_of_the_key_places`' scene), of 999 and one of 20,000; medians of 11. The
    /// difference is the sort and the names held until their folder is done.
    #[test]
    fn perf_a_listing_in_name_order() {
        if crate::test_results::unoptimized("sorting::tests::perf_a_listing_in_name_order") {
            return;
        }
        let t = crate::scratch::Scratch::new("perf-listing");
        let mut line = String::new();
        for (folders, each) in [(200usize, 99usize), (20, 999), (1, 20_000)] {
            let dirs: Vec<std::path::PathBuf> =
                (0..folders).map(|d| t.join(format!("{each}/d{d}"))).collect();
            for d in &dirs {
                std::fs::create_dir_all(d).unwrap();
                for f in 0..each {
                    // (names as a tree has them: not made in their order)
                    std::fs::write(d.join(format!("f{}.txt", (f * 7919) % each)), "").unwrap();
                }
            }
            let time = |sorted: bool| {
                let mut ms: Vec<f64> = (0..11)
                    .map(|_| {
                        let at = std::time::Instant::now();
                        for d in &dirs {
                            let rd = std::fs::read_dir(d).unwrap();
                            match sorted {
                                true => drop(std::hint::black_box(super::listing(rd, usize::MAX))),
                                false => drop(std::hint::black_box(rd.flatten().collect::<Vec<_>>())),
                            }
                        }
                        at.elapsed().as_secs_f64() * 1000.0
                    })
                    .collect();
                ms.sort_by(f64::total_cmp);
                ms[ms.len() / 2]
            };
            let (plain, sorted) = (time(false), time(true));
            line += &format!(
                "  {folders} x {each}: {plain:.2} ms as read_dir lists, {sorted:.2} ms in name order (+{:.2} ms, \
                 {:.3} µs an entry)\n",
                sorted - plain,
                (sorted - plain) * 1000.0 / (folders * each) as f64
            );
        }
        let said = format!("a listing in name order, 20,000 entries; medians of 11:\n{line}");
        let _ = std::io::Write::write_all(&mut std::io::stderr(), said.as_bytes());
    }
}
