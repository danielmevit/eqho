//! Integration tests for `--demo-dashboard` and `--demo-license` CLI parsing, isolation guard,
//! and demo state mapping.

use eqho_app::cli::{CliCommand, parse_args};
use eqho_app::license::{
    DemoLicense, LicenseState, STORE_FEATURE_ENABLED, demo_license_is_honoured, demo_license_state,
};
use jiff::Timestamp;

const NOW_UNIX_SECS: i64 = 1_800_000_000;

fn now_ts() -> Timestamp {
    Timestamp::new(NOW_UNIX_SECS, 0).expect("valid timestamp")
}

#[test]
fn parses_demo_license_separate_and_equals_forms() {
    let cases = [
        ("full", DemoLicense::Full),
        ("trial", DemoLicense::Trial),
        ("trial-last-day", DemoLicense::TrialLastDay),
        ("trial-ended", DemoLicense::TrialEnded),
        ("unknown", DemoLicense::Unknown),
        ("TRIAL-ENDED", DemoLicense::TrialEnded),
    ];

    for (raw, expected) in cases {
        let cmd1 = parse_args(["--demo-dashboard", "--demo-license", raw]).unwrap();
        assert_eq!(
            cmd1,
            CliCommand::Desktop {
                open_dashboard: true,
                tab: None,
                dll_dir: None,
                demo_dashboard: true,
                demo_license: Some(expected),
            }
        );

        let eq_arg = format!("--demo-license={raw}");
        let cmd2 = parse_args(["--demo-dashboard", eq_arg.as_str()]).unwrap();
        assert_eq!(cmd2, cmd1);
    }
}

#[test]
fn demo_license_alone_does_not_enable_demo_dashboard_isolation() {
    let cmd = parse_args(["--demo-license=trial-ended"]).unwrap();
    assert_eq!(
        cmd,
        CliCommand::Desktop {
            open_dashboard: false,
            tab: None,
            dll_dir: None,
            demo_dashboard: false,
            demo_license: Some(DemoLicense::TrialEnded),
        }
    );
    assert!(!demo_license_is_honoured(false));
}

#[test]
fn unknown_or_missing_demo_license_value_errors() {
    assert!(parse_args(["--demo-license"]).is_err());
    assert!(parse_args(["--demo-license="]).is_err());
    assert!(parse_args(["--demo-license=bogus"]).is_err());
    assert!(parse_args(["--demo-license", "expired"]).is_err());
}

#[test]
fn demo_license_is_honoured_only_in_isolated_store_builds() {
    assert!(!demo_license_is_honoured(false));
    assert_eq!(demo_license_is_honoured(true), STORE_FEATURE_ENABLED);
}

#[test]
fn demo_license_state_produces_expected_states_and_days_left() {
    let now = now_ts();

    assert_eq!(demo_license_state(DemoLicense::Full, now), LicenseState::Full);
    assert_eq!(
        demo_license_state(DemoLicense::Unknown, now),
        LicenseState::Unknown
    );
    assert_eq!(
        demo_license_state(DemoLicense::TrialEnded, now),
        LicenseState::TrialExpired
    );

    let trial = demo_license_state(DemoLicense::Trial, now);
    assert!(matches!(trial, LicenseState::Trial { .. }));
    assert_eq!(trial.trial_days_left(now), Some(5));
    assert!(trial.allows_dictation());

    let last_day = demo_license_state(DemoLicense::TrialLastDay, now);
    assert!(matches!(last_day, LicenseState::Trial { .. }));
    assert_eq!(last_day.trial_days_left(now), Some(1));
    assert!(last_day.allows_dictation());
}
