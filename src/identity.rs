//! Rphotos identity is separate from the upstream viewer and its saved data.

pub const APP_ID: &str = "rphotos";
pub const DISPLAY_NAME: &str = "Rphotos";
pub const LOG_TARGET: &str = "rphotos";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_identity_matches_package_and_log_target() {
        assert_eq!(APP_ID, env!("CARGO_PKG_NAME"));
        assert_eq!(APP_ID, LOG_TARGET);
        assert_eq!(DISPLAY_NAME.to_lowercase(), APP_ID);
    }
}
