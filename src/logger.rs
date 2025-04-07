use std::{
    borrow::Cow,
    collections::HashSet,
    env,
    future::Future,
    marker::PhantomData,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll},
};

use bytes::Bytes;
use futures_core::ready;
use pin_project_lite::pin_project;
use regex::Regex;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;

use actix_service::{Service, Transform};
use actix_utils::future::{Ready, ready};
#[cfg(feature = "tracing-request-id")]
use actix_web::HttpMessage;
use actix_web::body::{BodySize, MessageBody};
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::http::header::HeaderName;
use actix_web::{Error, Result};

/// Middleware for logging requests and responses summaries using slog.
///
/// This middleware uses the `slog` crate to output information.
///
/// # Default Format
/// The [`default`](SLogger::default)
///
/// # Examples
/// ```rust
/// use actix_web::App;
/// use actix_web_middleware_slogger::SLogger;
///
/// let app = App::new()
///     .wrap(SLogger::default());
/// ```
pub struct SLogger(Rc<Inner>);

#[derive(Debug, Clone)]
struct Inner {
    fields: ListFields,
    exclude: HashSet<String>,
    exclude_regex: Vec<Regex>,
    log_target: Cow<'static, str>,
}

impl SLogger {
    /// Create `SLogger` middleware with the specified `fields`.
    pub fn new(fields: Fields) -> SLogger {
        SLogger(Rc::new(Inner {
            fields: fields.into(),
            exclude: HashSet::new(),
            exclude_regex: Vec::new(),
            log_target: Cow::Borrowed(module_path!()),
        }))
    }

    /// Ignore and do not log access info for specified path.
    pub fn exclude<T: Into<String>>(mut self, path: T) -> Self {
        Rc::get_mut(&mut self.0)
            .unwrap()
            .exclude
            .insert(path.into());
        self
    }

    /// Ignore and do not log access info for paths that match regex.
    pub fn exclude_regex<T: Into<String>>(mut self, path: T) -> Self {
        let inner = Rc::get_mut(&mut self.0).unwrap();
        inner.exclude_regex.push(Regex::new(&path.into()).unwrap());
        self
    }

    /// Sets the logging target to `target`.
    ///
    /// By default, the log target is `module_path!()` of the log call location. In our case, that
    /// would be `actix_web_middleware_slogger::logger`.
    ///
    /// # Examples
    /// Using `.log_target("http_slog")` would have this effect on request logs:
    /// ```diff
    /// - [2015-10-21T07:28:00Z INFO  actix_web_middleware_slogger::logger] 127.0.0.1 "GET / HTTP/1.1" 200 88 "-" "dmc/1.0" 0.001985
    /// + [2015-10-21T07:28:00Z INFO  http_slog] 127.0.0.1 "GET / HTTP/1.1" 200 88 "-" "dmc/1.0" 0.001985
    ///                               ^^^^^^^^^
    /// ```
    pub fn log_target(mut self, target: impl Into<Cow<'static, str>>) -> Self {
        let inner = Rc::get_mut(&mut self.0).unwrap();
        inner.log_target = target.into();
        self
    }
}

impl Default for SLogger {
    /// Create `SLogger` middleware with format:
    ///
    /// Fields:
    /// - Method
    /// - Status
    /// - Path
    /// - Params
    /// - Host
    /// - RemoteAddr
    /// - Size
    /// - Duration
    /// - DateTime
    /// - UserAgent
    /// - Referer
    fn default() -> Self {
        SLogger(Rc::new(Inner {
            fields: Fields::default().into(),
            exclude: HashSet::new(),
            exclude_regex: Vec::new(),
            log_target: "actix_web_middleware_slogger::logger".into(),
        }))
    }
}

impl<S, B> Transform<S, ServiceRequest> for SLogger
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    B: MessageBody,
{
    type Response = ServiceResponse<StreamLog<B>>;
    type Error = Error;
    type Transform = SLoggerMiddlewareService<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(SLoggerMiddlewareService {
            service,
            inner: Rc::clone(&self.0),
        }))
    }
}

pin_project! {
    pub struct StreamLog<B> {
        #[pin]
        body: B,
        fields: Option<ListFields>,
        size: usize,
        time: OffsetDateTime,
        log_target: Cow<'static, str>,
    }

    impl<B> PinnedDrop for StreamLog<B> {
        fn drop(this: Pin<&mut Self>) {
            let this = this.project();
            if let Some(fields) = this.fields {
                for unit in &mut fields.0 {
                    unit.render(*this.size, *this.time)
                }

                #[cfg(feature = "log")]
                crate::wrapper::rust_log::log(
                    log::Level::Info,
                    this.log_target.as_ref(),
                    module_path!(),
                    std::panic::Location::caller(),
                    fields.0.clone(),
                );
            }
        }
    }
}

impl<B: MessageBody> MessageBody for StreamLog<B> {
    type Error = B::Error;

    #[inline]
    fn size(&self) -> BodySize {
        self.body.size()
    }

    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Bytes, Self::Error>>> {
        let this = self.project();

        match ready!(this.body.poll_next(cx)) {
            Some(Ok(chunk)) => {
                *this.size += chunk.len();
                Poll::Ready(Some(Ok(chunk)))
            }
            Some(Err(err)) => Poll::Ready(Some(Err(err))),
            None => Poll::Ready(None),
        }
    }
}

/// Logger middleware service.
pub struct SLoggerMiddlewareService<S> {
    inner: Rc<Inner>,
    service: S,
}

impl<S, B> Service<ServiceRequest> for SLoggerMiddlewareService<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    B: MessageBody,
{
    type Response = ServiceResponse<StreamLog<B>>;
    type Error = Error;
    type Future = SLoggerResponse<S, B>;

    actix_service::forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let excluded = self.inner.exclude.contains(req.path())
            || self
                .inner
                .exclude_regex
                .iter()
                .any(|r| r.is_match(req.path()));

        if excluded {
            SLoggerResponse {
                fut: self.service.call(req),
                fields: None,
                time: OffsetDateTime::now_utc(),
                log_target: Cow::Borrowed(""),
                _phantom: PhantomData,
            }
        } else {
            let now = OffsetDateTime::now_utc();
            let mut fields = self.inner.fields.clone();

            for unit in &mut fields.0 {
                unit.render_request(now, &req);
            }

            SLoggerResponse {
                fut: self.service.call(req),
                fields: Some(fields),
                time: now,
                log_target: self.inner.log_target.clone(),
                _phantom: PhantomData,
            }
        }
    }
}

pin_project! {
    pub struct SLoggerResponse<S, B>
    where
        B: MessageBody,
        S: Service<ServiceRequest>,
    {
        #[pin]
        fut: S::Future,
        time: OffsetDateTime,
        fields: Option<ListFields>,
        log_target: Cow<'static, str>,
        _phantom: PhantomData<B>,
    }
}

impl<S, B> Future for SLoggerResponse<S, B>
where
    B: MessageBody,
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
{
    type Output = Result<ServiceResponse<StreamLog<B>>, Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        let res = match ready!(this.fut.poll(cx)) {
            Ok(res) => res,
            Err(err) => return Poll::Ready(Err(err)),
        };

        if let Some(error) = res.response().error() {
            log::debug!("Error in response: {:?}", error);
        }

        let res = if let Some(fields) = this.fields {
            // to avoid polluting all the Logger types with the body parameter we swap the body
            // out temporarily since it's not usable in custom response functions anyway

            let (req, res) = res.into_parts();
            let (res, body) = res.into_parts();

            let temp_res = ServiceResponse::new(req, res.map_into_boxed_body());

            for unit in &mut fields.0 {
                unit.render_response(&temp_res);
            }

            // re-construct original service response
            let (req, res) = temp_res.into_parts();
            ServiceResponse::new(req, res.set_body(body))
        } else {
            res
        };

        let time = *this.time;
        let fields = this.fields.take();
        let log_target = this.log_target.clone();

        Poll::Ready(Ok(res.map_body(move |_, body| StreamLog {
            body,
            time,
            fields,
            size: 0,
            log_target,
        })))
    }
}

#[derive(Debug, Clone)]
struct ListFields(Vec<Field>);

impl From<Fields> for ListFields {
    fn from(fields: Fields) -> Self {
        ListFields(fields.0.into_iter().collect())
    }
}

#[derive(Debug, Clone)]
pub struct Fields(HashSet<Field>);

impl Default for Fields {
    fn default() -> Self {
        FieldsBuilder::default().build()
    }
}

impl Fields {
    pub fn builder() -> FieldsBuilder {
        FieldsBuilder::new()
    }

    pub fn new(fields: HashSet<Field>) -> Self {
        Fields(fields)
    }
}

pub struct FieldsBuilder {
    fields: HashSet<Field>,
}

impl FieldsBuilder {
    pub fn new() -> Self {
        FieldsBuilder {
            fields: HashSet::new(),
        }
    }

    pub fn build(self) -> Fields {
        Fields(self.fields)
    }

    pub fn with_method(mut self) -> Self {
        self.fields.insert(Field::Method);
        self
    }

    pub fn with_status(mut self) -> Self {
        self.fields.insert(Field::Status);
        self
    }

    pub fn with_path(mut self) -> Self {
        self.fields.insert(Field::Path);
        self
    }

    pub fn with_params(mut self) -> Self {
        self.fields.insert(Field::Params);
        self
    }

    pub fn with_version(mut self) -> Self {
        self.fields.insert(Field::Version);
        self
    }

    pub fn with_host(mut self) -> Self {
        self.fields.insert(Field::Host);
        self
    }

    pub fn with_remote_addr(mut self) -> Self {
        self.fields.insert(Field::RemoteAddr);
        self
    }

    pub fn with_real_ip(mut self) -> Self {
        self.fields.insert(Field::RealIp);
        self
    }

    pub fn with_request_id(mut self, header: &str) -> Self {
        self.fields
            .insert(Field::RequestId(HeaderName::try_from(header).unwrap()));
        self
    }

    #[cfg(feature = "tracing-request-id")]
    pub fn with_tracing_request_id(mut self) -> Self {
        self.fields.insert(Field::TracingRequestId);
        self
    }

    pub fn with_request_header(mut self, header: &str) -> Self {
        self.fields
            .insert(Field::RequestHeader(HeaderName::try_from(header).unwrap()));
        self
    }

    pub fn with_response_header(mut self, header: &str) -> Self {
        self.fields
            .insert(Field::ResponseHeader(HeaderName::try_from(header).unwrap()));
        self
    }

    pub fn with_size(mut self) -> Self {
        self.fields.insert(Field::Size);
        self
    }

    pub fn with_duration(mut self) -> Self {
        self.fields.insert(Field::Duration);
        self
    }

    pub fn with_duration_millis(mut self) -> Self {
        self.fields.insert(Field::DurationMillis);
        self
    }

    pub fn with_date_time(mut self) -> Self {
        self.fields.insert(Field::RequestTime);
        self
    }

    pub fn with_user_agent(mut self) -> Self {
        self.fields.insert(Field::UserAgent);
        self
    }

    pub fn with_referer(mut self) -> Self {
        self.fields.insert(Field::Referer);
        self
    }

    pub fn with_environment(mut self, var: &str) -> Self {
        self.fields.insert(Field::Environment(var.to_string()));
        self
    }
}

impl Default for FieldsBuilder {
    fn default() -> Self {
        FieldsBuilder::new()
            .with_method()
            .with_status()
            .with_path()
            .with_params()
            .with_version()
            .with_host()
            .with_remote_addr()
            .with_real_ip()
            .with_size()
            .with_duration()
            .with_date_time()
            .with_user_agent()
            .with_referer()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Field {
    /// Key, Value
    /// Used during result saving
    KV(String, Option<String>),
    /// Method. Example: GET
    Method,
    /// Status code. Example: 200, 404
    Status,
    /// Request path. Example: /index.html
    Path,
    /// Query string. Example: ?search=actix
    Params,
    /// Version of the HTTP protocol. Example: HTTP/1.1
    Version,
    /// Host. Example: localhost
    Host,
    /// Remote IP address. Example: 192.168.0.1
    RemoteAddr,
    /// Real IP address. Example: 192.168.0.1
    RealIp,
    /// Request ID. Example: 7b77f3f1-8e15-4b6a-9b3f-7f4b6f4b6f4b.
    /// Generated if not provided by the client.
    /// Used provided string to get the request ID from the request.
    RequestId(HeaderName),
    #[cfg(feature = "tracing-request-id")]
    /// Tracing request ID. Example: 7b77f3f1-8e15-4b6a-9b3f-7f4b6f4b6f4b.
    TracingRequestId,
    /// Request headers. Example: Accept: application/json
    RequestHeader(HeaderName),
    /// Response headers. Example: Content-Type: application/json
    ResponseHeader(HeaderName),
    /// Size of the response body in bytes. Example: 1024
    Size,
    /// Duration of the request in seconds. Example: 23
    Duration,
    /// Duration of the request in seconds with milliseconds. Example: 23.123
    DurationMillis,
    /// Timestamp in RFC3339 format. Example: 2019-05-29T18:51:00.000000Z
    RequestTime,
    /// User agent. Example: Mozilla/5.0
    UserAgent,
    /// Referer. Example: https://actix.rs
    Referer,
    /// Environment variable. Example: USER
    Environment(String),
}

#[derive(Clone, Copy, Debug)]
pub struct RequestId(Uuid);

impl RequestId {
    pub(crate) fn new() -> Self {
        #[cfg(not(feature = "uuid_v7"))]
        {
            Self(Uuid::new_v4())
        }
        #[cfg(feature = "uuid_v7")]
        {
            Self(Uuid::now_v7())
        }
    }
}

impl Field {
    fn render_request(&mut self, now: OffsetDateTime, req: &ServiceRequest) {
        match self {
            Field::Method => {
                *self = Field::KV("method".to_string(), Some(req.method().to_string()));
            }

            Field::Version => {
                let version = match req.version() {
                    actix_http::Version::HTTP_09 => "HTTP/0.9",
                    actix_http::Version::HTTP_10 => "HTTP/1.0",
                    actix_http::Version::HTTP_11 => "HTTP/1.1",
                    actix_http::Version::HTTP_2 => "HTTP/2.0",
                    actix_http::Version::HTTP_3 => "HTTP/3.0",
                    _ => "unknown",
                };
                *self = Field::KV("version".to_string(), Some(version.to_string()));
            }

            Field::Path => {
                *self = Field::KV("path".to_string(), Some(req.path().to_string()));
            }

            Field::Params => {
                *self = Field::KV("params".to_string(), Some(req.query_string().to_string()));
            }

            Field::Host => {
                *self = Field::KV(
                    "host".to_string(),
                    Some(req.connection_info().host().to_string()),
                );
            }

            Field::RemoteAddr => {
                *self = Field::KV(
                    "remote_addr".to_string(),
                    req.connection_info()
                        .peer_addr()
                        .map(|addr| addr.to_string()),
                );
            }

            Field::RealIp => {
                *self = Field::KV(
                    "real_ip".to_string(),
                    req.connection_info()
                        .realip_remote_addr()
                        .map(|addr| addr.to_string()),
                );
            }

            &mut Field::RequestId(ref header) => match req.headers().get(header) {
                Some(val) => {
                    *self = Field::KV(
                        header.to_string(),
                        Some(val.to_str().unwrap_or_default().to_string()),
                    );
                }
                None => {
                    let id = RequestId::new();
                    req.extensions_mut().insert(id);
                    *self = Field::KV(header.to_string(), Some(id.0.as_hyphenated().to_string()));
                }
            },

            #[cfg(feature = "tracing-request-id")]
            Field::TracingRequestId => {
                let ext = req.extensions();
                match ext.get::<tracing_actix_web::RequestId>() {
                    Some(id) => {
                        *self = Field::KV("tracing_request_id".to_string(), Some(id.to_string()));
                    }
                    None => {
                        *self = Field::KV("tracing_request_id".to_string(), None);
                    }
                }
            }

            &mut Field::RequestHeader(ref header) => {
                *self = match req.headers().get(header) {
                    Some(val) => Field::KV(
                        header.to_string(),
                        Some(val.to_str().unwrap_or_default().to_string()),
                    ),
                    None => Field::KV(header.to_string(), None),
                };
            }

            Field::RequestTime => {
                *self = Field::KV("datetime".to_string(), Some(now.format(&Rfc3339).unwrap()));
            }

            Field::UserAgent => {
                *self = Field::KV(
                    "user_agent".to_string(),
                    req.headers()
                        .get("user-agent")
                        .map(|v| v.to_str().unwrap_or_default().to_string()),
                );
            }

            Field::Referer => {
                *self = Field::KV(
                    "referer".to_string(),
                    req.headers()
                        .get("referer")
                        .map(|v| v.to_str().unwrap_or_default().to_string()),
                );
            }

            _ => {}
        }
    }

    pub fn render_response(&mut self, res: &ServiceResponse) {
        match self {
            Field::Status => {
                *self = Field::KV("status".to_string(), Some(res.status().to_string()));
            }

            Field::ResponseHeader(header) => {
                *self = match res.headers().get(header.as_str()) {
                    Some(val) => Field::KV(
                        header.to_string(),
                        Some(val.to_str().unwrap_or_default().to_string()),
                    ),
                    None => Field::KV(header.to_string(), None),
                };
            }

            _ => {}
        }
    }

    pub fn render(&mut self, size: usize, entry_time: OffsetDateTime) {
        match self {
            Field::Duration => {
                let rt = OffsetDateTime::now_utc() - entry_time;
                let rt = rt.as_seconds_f64();
                *self = Field::KV("duration".to_string(), Some(rt.to_string()));
            }

            Field::DurationMillis => {
                let rt = OffsetDateTime::now_utc() - entry_time;
                let rt = (rt.whole_nanoseconds() as f64) / 1_000_000.0;
                *self = Field::KV("duration".to_string(), Some(rt.to_string()));
            }

            Field::Size => {
                *self = Field::KV("size".to_string(), Some(size.to_string()));
            }

            Field::Environment(name) => {
                if let Ok(val) = env::var(name.as_str()) {
                    *self = Field::KV(name.to_string(), Some(val));
                } else {
                    *self = Field::KV(name.to_string(), None);
                }
            }

            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{
        HttpResponse,
        http::{Method, StatusCode, header},
        test::TestRequest,
    };

    #[test]
    fn test_slogger_builder() {
        // Test default configuration
        let logger = SLogger::default();
        assert_eq!(logger.0.log_target, "actix_web_middleware_slogger::logger");
        assert!(logger.0.exclude.is_empty());
        assert!(logger.0.exclude_regex.is_empty());

        // Test custom configuration
        let logger = SLogger::default()
            .exclude("/health")
            .exclude_regex("^/api/v1/.*")
            .log_target("custom_target");

        assert_eq!(logger.0.log_target, "custom_target");
        assert!(logger.0.exclude.contains("/health"));
        assert_eq!(logger.0.exclude_regex.len(), 1);
        assert!(logger.0.exclude_regex[0].is_match("/api/v1/users"));
        assert!(!logger.0.exclude_regex[0].is_match("/api/v2/users"));
    }

    #[test]
    fn test_fields_builder() {
        // Test default fields
        let fields = Fields::default();
        let field_set = &fields.0;

        assert!(field_set.contains(&Field::Method));
        assert!(field_set.contains(&Field::Status));
        assert!(field_set.contains(&Field::Path));
        assert!(field_set.contains(&Field::RemoteAddr));
        assert!(field_set.contains(&Field::Duration));

        // Test custom fields
        let custom_fields = Fields::builder()
            .with_method()
            .with_status()
            .with_request_header("content-type")
            .with_response_header("x-request-id")
            .with_environment("APP_ENV")
            .build();

        assert!(custom_fields.0.contains(&Field::Method));
        assert!(custom_fields.0.contains(&Field::Status));
        assert!(custom_fields.0.contains(&Field::RequestHeader(
            HeaderName::try_from("content-type").unwrap()
        )));
        assert!(custom_fields.0.contains(&Field::ResponseHeader(
            HeaderName::try_from("x-request-id").unwrap()
        )));
        assert!(
            custom_fields
                .0
                .contains(&Field::Environment("APP_ENV".to_string()))
        );
        assert!(!custom_fields.0.contains(&Field::Path));
    }

    #[test]
    fn test_field_render_request() {
        // Create test request
        let req = TestRequest::default()
            .method(Method::GET)
            .uri("/test?param=value")
            .insert_header(("user-agent", "test-agent"))
            .insert_header(("referer", "https://example.com"))
            .insert_header(("x-request-id", "test-id"))
            .to_http_request();

        let service_req = ServiceRequest::from_request(req);

        // Test Method field
        let mut field = Field::Method;
        field.render_request(OffsetDateTime::now_utc(), &service_req);
        if let Field::KV(key, value) = field {
            assert_eq!(key, "method");
            assert_eq!(value, Some("GET".to_string()));
        } else {
            panic!("Field should be KV");
        }

        // Test Path field
        let mut field = Field::Path;
        field.render_request(OffsetDateTime::now_utc(), &service_req);
        if let Field::KV(key, value) = field {
            assert_eq!(key, "path");
            assert_eq!(value, Some("/test".to_string()));
        } else {
            panic!("Field should be KV");
        }

        // Test Params field
        let mut field = Field::Params;
        field.render_request(OffsetDateTime::now_utc(), &service_req);
        if let Field::KV(key, value) = field {
            assert_eq!(key, "params");
            assert_eq!(value, Some("param=value".to_string()));
        } else {
            panic!("Field should be KV");
        }

        // Test UserAgent field
        let mut field = Field::UserAgent;
        field.render_request(OffsetDateTime::now_utc(), &service_req);
        if let Field::KV(key, value) = field {
            assert_eq!(key, "user_agent");
            assert_eq!(value, Some("test-agent".to_string()));
        } else {
            panic!("Field should be KV");
        }

        // Test Referer field
        let mut field = Field::Referer;
        field.render_request(OffsetDateTime::now_utc(), &service_req);
        if let Field::KV(key, value) = field {
            assert_eq!(key, "referer");
            assert_eq!(value, Some("https://example.com".to_string()));
        } else {
            panic!("Field should be KV");
        }

        // Test RequestHeader field
        let mut field = Field::RequestHeader(HeaderName::from_static("x-request-id"));
        field.render_request(OffsetDateTime::now_utc(), &service_req);
        if let Field::KV(key, value) = field {
            assert_eq!(key, "x-request-id");
            assert_eq!(value, Some("test-id".to_string()));
        } else {
            panic!("Field should be KV");
        }

        // Test RequestTime field
        let now = OffsetDateTime::now_utc();
        let mut field = Field::RequestTime;
        field.render_request(now, &service_req);
        if let Field::KV(key, value) = field {
            assert_eq!(key, "datetime");
            assert_eq!(value, Some(now.format(&Rfc3339).unwrap()));
        } else {
            panic!("Field should be KV");
        }
    }

    #[test]
    fn test_field_render_response() {
        // Create test request and response
        let req = TestRequest::default().to_http_request();
        // let service_req = ServiceRequest::from_request(req);

        let mut response = HttpResponse::build(StatusCode::OK);
        response.append_header((header::CONTENT_TYPE, "application/json"));
        response.append_header(("x-custom-header", "test-value"));

        // let service_resp = ServiceResponse::new(service_req, response.finish());
        let service_resp = ServiceResponse::new(req, response.finish());

        // Test Status field
        let mut field = Field::Status;
        field.render_response(&service_resp);
        if let Field::KV(key, value) = field {
            assert_eq!(key, "status");
            assert_eq!(value, Some("200 OK".to_string()));
        } else {
            panic!("Field should be KV");
        }

        // Test ResponseHeader field
        let mut field = Field::ResponseHeader(HeaderName::from_static("content-type"));
        field.render_response(&service_resp);
        if let Field::KV(key, value) = field {
            assert_eq!(key, "content-type");
            assert_eq!(value, Some("application/json".to_string()));
        } else {
            panic!("Field should be KV");
        }

        // Test custom ResponseHeader field
        let mut field = Field::ResponseHeader(HeaderName::from_static("x-custom-header"));
        field.render_response(&service_resp);
        if let Field::KV(key, value) = field {
            assert_eq!(key, "x-custom-header");
            assert_eq!(value, Some("test-value".to_string()));
        } else {
            panic!("Field should be KV");
        }

        // Test missing ResponseHeader field
        let mut field = Field::ResponseHeader(HeaderName::from_static("x-missing-header"));
        field.render_response(&service_resp);
        if let Field::KV(key, value) = field {
            assert_eq!(key, "x-missing-header");
            assert_eq!(value, None);
        } else {
            panic!("Field should be KV");
        }
    }

    #[test]
    fn test_field_render() {
        let entry_time = OffsetDateTime::now_utc() - time::Duration::seconds(2);

        // Test Size field
        let mut field = Field::Size;
        field.render(1024, entry_time);
        if let Field::KV(key, value) = field {
            assert_eq!(key, "size");
            assert_eq!(value, Some("1024".to_string()));
        } else {
            panic!("Field should be KV");
        }

        // Test Duration field
        let mut field = Field::Duration;
        field.render(0, entry_time);
        if let Field::KV(key, value) = field {
            assert_eq!(key, "duration");
            let duration: f64 = value.unwrap().parse().unwrap();
            assert!(duration >= 1.9 && duration <= 3.0); // Allow some margin for test execution time
        } else {
            panic!("Field should be KV");
        }

        // Test DurationMillis field
        let mut field = Field::DurationMillis;
        field.render(0, entry_time);
        if let Field::KV(key, value) = field {
            assert_eq!(key, "duration");
            let duration: f64 = value.unwrap().parse().unwrap();
            assert!(duration >= 1900.0 && duration <= 3000.0); // Allow some margin for test execution time
        } else {
            panic!("Field should be KV");
        }

        // Test Environment field (with env var set)
        unsafe {
            // Set the environment variable for testing
            std::env::set_var("TEST_ENV_VAR", "test_value");
        }
        let mut field = Field::Environment("TEST_ENV_VAR".to_string());
        field.render(0, entry_time);
        if let Field::KV(key, value) = field {
            assert_eq!(key, "TEST_ENV_VAR");
            assert_eq!(value, Some("test_value".to_string()));
        } else {
            panic!("Field should be KV");
        }

        // Test Environment field (with env var not set)
        let mut field = Field::Environment("MISSING_ENV_VAR".to_string());
        field.render(0, entry_time);
        if let Field::KV(key, value) = field {
            assert_eq!(key, "MISSING_ENV_VAR");
            assert_eq!(value, None);
        } else {
            panic!("Field should be KV");
        }
    }

    #[test]
    fn test_request_id_generation() {
        let id1 = RequestId::new();
        let id2 = RequestId::new();
        assert_ne!(id1.0, id2.0); // IDs should be unique
    }
}
