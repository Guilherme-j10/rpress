/// Configurable HTTP security headers injected automatically into every response.
///
/// Use the builder methods to declare which headers the framework should send.
/// Headers defined here will **not** override headers already set by a handler
/// via [`ResponsePayload::with_header`](crate::ResponsePayload::with_header),
/// giving per-route control when needed.
///
/// # Example
///
/// ```rust
/// use rpress::RpressSecurityHeaders;
///
/// let security = RpressSecurityHeaders::new()
///     .content_security_policy("default-src 'self'; script-src 'self'")
///     .x_frame_options("DENY")
///     .x_xss_protection("1; mode=block")
///     .custom("Permissions-Policy", "camera=(), microphone=()");
/// ```
pub struct RpressSecurityHeaders {
    headers: Vec<(String, String)>,
}

impl RpressSecurityHeaders {
    /// Creates an empty security headers configuration.
    pub fn new() -> Self {
        Self {
            headers: Vec::new(),
        }
    }

    /// Sets the `Content-Security-Policy` header value.
    ///
    /// Controls which resources the browser is allowed to load, mitigating XSS
    /// and data-injection attacks. Example values:
    /// - `"default-src 'self'"` — only allow resources from the same origin
    /// - `"default-src 'self'; script-src 'self' 'unsafe-inline'"` — also allow inline scripts
    pub fn content_security_policy(mut self, value: impl Into<String>) -> Self {
        self.headers
            .push(("Content-Security-Policy".into(), value.into()));
        self
    }

    /// Sets the `X-Frame-Options` header value.
    ///
    /// Prevents clickjacking by controlling whether the page can be embedded in
    /// `<iframe>`, `<frame>`, or `<object>`. Common values:
    /// - `"DENY"` — never allow framing
    /// - `"SAMEORIGIN"` — only allow framing from the same origin
    pub fn x_frame_options(mut self, value: impl Into<String>) -> Self {
        self.headers
            .push(("X-Frame-Options".into(), value.into()));
        self
    }

    /// Sets the `X-XSS-Protection` header value.
    ///
    /// Enables the browser's built-in XSS filter (legacy, but still useful for
    /// older browsers). Common value: `"1; mode=block"`.
    pub fn x_xss_protection(mut self, value: impl Into<String>) -> Self {
        self.headers
            .push(("X-XSS-Protection".into(), value.into()));
        self
    }

    /// Adds an arbitrary security header not covered by the named methods.
    ///
    /// Useful for headers like `Permissions-Policy`, `Referrer-Policy`,
    /// `Cross-Origin-Opener-Policy`, etc.
    pub fn custom(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((key.into(), value.into()));
        self
    }

    /// Returns a reference to the configured header pairs.
    pub(crate) fn headers(&self) -> &[(String, String)] {
        &self.headers
    }
}

impl Default for RpressSecurityHeaders {
    fn default() -> Self {
        Self::new()
    }
}
