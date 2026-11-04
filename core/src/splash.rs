//! Startup splash selection.
//!
//! Pure logic with no GUI and no clock access: the application passes a month and a day,
//! so every rule below is covered by unit tests.
//!
//! The rule table [`RULES`] is the single source of truth. Image files in `app/assets/splash/`
//! carry the season id and the primary date range in their names (for example
//! `christmas_1224-1225.jpg`), and a test keeps the names and the table consistent.

/// A (month, day) pair, both 1-based.
pub type MonthDay = (u32, u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Season {
    /// Shown on every day that is not covered by a holiday rule.
    Default,
    Halloween,
    Christmas,
    NewYear,
    Valentine,
}

/// One holiday rule: the splash image and the inclusive date ranges when it is shown.
/// A range whose start is after its end wraps over the new year (e.g. Dec 31 – Jan 1).
pub struct Rule {
    pub season: Season,
    /// Short lowercase id, also the prefix of the image file name.
    pub id: &'static str,
    /// Image file names inside `app/assets/splash/`. Several files mean variants: one is picked per launch.
    pub files: &'static [&'static str],
    pub ranges: &'static [(MonthDay, MonthDay)],
}

/// All seasons. `Default` has no ranges: it is the fallback.
pub const RULES: &[Rule] = &[
    Rule { season: Season::Default, id: "default", files: &["default_a.jpg", "default_b.jpg"], ranges: &[] },
    Rule { season: Season::Halloween, id: "halloween", files: &["halloween_1030-1031.jpg"], ranges: &[((10, 30), (10, 31))] },
    // Western Christmas (Dec 24-25) and Eastern Christmas (Jan 6-7) share one picture.
    Rule { season: Season::Christmas, id: "christmas", files: &["christmas_1224-1225.jpg"], ranges: &[((12, 24), (12, 25)), ((1, 6), (1, 7))] },
    Rule { season: Season::NewYear, id: "newyear", files: &["newyear_1231-0101.jpg"], ranges: &[((12, 31), (1, 1))] },
    Rule { season: Season::Valentine, id: "valentine", files: &["valentine_0213-0214.jpg"], ranges: &[((2, 13), (2, 14))] },
];

impl Season {
    pub const ALL: [Season; 5] = [Season::Default, Season::Halloween, Season::Christmas, Season::NewYear, Season::Valentine];

    pub fn rule(self) -> &'static Rule {
        RULES.iter().find(|r| r.season == self).expect("every season has a rule")
    }
    pub fn id(self) -> &'static str {
        self.rule().id
    }
    /// The image file for this launch; `seed` (any number, e.g. the clock) chooses between variants.
    pub fn file_name(self, seed: u64) -> &'static str {
        let f = self.rule().files;
        f[(seed % f.len() as u64) as usize]
    }
    /// Parses a manual choice (`JANK_SPLASH` / `--splash`): the season id or its 1-based number.
    pub fn parse(s: &str) -> Option<Season> {
        let s = s.trim().to_lowercase();
        if let Ok(n) = s.parse::<usize>() {
            return n.checked_sub(1).and_then(|i| Season::ALL.get(i).copied());
        }
        let s = match s.as_str() {
            "xmas" => "christmas",
            "new-year" | "new_year" => "newyear",
            "valentines" => "valentine",
            other => other,
        };
        Season::ALL.into_iter().find(|x| x.id() == s)
    }
}

fn in_range(date: MonthDay, (start, end): (MonthDay, MonthDay)) -> bool {
    if start <= end {
        date >= start && date <= end
    } else {
        date >= start || date <= end
    }
}

/// The season for a calendar date (month 1-12, day 1-31).
pub fn season_for(month: u32, day: u32) -> Season {
    RULES.iter().find(|r| r.ranges.iter().any(|&rg| in_range((month, day), rg))).map_or(Season::Default, |r| r.season)
}

/// Parses `MM-DD`, `M/D` or `M.D` (used by `--date` / `JANK_DATE` to preview a season without changing the system clock).
pub fn parse_month_day(s: &str) -> Option<MonthDay> {
    let (m, d) = s.trim().split_once(['-', '.', '/'])?;
    let (m, d): (u32, u32) = (m.trim().parse().ok()?, d.trim().parse().ok()?);
    ((1..=12).contains(&m) && (1..=31).contains(&d)).then_some((m, d))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halloween_is_the_day_before_and_the_day() {
        assert_eq!(season_for(10, 29), Season::Default);
        assert_eq!(season_for(10, 30), Season::Halloween);
        assert_eq!(season_for(10, 31), Season::Halloween);
        assert_eq!(season_for(11, 1), Season::Default);
    }
    #[test]
    fn christmas_western_and_eastern() {
        for (m, d, s) in [(12, 23, Season::Default), (12, 24, Season::Christmas), (12, 25, Season::Christmas), (12, 26, Season::Default), (1, 5, Season::Default), (1, 6, Season::Christmas), (1, 7, Season::Christmas), (1, 8, Season::Default)] {
            assert_eq!(season_for(m, d), s, "{m}-{d}");
        }
    }
    #[test]
    fn new_year_wraps_over_the_year_boundary() {
        assert_eq!(season_for(12, 30), Season::Default);
        assert_eq!(season_for(12, 31), Season::NewYear);
        assert_eq!(season_for(1, 1), Season::NewYear);
        assert_eq!(season_for(1, 2), Season::Default);
    }
    #[test]
    fn valentine_is_the_day_before_and_the_day() {
        assert_eq!(season_for(2, 12), Season::Default);
        assert_eq!(season_for(2, 13), Season::Valentine);
        assert_eq!(season_for(2, 14), Season::Valentine);
        assert_eq!(season_for(2, 15), Season::Default);
    }
    #[test]
    fn ten_special_days_in_a_year_and_ranges_never_overlap() {
        let days = [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
        let mut special = 0;
        for (i, n) in days.iter().enumerate() {
            for d in 1..=*n {
                let hits = RULES.iter().filter(|r| r.ranges.iter().any(|&rg| in_range((i as u32 + 1, d), rg))).count();
                assert!(hits <= 1, "{}-{d} matches several rules", i + 1);
                special += hits;
            }
        }
        assert_eq!(special, 2 + 4 + 2 + 2, "halloween 2 + christmas 4 + new year 2 + valentine 2");
    }
    #[test]
    fn file_names_match_the_primary_range() {
        for r in RULES {
            if r.ranges.is_empty() {
                assert!(r.files.iter().all(|f| f.starts_with("default_") && f.ends_with(".jpg")));
                assert_eq!(r.files.len(), 2, "two default pictures");
                continue;
            }
            let ((m1, d1), (m2, d2)) = r.ranges[0];
            assert_eq!(r.files, &[format!("{}_{m1:02}{d1:02}-{m2:02}{d2:02}.jpg", r.id).as_str()], "file name of {} must encode its primary range", r.id);
        }
    }
    #[test]
    fn ids_and_files_are_unique() {
        for (i, a) in RULES.iter().enumerate() {
            for b in &RULES[i + 1..] {
                assert!(a.id != b.id && a.files.iter().all(|f| !b.files.contains(f)));
            }
        }
        assert_eq!(RULES.len(), Season::ALL.len());
    }
    #[test]
    fn variants_are_picked_by_seed() {
        assert_ne!(Season::Default.file_name(0), Season::Default.file_name(1));
        assert_eq!(Season::Default.file_name(0), Season::Default.file_name(2));
        assert_eq!(Season::Halloween.file_name(7), "halloween_1030-1031.jpg");
    }
    #[test]
    fn manual_override_parsing() {
        assert_eq!(Season::parse("1"), Some(Season::Default));
        assert_eq!(Season::parse("5"), Some(Season::Valentine));
        assert_eq!(Season::parse("0"), None);
        assert_eq!(Season::parse("6"), None);
        assert_eq!(Season::parse(" Halloween "), Some(Season::Halloween));
        assert_eq!(Season::parse("xmas"), Some(Season::Christmas));
        assert_eq!(Season::parse("new-year"), Some(Season::NewYear));
        assert_eq!(parse_month_day("12-24"), Some((12, 24)));
        assert_eq!(parse_month_day("2/14"), Some((2, 14)));
        assert_eq!(parse_month_day("13-01"), None);
        assert_eq!(parse_month_day("garbage"), None);
    }
}
