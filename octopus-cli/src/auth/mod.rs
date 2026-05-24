pub struct OAuthManager;

impl OAuthManager {
    pub fn new() -> Self {
        Self
    }

    /// Ensure OAuth tokens are fresh.
    ///
    /// When `force` is true, always refresh regardless of expiry.
    /// This is used after receiving a 401 to retry the operation.
    pub async fn ensure_fresh(&self, _force: bool) -> crate::exception::Result<()> {
        // TODO: implement real OAuth refresh flow
        // For now, stub: assume tokens are valid
        Ok(())
    }
}

impl Default for OAuthManager {
    fn default() -> Self {
        Self::new()
    }
}
