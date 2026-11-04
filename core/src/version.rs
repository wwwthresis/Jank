//! Jank! release numbering.
//!
//! # Scheme
//!
//! ```text
//!   MAJOR . MINOR . PATCH [ - CHANNEL ] + BUILD
//!   0     . 1     . 0     - alpha       + 31026
//! ```
//!
//! * **MAJOR** changes only when a project written by the new version can no longer be opened by
//!   the previous major line, or when the workflow changes in a way that needs migration notes.
//! * **MINOR** adds features. Projects stay readable by every later version.
//! * **PATCH** contains fixes only.
//! * **CHANNEL** is `alpha` (feature set still moving), `beta` (feature-complete, bug fixing),
//!   `rc` (release candidate) or empty for a stable release.
//! * **BUILD** is a number that grows with every build produced by the release pipeline. It is
//!   the number printed on the splash screen and shown on the About page, and it is what a
//!   support request should quote.
//!
//! Ordering: `MAJOR`, `MINOR`, `PATCH`, then channel (`alpha < beta < rc < stable`), then `BUILD`.
//!
//! The project file format has its own, independent integer version ([`FORMAT_POLICY`] explains the
//! compatibility promise). A new application version does not necessarily change the file format.

use std::cmp::Ordering;
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Channel {
    Alpha,
    Beta,
    ReleaseCandidate,
    Stable,
}

impl Channel {
    pub fn as_str(self) -> &'static str {
        match self {
            Channel::Alpha => "alpha",
            Channel::Beta => "beta",
            Channel::ReleaseCandidate => "rc",
            Channel::Stable => "",
        }
    }
    fn parse(s: &str) -> Option<Channel> {
        match s {
            "alpha" => Some(Channel::Alpha),
            "beta" => Some(Channel::Beta),
            "rc" => Some(Channel::ReleaseCandidate),
            "" => Some(Channel::Stable),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AppVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub channel: Channel,
    pub build: u32,
}

impl AppVersion {
    /// Parses `MAJOR.MINOR.PATCH[-channel][+BUILD]`. A missing build number is read as 0.
    pub fn parse(s: &str) -> Result<AppVersion, String> {
        let s = s.trim();
        let (rest, build) = match s.split_once('+') {
            Some((r, b)) => (r, b.parse::<u32>().map_err(|_| format!("invalid build number '{b}'"))?),
            None => (s, 0),
        };
        let (core, channel) = match rest.split_once('-') {
            Some((c, ch)) => (c, Channel::parse(ch).ok_or_else(|| format!("unknown channel '{ch}' (expected alpha, beta or rc)"))?),
            None => (rest, Channel::Stable),
        };
        let mut it = core.split('.');
        let mut next = |what: &str| -> Result<u32, String> { it.next().ok_or_else(|| format!("missing {what} number"))?.parse::<u32>().map_err(|_| format!("invalid {what} number in '{core}'")) };
        let (major, minor, patch) = (next("major")?, next("minor")?, next("patch")?);
        if it.next().is_some() {
            return Err(format!("too many numeric parts in '{core}'"));
        }
        Ok(AppVersion { major, minor, patch, channel, build })
    }

    /// `0.1.0-alpha` (no build number): used in window titles and file metadata.
    pub fn short(&self) -> String {
        match self.channel {
            Channel::Stable => format!("{}.{}.{}", self.major, self.minor, self.patch),
            c => format!("{}.{}.{}-{}", self.major, self.minor, self.patch, c.as_str()),
        }
    }

    /// True if this build can be expected to open a project saved by `other` (same major line, not newer minor).
    pub fn reads_projects_from(&self, other: &AppVersion) -> bool {
        self.major == other.major && (self.minor, self.patch) >= (other.minor, other.patch)
    }
}

impl fmt::Display for AppVersion {
    /// Full form, e.g. `0.1.0-alpha+31026`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.short())?;
        if self.build != 0 {
            write!(f, "+{}", self.build)?;
        }
        Ok(())
    }
}

impl PartialOrd for AppVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for AppVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch, self.channel, self.build).cmp(&(other.major, other.minor, other.patch, other.channel, other.build))
    }
}

/// Compatibility promise of the `.jank` project format (version number stored in the file header).
///
/// * Every build opens every older format version (readers are never removed within a major line).
/// * A build refuses, with a clear message, to open a file whose format is newer than it knows,
///   instead of guessing and losing data.
/// * A format version is only increased when the layout of existing chunks changes. New optional
///   chunks do not change it; chunks a build does not know are skipped on load (they are not
///   written back, so saving with an older build drops data it cannot understand).
pub const FORMAT_POLICY: &str = "older files always open; newer files are refused with a message; unknown chunks are skipped";

/// Result of comparing a file's format version with what this build understands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatCheck {
    Ok,
    /// Opened fine but the file was written by an older format; saving will upgrade it.
    Older,
    /// The file comes from a newer Jank!; refuse to open.
    TooNew,
}

pub fn check_format(file: u32, supported: u32) -> FormatCheck {
    match file.cmp(&supported) {
        Ordering::Equal => FormatCheck::Ok,
        Ordering::Less => FormatCheck::Older,
        Ordering::Greater => FormatCheck::TooNew,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_prints_the_documented_forms() {
        let v = AppVersion::parse("0.1.0-alpha+31026").unwrap();
        assert_eq!((v.major, v.minor, v.patch, v.channel, v.build), (0, 1, 0, Channel::Alpha, 31026));
        assert_eq!(v.to_string(), "0.1.0-alpha+31026");
        assert_eq!(v.short(), "0.1.0-alpha");
        assert_eq!(AppVersion::parse("2.4.1").unwrap().to_string(), "2.4.1");
        assert_eq!(AppVersion::parse("1.0.0-rc+7").unwrap().to_string(), "1.0.0-rc+7");
    }
    #[test]
    fn rejects_garbage_with_a_helpful_message() {
        for bad in ["", "1", "1.2", "1.2.3.4", "a.b.c", "1.2.3-gamma", "1.2.3+x", "1.2.-3"] {
            assert!(AppVersion::parse(bad).is_err(), "'{bad}' must be rejected");
        }
        assert!(AppVersion::parse("1.2.3-gamma").unwrap_err().contains("gamma"));
    }
    #[test]
    fn ordering_follows_the_documented_rules() {
        let p = |s: &str| AppVersion::parse(s).unwrap();
        assert!(p("0.1.0-alpha+5") < p("0.1.0-alpha+6"));
        assert!(p("0.1.0-alpha+99") < p("0.1.0-beta+1"));
        assert!(p("0.1.0-beta") < p("0.1.0-rc"));
        assert!(p("0.1.0-rc+50") < p("0.1.0"));
        assert!(p("0.1.9") < p("0.2.0-alpha"));
        assert!(p("0.9.9") < p("1.0.0-alpha"));
    }
    #[test]
    fn project_reading_rule() {
        let p = |s: &str| AppVersion::parse(s).unwrap();
        assert!(p("1.3.0").reads_projects_from(&p("1.2.5")));
        assert!(p("1.3.0").reads_projects_from(&p("1.3.0")));
        assert!(!p("1.2.0").reads_projects_from(&p("1.3.0")), "an older build must not claim to read newer projects");
    }
    #[test]
    fn format_check() {
        assert_eq!(check_format(1, 1), FormatCheck::Ok);
        assert_eq!(check_format(1, 2), FormatCheck::Older);
        assert_eq!(check_format(3, 2), FormatCheck::TooNew);
    }
}
