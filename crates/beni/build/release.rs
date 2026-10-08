// The mruby release `beni-sys` publishes, turned into the cfgs that let
// this crate follow it, named as rb-sys names Ruby's.
//
// A build script is outside `cargo test`'s reach, so the read lives
// here and both `build.rs` and the library's test build include it.

/// The variable the release metadata reaches this build through.
const RELEASE_METADATA: &str = "DEP_MRUBY_RELEASE";

/// The releases a cfg compares against, from the supported floor up.
const COMPARABLE_RELEASES: [(u32, u32); 2] = [(4, 0), (4, 1)];

/// The `(major, minor)` release, from the metadata's `major.minor`
/// value. A build without the metadata, or with any other value, fails
/// loudly: behavior this crate selects by release is never guessed.
fn configured_release(metadata: Option<&str>) -> (u32, u32) {
    let release = metadata.and_then(|value| {
        let (major, minor) = value.split_once('.')?;
        Some((major.parse().ok()?, minor.parse().ok()?))
    });
    release.unwrap_or_else(|| {
        unpublished_metadata(
            RELEASE_METADATA,
            "the release its bindings declare as `major.minor`",
            metadata,
        )
    })
}

/// Whether a comparison holds for the configured release's ordering
/// against a comparable one.
type Holds = fn(std::cmp::Ordering) -> bool;

/// Each comparison a release cfg names, with when it holds.
const COMPARISONS: [(&str, Holds); 5] = [
    ("lt", std::cmp::Ordering::is_lt),
    ("lte", std::cmp::Ordering::is_le),
    ("eq", std::cmp::Ordering::is_eq),
    ("gte", std::cmp::Ordering::is_ge),
    ("gt", std::cmp::Ordering::is_gt),
];

/// The cfg naming comparison `op` against the comparable `release`:
/// `mruby_<op>_<major>_<minor>`.
fn release_cfg_name(op: &str, (major, minor): (u32, u32)) -> String {
    format!("mruby_{op}_{major}_{minor}")
}

/// Every cfg name a comparison against a comparable release can set.
fn release_cfg_names() -> Vec<String> {
    COMPARABLE_RELEASES
        .iter()
        .flat_map(|&compared| COMPARISONS.map(|(op, _)| release_cfg_name(op, compared)))
        .collect()
}

/// The cfg names `release` sets, one per comparison that holds.
fn release_cfgs(release: (u32, u32)) -> Vec<String> {
    COMPARABLE_RELEASES
        .iter()
        .flat_map(|&compared| {
            let ordering = release.cmp(&compared);
            COMPARISONS
                .into_iter()
                .filter(move |(_, holds)| holds(ordering))
                .map(move |(op, _)| release_cfg_name(op, compared))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{configured_release, release_cfg_names, release_cfgs};

    #[test]
    fn the_release_is_read_from_the_metadata() {
        assert_eq!(configured_release(Some("4.1")), (4, 1));
    }

    #[test]
    #[should_panic(expected = "DEP_MRUBY_RELEASE is unset")]
    fn a_build_without_the_release_metadata_stops() {
        configured_release(None);
    }

    #[test]
    #[should_panic(expected = "DEP_MRUBY_RELEASE is `4`")]
    fn a_release_without_a_minor_number_stops_the_build() {
        configured_release(Some("4"));
    }

    #[test]
    fn the_floor_release_sits_below_every_later_comparable_release() {
        let cfgs = release_cfgs((4, 0));
        for set in [
            "mruby_eq_4_0",
            "mruby_gte_4_0",
            "mruby_lte_4_0",
            "mruby_lt_4_1",
        ] {
            assert!(cfgs.iter().any(|cfg| cfg == set), "{set} is set");
        }
        for unset in ["mruby_gt_4_0", "mruby_gte_4_1", "mruby_eq_4_1"] {
            assert!(cfgs.iter().all(|cfg| cfg != unset), "{unset} is unset");
        }
    }

    #[test]
    fn a_release_past_the_table_compares_greater_than_all_of_it() {
        let cfgs = release_cfgs((5, 2));
        assert!(cfgs.iter().any(|cfg| cfg == "mruby_gt_4_1"));
        assert!(cfgs.iter().all(|cfg| !cfg.starts_with("mruby_lt_")));
    }

    #[test]
    fn every_set_cfg_is_a_declared_name() {
        let names = release_cfg_names();
        for cfg in release_cfgs((4, 1)) {
            assert!(names.contains(&cfg), "{cfg} is declared");
        }
    }
}
