//! # rpress-client
//!
//! Socket.IO client SDK for Rust — connect to Rpress or any Socket.IO v4+
//! server for real-time, event-driven communication.
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use rpress_client::SocketIoClient;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let mut client = SocketIoClient::connect("http://localhost:3000").await?;
//!
//!     client.on("chat message", |data| async move {
//!         println!("Received: {:?}", data);
//!     }).await;
//!
//!     client.emit("chat message", &serde_json::json!("Hello from Rust!")).await?;
//!     client.disconnect().await?;
//!     Ok(())
//! }
//! ```

pub(crate) mod engine_io;
pub(crate) mod socket_io;
pub(crate) mod transport;

mod client;

pub use client::SocketIoClient;
