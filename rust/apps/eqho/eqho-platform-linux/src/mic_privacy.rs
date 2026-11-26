//! Linux microphone privacy probe and desktop sound settings launcher.

use std::process::{Command, Stdio};

use eqho_platform::MicPrivacyProbe;

/// Linux sound settings launcher (Linux has no global OS mic privacy kill-switch).
#[derive(Debug, Default, Clone, Copy)]
pub struct LinuxMicPrivacyProbe;

impl MicPrivacyProbe for LinuxMicPrivacyProbe {
    fn mic_access_blocked(&self) -> Option<bool> {
        None
    }

    fn open_mic_privacy_settings(&self) -> bool {
        let candidates: &[(&str, &[&str])] = &[
            ("gnome-control-center", &["sound"]),
            ("systemsettings", &["kcm_pulseaudio"]),
            ("pavucontrol", &[]),
        ];
        for (bin, args) in candidates {
            if Command::new(bin)
                .args(*args)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .is_ok()
            {
                return true;
            }
        }
        false
    }
}
