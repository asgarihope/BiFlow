//! Windows named-pipe ACL helpers for the privileged helper process.
//!
//! Isolated from workspace `unsafe_code = "forbid"` because Tokio and Win32
//! require an `unsafe` `SECURITY_ATTRIBUTES` pointer when creating a pipe that
//! Medium-integrity desktop clients can open.

/// SDDL allowing SYSTEM, Administrators, and local Users at Medium integrity.
///
/// `ME` (Medium) is required so a non-elevated desktop process can connect to a
/// pipe created by SYSTEM. Remote clients are rejected by the pipe mode flags.
pub const HELPER_PIPE_SDDL: &str = "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;BU)S:(ML;;NW;;;ME)";

#[cfg(windows)]
mod windows_impl;

#[cfg(windows)]
pub use windows_impl::create_helper_server;

#[cfg(test)]
mod tests {
    use super::HELPER_PIPE_SDDL;

    #[test]
    fn packaged_sddl_allows_users_at_medium_integrity() {
        assert!(HELPER_PIPE_SDDL.contains("BU"));
        assert!(HELPER_PIPE_SDDL.contains("ME"));
        assert!(HELPER_PIPE_SDDL.contains("SY"));
    }
}
