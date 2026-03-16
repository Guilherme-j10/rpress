// db.rs — Simulated database pool.
//
// In a real application, replace DbPool with your actual connection pool, e.g.:
//   use sqlx::PgPool;
//   let db = Arc::new(PgPool::connect(&database_url).await?);
//
// The pattern is the same: wrap the pool in Arc, pass it to your route
// functions, and store it inside each controller.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct User {
    pub id: u32,
    pub name: String,
    pub email: String,
}

/// A thread-safe, in-memory user store that mimics the interface of a real
/// database pool (async methods, fallible results).
pub struct DbPool {
    users: Mutex<HashMap<u32, User>>,
    next_id: AtomicU32,
}

impl DbPool {
    /// Creates the pool and seeds it with a few mock users.
    pub fn new() -> Self {
        let mut users = HashMap::new();
        users.insert(
            1,
            User {
                id: 1,
                name: "Alice".to_string(),
                email: "alice@example.com".to_string(),
            },
        );
        users.insert(
            2,
            User {
                id: 2,
                name: "Bob".to_string(),
                email: "bob@example.com".to_string(),
            },
        );

        Self {
            users: Mutex::new(users),
            next_id: AtomicU32::new(3),
        }
    }

    /// Finds a single user by ID. Returns `None` if not found.
    pub async fn find_user(&self, id: u32) -> Option<User> {
        self.users.lock().unwrap().get(&id).cloned()
    }

    /// Returns all users, sorted by ID.
    pub async fn list_users(&self) -> Vec<User> {
        let map = self.users.lock().unwrap();
        let mut users: Vec<User> = map.values().cloned().collect();
        users.sort_by_key(|u| u.id);
        users
    }

    /// Inserts a new user and returns the created record.
    pub async fn create_user(&self, name: String, email: String) -> User {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let user = User { id, name, email };
        self.users.lock().unwrap().insert(id, user.clone());
        user
    }

    /// Deletes a user by ID. Returns `true` if the record existed.
    pub async fn delete_user(&self, id: u32) -> bool {
        self.users.lock().unwrap().remove(&id).is_some()
    }
}
