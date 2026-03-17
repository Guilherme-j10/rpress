use std::collections::HashMap;
use std::sync::Arc;

use crate::{
    core::handler_response::IntoRpressResult,
    types::definitions::{Handler, Middleware, Next, RequestPayload, RpressResult},
};

struct DynamicParam {
    name: String,
    route: Route,
}

pub(crate) type ArcHandler = Arc<Handler>;

pub(crate) enum RouteMatch {
    Found(ArcHandler, HashMap<String, String>, Option<usize>),
    WrongMethod,
    NotFound,
}

#[derive(Default)]
pub(crate) struct Route {
    static_path: HashMap<String, Route>,
    dynamic_params: Option<Box<DynamicParam>>,
    handlers: HashMap<String, (ArcHandler, Option<usize>)>,
}

impl Route {
    pub(crate) fn new() -> Self {
        Route::default()
    }

    pub(crate) fn find(&self, path: &str, method: &str) -> RouteMatch {
        let segments: Vec<&str> = path
            .split("/")
            .filter(|s| !s.is_empty() && *s != "." && *s != "..")
            .collect();
        let mut params = HashMap::new();

        match self.match_recursive(&segments, &mut params) {
            Some(node) if node.handlers.contains_key(method) => {
                let (handler, body_limit) = node.handlers.get(method).unwrap();
                RouteMatch::Found(handler.clone(), params, *body_limit)
            }
            Some(_) => RouteMatch::WrongMethod,
            None => RouteMatch::NotFound,
        }
    }

    pub(crate) fn insert_route(&mut self, path: &str, method: &str, handler: Handler, max_body_size: Option<usize>) {
        let segments: Vec<&str> = path.split("/").filter(|s| !s.is_empty()).collect();
        self.recursive_insert(&segments, method, Arc::new(handler), max_body_size);
    }

    fn match_recursive<'a>(
        &'a self,
        segments: &[&str],
        params: &mut HashMap<String, String>,
    ) -> Option<&'a Route> {
        if segments.is_empty() {
            if self.handlers.is_empty() {
                return None;
            }
            return Some(self);
        }

        let current_segment = segments[0];
        let remaining = &segments[1..];

        if let Some(child_node) = self.static_path.get(current_segment)
            && let Some(result) = child_node.match_recursive(remaining, params)
        {
            return Some(result);
        }

        if let Some(ref dynamic_param) = self.dynamic_params {
            params.insert(dynamic_param.name.clone(), current_segment.to_string());
            return dynamic_param.route.match_recursive(remaining, params);
        }

        None
    }

    fn recursive_insert(
        &mut self,
        segments: &[&str],
        method: &str,
        handler: ArcHandler,
        max_body_size: Option<usize>,
    ) {
        if segments.is_empty() {
            self.handlers.insert(method.to_string(), (handler, max_body_size));
            return;
        }

        let current_segment = segments[0];
        let remaining = &segments[1..];

        if let Some(stripped) = current_segment.strip_prefix(':') {
            let param_name = stripped.to_string();

            if self.dynamic_params.is_none() {
                self.dynamic_params = Some(Box::new(DynamicParam {
                    name: param_name,
                    route: Route::new(),
                }));
            }

            if let Some(ref mut dp) = self.dynamic_params {
                dp.route.recursive_insert(remaining, method, handler, max_body_size);
            }
        } else {
            let next_node = self
                .static_path
                .entry(current_segment.to_string())
                .or_default();

            next_node.recursive_insert(remaining, method, handler, max_body_size);
        }
    }
}

/// A route group that bundles related routes and optional group-level middleware.
///
/// # Example
///
/// ```ignore
/// let mut routes = RpressRoutes::new();
/// routes.use_middleware(|req, next| async move { next(req).await });
/// routes.add(":get/hello", |_req: RequestPayload| async { ResponsePayload::text("hi") });
/// app.add_route_group(routes);
/// ```
#[derive(Default)]
pub struct RpressRoutes {
    pub(crate) routes: HashMap<String, Option<Handler>>,
    pub(crate) middlewares: Vec<Middleware>,
    pub(crate) max_body_size: Option<usize>,
}

impl RpressRoutes {
    /// Creates an empty route group.
    pub fn new() -> Self {
        Self {
            routes: HashMap::default(),
            middlewares: Vec::new(),
            max_body_size: None,
        }
    }

    /// Sets the maximum request body size for all routes in this group.
    /// Overrides the global limit set on `Rpress`.
    pub fn set_max_body_size(&mut self, bytes: usize) {
        self.max_body_size = Some(bytes);
    }

    /// Registers a middleware that applies only to routes in this group.
    pub fn use_middleware<F, Fut>(&mut self, middleware: F)
    where
        F: Fn(RequestPayload, Next) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = RpressResult> + Send + 'static,
    {
        self.middlewares
            .push(Arc::new(move |req, next| Box::pin(middleware(req, next))));
    }

    /// Adds a route handler. The name format is `:method/path` (e.g. `:get/users/:id`).
    pub fn add<T, F, Fut, R>(&mut self, name: T, handler: F)
    where
        T: Into<String>,
        R: IntoRpressResult + 'static,
        F: Fn(RequestPayload) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = R> + Send + 'static,
    {
        self.routes.insert(
            name.into(),
            Some(Box::new(move |req| {
                let fut = handler(req);

                Box::pin(async move { fut.await.into_result() })
            })),
        );
    }
}
