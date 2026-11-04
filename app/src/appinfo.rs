//! Identity of this build: product name, version, build number, legal text.
//!
//! The version scheme itself (parsing, ordering, compatibility) lives in
//! `psxcore::version`; this module only supplies the values for the running binary.

use psxcore::version::{AppVersion, Channel};

pub const PRODUCT: &str = "Jank!";
pub const TAGLINE: &str = "A studio for low-poly and PSX-style models";
pub const COPYRIGHT: &str = "Copyright (c) the Jank! authors. All rights reserved.";

/// Release channel of this build. Change it together with `Cargo.toml` when a stage is promoted.
pub const CHANNEL: Channel = Channel::Alpha;

/// Build number, stamped by the release pipeline into `BUILD_NUMBER` (it also appears on the splash art).
const BUILD_TEXT: &str = include_str!("../BUILD_NUMBER");

/// Third-party licence report, produced by `tools/gen_licenses.py`.
pub const THIRD_PARTY_LICENSES: &str = include_str!("../assets/THIRD_PARTY_LICENSES.txt");

pub fn build_number() -> u32 {
    BUILD_TEXT.trim().parse().unwrap_or(0)
}

/// Version of the running application.
pub fn version() -> AppVersion {
    let mut parts = env!("CARGO_PKG_VERSION").split('.').map(|p| p.parse::<u32>().unwrap_or(0));
    AppVersion {
        major: parts.next().unwrap_or(0),
        minor: parts.next().unwrap_or(0),
        patch: parts.next().unwrap_or(0),
        channel: CHANNEL,
        build: build_number(),
    }
}

/// Full text, e.g. `0.2.0-alpha+31027`.
pub fn version_string() -> String {
    version().to_string()
}

/// Operating system and architecture, for support requests.
pub fn platform() -> String {
    format!("{} / {}", std::env::consts::OS, std::env::consts::ARCH)
}

/// Multi-line block a user can paste into a bug report.
pub fn support_report() -> String {
    format!(
        "{} {}\nproject file format: {}\nplatform: {}\n",
        PRODUCT,
        version_string(),
        psxcore::project::VERSION,
        platform()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_matches_cargo_and_build_file() {
        let v = version();
        assert_eq!(format!("{}.{}.{}", v.major, v.minor, v.patch), env!("CARGO_PKG_VERSION"));
        assert_eq!(v.build, build_number());
        assert!(v.build > 0, "BUILD_NUMBER must hold a positive integer");
    }

    #[test]
    fn version_string_round_trips() {
        let s = version_string();
        assert_eq!(AppVersion::parse(&s).unwrap(), version());
    }

    #[test]
    fn license_report_is_present_and_names_the_fonts() {
        assert!(THIRD_PARTY_LICENSES.contains("THIRD-PARTY LICENSES"));
        assert!(THIRD_PARTY_LICENSES.contains("SIL OPEN FONT LICENSE"));
        assert!(!THIRD_PARTY_LICENSES.contains("licence: GPL"));
    }
}
