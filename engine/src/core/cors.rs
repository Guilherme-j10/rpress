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

    pub fn set_origins(mut self, origins: Vec<&str>) -> Self {
        self.allowed_origins = origins.into_iter().map(String::from).collect();
        self
    }

    pub fn set_methods(mut self, methods: Vec<&str>) -> Self {
        self.allowed_methods = methods.into_iter().map(String::from).collect();
        self
    }

    pub fn set_headers(mut self, headers: Vec<&str>) -> Self {
        self.allowed_headers = headers.into_iter().map(String::from).collect();
        self
    }

    pub fn set_expose_headers(mut self, headers: Vec<&str>) -> Self {
        self.expose_headers = headers.into_iter().map(String::from).collect();
        self
    }

    pub fn set_max_age(mut self, seconds: u64) -> Self {
        self.max_age = Some(seconds);
        self
    }

    pub fn set_credentials(mut self, allow: bool) -> Self {
        self.allow_credentials = allow;
        self
    }
}
