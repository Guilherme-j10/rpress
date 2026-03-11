use std::collections::HashMap;

use crate::types::definitions::Handler;

struct DynamicParam {
    name: String,
    route: Route,
}

#[derive(Default)]
pub struct Route {
    static_path: HashMap<String, Route>,
    dynamic_params: Option<Box<DynamicParam>>,
    pub method: Option<String>,
    pub handler: Option<Handler>,
}

impl Route {
    pub fn new() -> Self {
        Route::default()
    }

    pub fn find(&self, path: &str) -> Option<(&Handler, &String, HashMap<String, String>)> {
        let segments: Vec<&str> = path.split("/").filter(|s| !s.is_empty()).collect();
        let mut params = HashMap::new();

        self.match_recursive(&segments, &mut params)
            .map(|result| return (result.0.unwrap(), result.1.unwrap(), params))
    }

    pub fn insert_route(&mut self, path: &str, method: &str, handler: Handler) -> () {
        let segments: Vec<&str> = path.split("/").filter(|s| !s.is_empty()).collect();
        self.recursive_insert(&segments, method, handler);
    }

    fn match_recursive<'a>(
        &'a self,
        segments: &[&str],
        params: &mut HashMap<String, String>,
    ) -> Option<(Option<&'a Handler>, Option<&'a String>)> {
        if segments.is_empty() {
            if self.handler.is_none() && self.method.is_none() {
                return None;
            } else {
                return Some((self.handler.as_ref(), self.method.as_ref()));
            }
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

    fn recursive_insert(&mut self, segments: &[&str], method: &str, handler: Handler) -> () {
        if segments.is_empty() {
            self.handler = Some(handler);
            self.method = Some(method.to_string());
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
                }))
            }

            self.dynamic_params
                .as_mut()
                .unwrap()
                .route
                .recursive_insert(remaining, method, handler);
        } else {
            let next_node = self
                .static_path
                .entry(current_segment.to_string())
                .or_insert_with(Route::new);

            next_node.recursive_insert(remaining, method, handler);
        }
    }
}
