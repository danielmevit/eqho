//! Speech model recommendation based on hardware class and target language.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HardwareClass {
    Gpu,
    Cpu,
}

pub fn recommend_model(hw: HardwareClass, lang: &str) -> &'static str {
    match hw {
        HardwareClass::Gpu => {
            if lang == "en" {
                "distil-large-v3"
            } else {
                "large-v3-turbo"
            }
        }
        HardwareClass::Cpu => {
            if lang == "en" {
                "distil-small.en"
            } else {
                "small"
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn recommends_expected_models() {
        assert_eq!(recommend_model(HardwareClass::Gpu, "en"), "distil-large-v3");
        assert_eq!(recommend_model(HardwareClass::Gpu, "de"), "large-v3-turbo");
        assert_eq!(recommend_model(HardwareClass::Cpu, "en"), "distil-small.en");
        assert_eq!(recommend_model(HardwareClass::Cpu, "fr"), "small");
    }
}
