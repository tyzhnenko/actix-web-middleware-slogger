# actix-web-middleware-slogger

[![crates.io](https://img.shields.io/crates/v/actix-web-middleware-slogger.svg)](https://crates.io/crates/actix-web-middleware-slogger)
[![Documentation](https://docs.rs/actix-web-middleware-slogger/badge.svg)](https://docs.rs/actix-web-middleware-slogger)
[![MIT licensed](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)

A structured logging middleware for Actix Web, inspired by the `Logger` middleware from actix-web.

## Terms of use

By using this project or its source code, for any purpose and in any shape or form, you grant your **implicit agreement** to all the following statements:

- You **condemn Russia and its military aggression against Ukraine**
- You **recognize that Russia is an occupant that unlawfully invaded a sovereign state**
- You **support Ukraine's territorial integrity, including its claims over temporarily occupied territories of Crimea and Donbas**
- You **reject false narratives perpetuated by Russian state propaganda**

## Features

- 🔄 Structured logging of HTTP requests and responses
- ⚙️ Fields selection (method, path, duration, headers, etc.)
- 📋 Support for standard `log` crate integration
- 🔍 Request ID tracking (with UUID v4 or v7 support)
- 🔗 Integration with tracing ecosystem via `tracing-request-id` feature
- 🧩 Pattern-based path exclusion

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
actix-web-middleware-slogger = "0.1.0"
```

## Usage

### Basic Usage

```rust
use tokio;
use actix_web;
use actix_web::{web, App, HttpServer};
use actix_web_middleware_slogger::SLogger;
use structured_logger::{Builder, async_json::new_writer};

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Initialize your logger of choice
    Builder::new()
        .with_target_writer("*", new_writer(tokio::io::stdout()))
        .init();

    HttpServer::new(|| {
        App::new()
            .wrap(SLogger::default())
            .route("/", web::get().to(|| async { "Hello world!" }))
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}
```

### With Tracing Integration

Use the `tracing-request-id` feature to integrate with the tracing ecosystem:

```toml
[dependencies]
actix-web-middleware-slogger = { version = "0.1.0", features = ["tracing-request-id"] }
```

```rust
use tokio;
use actix_web;
use actix_web::{web, App, HttpServer};
use actix_web_middleware_slogger::SLogger;
use structured_logger::{Builder, async_json::new_writer};
use tracing_actix_web::TracingLogger;


#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Initialize your logger of choice
    Builder::new()
        .with_target_writer("*", new_writer(tokio::io::stdout()))
        .init();

    HttpServer::new(|| {
        App::new()
            .wrap(TracingLogger::default())
            .wrap(SLogger::default())
            .route("/", web::get().to(|| async { "Hello world!" }))
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}
```

## Configuration

### Custom Fields

You can customize which fields are included in your logs:

```rust
use actix_web_middleware_slogger::{SLogger, FieldsBuilder};

let logger = SLogger::new(
    Fields::builder()
        .with_method()                  // HTTP method (GET, POST, etc.)
        .with_path()                    // Request path
        .with_status()                  // Response status code
        .with_duration()                // Request duration in seconds
        .with_size()                    // Response size in bytes
        .with_remote_addr()             // Client IP address
        .with_request_id("request-id")  // Auto-generated request ID
        .build()
);
```

### Path Exclusions

Exclude specific paths from logging:

```rust
let logger = SLogger::default()
    .exclude("/health")
    .exclude("/metrics");
```

Or use regex patterns:

```rust
let logger = SLogger::default()
    .exclude_regex(r"^/assets/.*");
```

### Custom Log Target

Change the logger target name:

```rust
let logger = SLogger::default().log_target("api.access");
```

## Available Fields

The following fields can be added to your log output:

- `method` - HTTP method (GET, POST, etc.)
- `status` - Response status code
- `path` - Request path
- `params` - Query parameters
- `version` - HTTP protocol version
- `host` - Request host
- `remote_addr` - Client IP address
- `real_ip` - Client real IP (when behind proxy)
- `request_id` - Auto-generated or extracted request ID
- `size` - Response size in bytes
- `duration` - Request duration in seconds
- `duration_millis` - Request duration in milliseconds
- `datetime` - Timestamp in RFC3339 format
- `user_agent` - Client user agent
- `referer` - Request referrer

You can also log custom request headers, response headers, and environment variables.

## Feature Flags

- `log` (default) - Enable integration with the standard `log` crate
- `tracing-request-id` - Enable integration with `tracing-actix-web`'s request ID
- `uuid_v7` - Use UUIDv7 instead of UUIDv4 for request IDs

## License

This project is licensed under the MIT License - see [LICENSE](./LICENSE) file for details.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.