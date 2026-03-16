/// CORS configuration builder for the Rpress server.
pub struct RpressCors {
    pub(crate) allowed_origins: Vec<String>,
    pub(crate) allowed_methods: Vec<String>,
    pub(crate) allowed_headers: Vec<String>,
    pub(crate) expose_headers: Vec<String>,
    pub(crate) max_age: Option<u64>,
    pub(crate) allow_credentials: bool,
}

impl Default for RpressCors {
    fn default() -> Self {
        Self::new()
    }
}

impl RpressCors {
    /// Creates a new CORS configuration with permissive defaults (all origins, common methods/headers).
    pub fn new() -> Self {
        Self {
            allowed_origins: vec!["*".to_string()],
            allowed_methods: ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            allowed_headers: ["Content-Type", "Authorization"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            expose_headers: vec![],
            max_age: Some(86400),
            allow_credentials: false,
        }
    }

    /// Sets the allowed origins for CORS requests.
    pub fn set_origins(mut self, origins: Vec<&str>) -> Self {
        self.allowed_origins = origins.into_iter().map(String::from).collect();
        self
    }

    /// Sets the allowed HTTP methods for CORS requests.
    pub fn set_methods(mut self, methods: Vec<&str>) -> Self {
        self.allowed_methods = methods.into_iter().map(String::from).collect();
        self
    }

    /// Sets the allowed request headers for CORS.
    pub fn set_headers(mut self, headers: Vec<&str>) -> Self {
        self.allowed_headers = headers.into_iter().map(String::from).collect();
        self
    }

    /// Sets headers that the browser is allowed to access from the response.
    pub fn set_expose_headers(mut self, headers: Vec<&str>) -> Self {
        self.expose_headers = headers.into_iter().map(String::from).collect();
        self
    }

    /// Sets how long (in seconds) the browser should cache preflight results.
    pub fn set_max_age(mut self, seconds: u64) -> Self {
        self.max_age = Some(seconds);
        self
    }

    /// Controls whether credentials (cookies, auth headers) are allowed in CORS requests.
    pub fn set_credentials(mut self, allow: bool) -> Self {
        self.allow_credentials = allow;
        self
    }
}
