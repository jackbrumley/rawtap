//! RawTap's app ID (Release and Update Standard section 10): the menu
//! launcher's file name and the ID in system info. Chosen once; never change
//! it after release.

pub const APP_ID: &str = "info.brumley.rawtap";

#[cfg(test)]
mod tests {
    use super::APP_ID;
    use std::path::Path;

    fn launcher() -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("packaging")
            .join(format!("{APP_ID}.desktop"));
        std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{} must exist: {error}", path.display()))
    }

    #[test]
    fn launcher_is_named_after_the_app_id_and_runs_in_a_terminal() {
        let launcher = launcher();
        assert!(launcher.contains("\nExec=rawtap\n"));
        assert!(launcher.contains("\nTerminal=true\n"));
        assert!(launcher.contains("\nIcon=rawtap\n"));
    }

    #[test]
    fn app_id_follows_the_id_rules() {
        assert!(!APP_ID.ends_with(".desktop") && !APP_ID.ends_with(".app"));
        for segment in APP_ID.split('.') {
            assert!(!segment.is_empty());
            assert!(
                segment
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()),
                "segment '{segment}' must be lowercase letters and digits"
            );
        }
    }
}
