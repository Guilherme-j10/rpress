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
    Found(ArcHandler, HashMap<String, String>),
    WrongMethod,
    NotFound,
}

#[derive(Default)]
pub(crate) struct Route {
    static_path: HashMap<String, Route>,
    dynamic_params: Option<Box<DynamicParam>>,
    handlers: HashMap<String, ArcHandler>,
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
                RouteMatch::Found(node.handlers.get(method).unwrap().clone(), params)
            }
            Some(_) => RouteMatch::WrongMethod,
            None => RouteMatch::NotFound,
        }
    }

    pub(crate) fn insert_route(&mut self, path: &str, method: &str, handler: Handler) {
        let segments: Vec<&str> = path.split("/").filter(|s| !s.is_empty()).collect();
        self.recursive_insert(&segments, method, Arc::new(handler));
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

        if let Some(child_node) = self.static_path.get(current_segment) {
            if let Some(result) = child_node.match_recursive(remaining, params) {
                return Some(result);
            }
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
    ) {
        if segments.is_empty() {
            self.handlers.insert(method.to_string(), handler);
            return;
        }

        let current_segment = segments[0];
        let remaining = &segments[1..];

        if current_segment.starts_with(":") {
            let param_name = current_segment[1..].to_string();

            if self.dynamic_params.is_none() {
                self.dynamic_params = Some(Box::new(DynamicParam {
                    name: param_name,
                    route: Route::new(),
                }));
            }

            if let Some(ref mut dp) = self.dynamic_params {
                dp.route.recursive_insert(remaining, method, handler);
            }
        } else {
            let next_node = self
                .static_path
                .entry(current_segment.to_string())
                .or_insert_with(Route::new);

            next_node.recursive_insert(remaining, method, handler);
        }
    }
}

#[derive(Default)]
pub struct RpressRoutes {
    pub(crate) routes: HashMap<String, Option<Handler>>,
    pub(crate) middlewares: Vec<Middleware>,
}

impl RpressRoutes {
    pub fn new() -> Self {
        Self {
            routes: HashMap::default(),
            middlewares: Vec::new(),
        }
    }

    pub fn use_middleware<F, Fut>(&mut self, middleware: F)
    where
        F: Fn(RequestPayload, Next) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = RpressResult> + Send + 'static,
    {
        self.middlewares
            .push(Arc::new(move |req, next| Box::pin(middleware(req, next))));
    }

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
