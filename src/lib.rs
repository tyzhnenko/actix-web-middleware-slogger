//! Actix-web middleware for structured access logs.
//! This middleware inspired by the `actix-web`'s `Logger` middleware.
//!
//! # Examples:
//! ## By default middleware uses the standard `log` crate for logging.
//! ```bash
//! crate add actix-web-middleware-slogger
//! ```
//! Example usage with standard `log` crate and `structured_logger` crate:
//! ```rust
//! use actix_web::{web, App, HttpServer, main};
//! use actix_web_middleware_slogger::SLogger;
//! use tokio;
//! use structured_logger::{Builder, async_json::new_writer, unix_ms};
//!
//! #[actix_web::main] // or #[tokio::main]
//! async fn main() -> std::io::Result<()> {
//!     Builder::new()
//!         .with_target_writer("*", new_writer(tokio::io::stdout()))
//!         .init();
//!
//!     HttpServer::new(|| {
//!         App::new()
//!             .wrap(SLogger::default())
//!             .route("/", web::get().to(|| async { "Hello world!" }))
//!     })
//!     .bind("127.0.0.1:8080")?;
//!     Ok(())
//! }
//! ```
//! ## `tracing-request-id` feature allows to log Request ID that set by `TracingLogger`.
//! ```bash
//! crate add actix-web-middleware-slogger --features tracing-request-id
//! ```
//! Example usage with `tracing-request-id` feature:
//! ```rust
//! use actix_web;
//! use actix_web::{web, App, HttpServer};
//! use actix_web_middleware_slogger::SLogger;
//! use tokio;
//! use structured_logger::{Builder, async_json::new_writer, unix_ms};
//! use tracing_actix_web::TracingLogger;
//!
//! #[actix_web::main] // or #[tokio::main]
//! async fn main() -> std::io::Result<()> {
//!     Builder::new()
//!         .with_target_writer("*", new_writer(tokio::io::stdout()))
//!         .init();
//!
//!     HttpServer::new(|| {
//!         App::new()
//!             .wrap(TracingLogger::default())
//!             .wrap(SLogger::default())
//!             .route("/", web::get().to(|| async { "Hello world!" }))
//!     })
//!     .bind("127.0.0.1:8080")?;
//!     Ok(())
//! }
//!```
//! # Features
//! - Structured logging of HTTP requests and responses
//! - Fields selection (method, path, duration, headers, etc.)
//! - Support for standard `log` crate integration
//! - Request ID tracking (with UUID v4 or v7 support)
//! - Integration with tracing ecosystem via `tracing-request-id` feature
//! - Pattern-based path exclusion
//!
//! # Configuration
//!
//! ## Custom Fields
//!
//! You can customize which fields are included in your logs:
//!
//! ```rust
//! use actix_web_middleware_slogger::{SLogger, Fields};
//!
//! let logger = SLogger::new(
//!     Fields::builder()
//!         .with_method()                  // HTTP method (GET, POST, etc.)
//!         .with_path()                    // Request path
//!         .with_status()                  // Response status code
//!         .with_duration()                // Request duration in seconds
//!         .with_size()                    // Response size in bytes
//!         .with_remote_addr()             // Client IP address
//!         .with_request_id("request-id")  // Auto-generated request ID
//!         .build()
//! );
//! ```
//! ## Path Exclusions
//!
//! Exclude specific paths from logging:
//!
//! ```rust
//! use actix_web_middleware_slogger::SLogger;
//!
//! let logger = SLogger::default()
//!     .exclude("/health")
//!     .exclude("/metrics");
//! ```
//!
//! use regex patterns:
//!
//! ```rust
//! use actix_web_middleware_slogger::SLogger;
//!
//! let logger = SLogger::default()
//!     .exclude_regex(r"^/assets/.*");
//! ```
//! # Available Fields
//!
//! The following fields can be added to your log output:
//!
//! - `method` - HTTP method (GET, POST, etc.)
//! - `status` - Response status code
//! - `path` - Request path
//! - `params` - Query parameters
//! - `version` - HTTP protocol version
//! - `host` - Request host
//! - `remote_addr` - Client IP address
//! - `real_ip` - Client real IP (when behind proxy)
//! - `request_id` - Auto-generated or extracted request ID
//! - `size` - Response size in bytes
//! - `duration` - Request duration in seconds
//! - `duration_millis` - Request duration in milliseconds
//! - `datetime` - Timestamp in RFC3339 format
//! - `user_agent` - Client user agent
//! - `referer` - Request referrer
//!
//! You can also log custom request headers, response headers, and environment variables.
//!
//! # Feature Flags
//!
//! - `log` (default) - Enable integration with the standard `log` crate
//! - `tracing-request-id` - Enable integration with `tracing-actix-web`'s request ID
//! - `uuid_v7` - Use UUIDv7 instead of UUIDv4 for request IDs

mod logger;
mod wrapper;

pub use crate::logger::RequestId;
pub use crate::logger::{Fields, SLogger};
pub use crate::wrapper::rust_log;
