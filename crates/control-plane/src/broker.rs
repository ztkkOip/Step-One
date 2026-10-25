//! Host-only model proxy core. No HTTP listener, guest authentication fallback,
//! OAuth flow, desktop credential form or agent process is created here.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use uuid::Uuid;
use zeroize::Zeroizing;

pub const MAX_REQUEST_BYTES: usize = 1_048_576;
pub const MAX_RESPONSE_BYTES: usize = 8_388_608;
const MAX_SECRET_BYTES: usize = 16_384;
pub type BrokerResult<T> = std::result::Result<T, BrokerError>;

/// Codes and messages are safe constants. Raw transport/credential errors must
/// never be propagated to the caller, audit trail or UI.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BrokerError {
    pub code: String,
    pub message: String,
}
impl BrokerError {
    pub fn new(code: &str, message: &str) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}
impl std::fmt::Display for BrokerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for BrokerError {}

/// The only secret-bearing type exposed to a trusted store/transport. There is
/// deliberately no Serialize, Deserialize, Clone or unredacted Debug impl.
pub struct Secret(Zeroizing<String>);
impl Secret {
    pub fn new(value: String) -> BrokerResult<Self> {
        let value = Zeroizing::new(value);
        if value.is_empty() || value.len() > MAX_SECRET_BYTES || value.chars().any(char::is_control)
        {
            return Err(BrokerError::new("INVALID_CREDENTIAL", "凭据格式无效。"));
        }
        Ok(Self(value))
    }
    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}
impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret([REDACTED])")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CredentialId(String);
impl CredentialId {
    pub fn new(value: &str) -> BrokerResult<Self> {
        let parsed = Uuid::parse_str(value).map_err(|_| invalid_credential_id())?;
        if parsed.is_nil() || parsed.hyphenated().to_string() != value {
            return Err(invalid_credential_id());
        }
        Ok(Self(value.into()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
fn invalid_credential_id() -> BrokerError {
    BrokerError::new("INVALID_CREDENTIAL_ID", "凭据引用无效。")
}

pub trait CredentialStore: Send + Sync {
    fn resolve(&self, id: &CredentialId) -> BrokerResult<Secret>;
}

/// URI construction and authentication scheme are fixed in the host binary.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub enum ModelEndpoint {
    OpenAiResponses,
    AnthropicMessages,
    DeepseekChat,
}
impl ModelEndpoint {
    pub fn url(self) -> &'static str {
        match self {
            Self::OpenAiResponses => "https://api.openai.com/v1/responses",
            Self::AnthropicMessages => "https://api.anthropic.com/v1/messages",
            Self::DeepseekChat => "https://api.deepseek.com/chat/completions",
        }
    }
}

/// A host-side opaque session handle, intentionally not deserializable. A future
/// authenticated guest channel must associate its peer with an existing handle;
/// accepting an untrusted request's session-id string is not authentication.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SessionId(Uuid);
impl std::fmt::Debug for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SessionId([HOST-BOUND])")
    }
}

pub trait SessionAuthorizer: Send + Sync {
    /// Called immediately before resolving credentials and again before send.
    /// Implementations must only accept host-established, unrevoked bindings.
    fn authorize(
        &self,
        session: &SessionId,
        endpoint: ModelEndpoint,
        model: &str,
    ) -> BrokerResult<CredentialId>;
}

#[derive(Debug, Clone)]
pub struct ModelScope {
    pub credential_id: CredentialId,
    pub models: BTreeSet<String>,
}

/// Host management API, not an IPC/guest API. Empty state denies every request.
#[derive(Default)]
pub struct HostSessions {
    bindings: Mutex<BTreeMap<SessionId, BTreeMap<ModelEndpoint, ModelScope>>>,
}
impl HostSessions {
    pub fn bind(&self, scopes: BTreeMap<ModelEndpoint, ModelScope>) -> BrokerResult<SessionId> {
        if scopes.is_empty()
            || scopes.values().any(|scope| {
                scope.models.is_empty() || scope.models.iter().any(|model| !valid_model(model))
            })
        {
            return Err(BrokerError::new(
                "INVALID_SCOPE",
                "会话必须指定模型服务和允许的模型。",
            ));
        }
        let id = SessionId(Uuid::new_v4());
        self.bindings
            .lock()
            .map_err(|_| denied())?
            .insert(id.clone(), scopes);
        Ok(id)
    }
    pub fn revoke(&self, session: &SessionId) -> BrokerResult<()> {
        self.bindings.lock().map_err(|_| denied())?.remove(session);
        Ok(())
    }
}
impl SessionAuthorizer for HostSessions {
    fn authorize(
        &self,
        session: &SessionId,
        endpoint: ModelEndpoint,
        model: &str,
    ) -> BrokerResult<CredentialId> {
        self.bindings
            .lock()
            .map_err(|_| denied())?
            .get(session)
            .and_then(|bindings| bindings.get(&endpoint))
            .filter(|scope| scope.models.contains(model))
            .map(|scope| scope.credential_id.clone())
            .ok_or_else(denied)
    }
}
fn denied() -> BrokerError {
    BrokerError::new("SESSION_DENIED", "此会话未获模型访问授权，或授权已撤销。")
}

/// This request is constructed only by ModelBroker. The transport is trusted
/// host code and must not serialize/debug it into IPC, logs, files, or argv.
pub struct HostRequest {
    endpoint: ModelEndpoint,
    body: Vec<u8>,
    authorization: Secret,
}
impl HostRequest {
    pub fn endpoint(&self) -> ModelEndpoint {
        self.endpoint
    }
    pub fn url(&self) -> &'static str {
        self.endpoint.url()
    }
    pub fn body(&self) -> &[u8] {
        &self.body
    }
    pub fn headers(&self) -> Vec<(&'static str, &str)> {
        let mut headers = vec![("content-type", "application/json")];
        match self.endpoint {
            ModelEndpoint::AnthropicMessages => {
                headers.push(("x-api-key", self.authorization.expose_secret()));
                headers.push(("anthropic-version", "2023-06-01"));
            }
            _ => headers.push(("authorization", self.authorization.expose_secret())),
        }
        headers
    }
}
impl std::fmt::Debug for HostRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostRequest")
            .field("endpoint", &self.endpoint)
            .field("body", &"[REDACTED]")
            .field("authorization", &"[REDACTED]")
            .finish()
    }
}

pub struct TransportResponse {
    pub status: u16,
    pub body: Vec<u8>,
    pub redirected: bool,
}
impl std::fmt::Debug for TransportResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TransportResponse")
            .field("status", &self.status)
            .field("body", &"[REDACTED]")
            .field("redirected", &self.redirected)
            .finish()
    }
}

/// Implementations must use TLS with certificate verification, prohibit HTTP
/// redirects and ambient proxy settings, and bound reads to MAX_RESPONSE_BYTES
/// before allocating. No streaming fake-completion or caller-selected URL.
pub trait TrustedTransport: Send + Sync {
    fn send(&self, request: &HostRequest) -> BrokerResult<TransportResponse>;
}

pub struct ModelBroker {
    credentials: Arc<dyn CredentialStore>,
    authorizer: Arc<dyn SessionAuthorizer>,
    transport: Arc<dyn TrustedTransport>,
}
impl ModelBroker {
    pub fn new(
        credentials: Arc<dyn CredentialStore>,
        authorizer: Arc<dyn SessionAuthorizer>,
        transport: Arc<dyn TrustedTransport>,
    ) -> Self {
        Self {
            credentials,
            authorizer,
            transport,
        }
    }

    pub fn send(
        &self,
        session: &SessionId,
        endpoint: ModelEndpoint,
        body: &[u8],
    ) -> BrokerResult<Vec<u8>> {
        let (model, body) = validate_body(endpoint, body)?;
        let credential_id = self
            .authorizer
            .authorize(session, endpoint, &model)
            .map_err(|_| denied())?;
        let secret = self.credentials.resolve(&credential_id).map_err(|_| {
            BrokerError::new(
                "CREDENTIAL_UNAVAILABLE",
                "宿主机凭据不可用，请在宿主机重新配置。",
            )
        })?;
        // Revocation during vault I/O must prevent dispatch. Requests already sent
        // cannot be recalled; session revocation prevents subsequent dispatches.
        let current = self
            .authorizer
            .authorize(session, endpoint, &model)
            .map_err(|_| denied())?;
        if current != credential_id {
            return Err(denied());
        }
        let authorization = if endpoint == ModelEndpoint::AnthropicMessages {
            Secret::new(secret.expose_secret().into())?
        } else {
            Secret::new(format!("Bearer {}", secret.expose_secret()))?
        };
        let request = HostRequest {
            endpoint,
            body,
            authorization,
        };
        let response = self.transport.send(&request).map_err(|_| {
            BrokerError::new("UPSTREAM_UNAVAILABLE", "模型服务请求失败，请稍后重试。")
        })?;
        if response.redirected || (300..400).contains(&response.status) {
            return Err(BrokerError::new(
                "REDIRECT_DENIED",
                "模型服务重定向已阻止。",
            ));
        }
        if !(200..300).contains(&response.status) {
            return Err(BrokerError::new(
                "UPSTREAM_REJECTED",
                "模型服务拒绝了请求。",
            ));
        }
        if response.body.len() > MAX_RESPONSE_BYTES {
            return Err(BrokerError::new(
                "RESPONSE_TOO_LARGE",
                "模型服务响应超出限制。",
            ));
        }
        let value: Value = serde_json::from_slice(&response.body)
            .map_err(|_| BrokerError::new("INVALID_RESPONSE", "模型服务返回了不支持的响应。"))?;
        // A compromised or misconfigured upstream must not echo the injected
        // secret into the guest response, including escaped JSON string forms.
        if contains_bytes(&response.body, secret.expose_secret().as_bytes())
            || contains_secret(&value, secret.expose_secret())
        {
            return Err(BrokerError::new(
                "CREDENTIAL_LEAK_BLOCKED",
                "响应包含受保护内容，已阻止返回。",
            ));
        }
        Ok(response.body)
    }
}

fn valid_model(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.:/".contains(&byte))
}
fn validate_body(endpoint: ModelEndpoint, body: &[u8]) -> BrokerResult<(String, Vec<u8>)> {
    let invalid = || BrokerError::new("INVALID_REQUEST", "模型请求格式无效或包含不支持的字段。");
    if body.is_empty() || body.len() > MAX_REQUEST_BYTES {
        return Err(invalid());
    }
    let value: Value = serde_json::from_slice(body).map_err(|_| invalid())?;
    let object = value.as_object().ok_or_else(invalid)?;
    let allowed: &[&str] = match endpoint {
        ModelEndpoint::OpenAiResponses => &[
            "model",
            "input",
            "instructions",
            "max_output_tokens",
            "temperature",
            "top_p",
            "tools",
            "tool_choice",
            "parallel_tool_calls",
            "text",
            "reasoning",
            "stream",
            "store",
        ],
        ModelEndpoint::AnthropicMessages => &[
            "model",
            "messages",
            "max_tokens",
            "system",
            "temperature",
            "top_p",
            "top_k",
            "tools",
            "tool_choice",
            "stop_sequences",
            "thinking",
            "stream",
        ],
        ModelEndpoint::DeepseekChat => &[
            "model",
            "messages",
            "max_tokens",
            "temperature",
            "top_p",
            "tools",
            "tool_choice",
            "response_format",
            "stop",
            "frequency_penalty",
            "presence_penalty",
            "stream",
        ],
    };
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(invalid());
    }
    if let Some(stream) = object.get("stream") {
        if stream != &Value::Bool(false) {
            return Err(BrokerError::new(
                "STREAMING_UNAVAILABLE",
                "流式响应通道尚未实现，请使用非流式请求。",
            ));
        }
    }
    let model = object
        .get("model")
        .and_then(Value::as_str)
        .filter(|value| valid_model(value))
        .ok_or_else(invalid)?;
    let input = if endpoint == ModelEndpoint::OpenAiResponses {
        "input"
    } else {
        "messages"
    };
    if !object.contains_key(input) {
        return Err(invalid());
    }
    // Remote tool servers would introduce a second, unreviewed credential and
    // network boundary. Only ordinary model function tools are accepted here.
    if let Some(tools) = object.get("tools") {
        let tools = tools.as_array().ok_or_else(invalid)?;
        for tool in tools {
            let tool = tool.as_object().ok_or_else(invalid)?;
            let kind = tool.get("type").and_then(Value::as_str);
            if endpoint == ModelEndpoint::AnthropicMessages {
                if kind.is_some_and(|kind| kind != "custom") {
                    return Err(invalid());
                }
            } else if kind != Some("function") {
                return Err(invalid());
            }
            if tool.contains_key("url")
                || tool.contains_key("server_url")
                || tool.contains_key("headers")
            {
                return Err(invalid());
            }
        }
    }
    let encoded = serde_json::to_vec(&value).map_err(|_| invalid())?;
    Ok((model.into(), encoded))
}
fn contains_bytes(body: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && body.windows(needle.len()).any(|part| part == needle)
}
fn contains_secret(value: &Value, secret: &str) -> bool {
    match value {
        Value::String(value) => value.contains(secret),
        Value::Array(values) => values.iter().any(|value| contains_secret(value, secret)),
        Value::Object(values) => values
            .iter()
            .any(|(key, value)| key.contains(secret) || contains_secret(value, secret)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const API_KEY: &str = "host-private-api-key";
    fn credential_id() -> CredentialId {
        CredentialId::new("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee").unwrap()
    }
    fn scopes(endpoint: ModelEndpoint) -> BTreeMap<ModelEndpoint, ModelScope> {
        [(
            endpoint,
            ModelScope {
                credential_id: credential_id(),
                models: ["test-model".into()].into(),
            },
        )]
        .into()
    }
    fn body(endpoint: ModelEndpoint) -> Vec<u8> {
        if endpoint == ModelEndpoint::OpenAiResponses {
            br#"{"model":"test-model","input":"hello"}"#.to_vec()
        } else {
            br#"{"model":"test-model","messages":[]}"#.to_vec()
        }
    }
    struct FakeStore {
        calls: AtomicUsize,
        fail: bool,
    }
    impl Default for FakeStore {
        fn default() -> Self {
            Self {
                calls: AtomicUsize::new(0),
                fail: false,
            }
        }
    }
    impl CredentialStore for FakeStore {
        fn resolve(&self, _: &CredentialId) -> BrokerResult<Secret> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.fail {
                Err(BrokerError::new(API_KEY, API_KEY))
            } else {
                Secret::new(API_KEY.into())
            }
        }
    }
    struct FakeTransport {
        calls: AtomicUsize,
        expected_endpoint: ModelEndpoint,
        response: Mutex<Option<BrokerResult<TransportResponse>>>,
    }
    impl FakeTransport {
        fn new(endpoint: ModelEndpoint, response: BrokerResult<TransportResponse>) -> Self {
            Self {
                calls: AtomicUsize::new(0),
                expected_endpoint: endpoint,
                response: Mutex::new(Some(response)),
            }
        }
    }
    impl TrustedTransport for FakeTransport {
        fn send(&self, request: &HostRequest) -> BrokerResult<TransportResponse> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            assert_eq!(request.endpoint(), self.expected_endpoint);
            assert_eq!(request.url(), self.expected_endpoint.url());
            assert!(!format!("{request:?}").contains(API_KEY));
            let headers: BTreeMap<_, _> = request.headers().into_iter().collect();
            assert_eq!(headers["content-type"], "application/json");
            if request.endpoint() == ModelEndpoint::AnthropicMessages {
                assert_eq!(headers["x-api-key"], API_KEY);
                assert_eq!(headers["anthropic-version"], "2023-06-01");
                assert!(!headers.contains_key("authorization"));
            } else {
                assert_eq!(headers["authorization"], format!("Bearer {API_KEY}"));
                assert!(!headers.contains_key("x-api-key"));
            }
            assert!(!contains_bytes(request.body(), API_KEY.as_bytes()));
            self.response
                .lock()
                .unwrap()
                .take()
                .expect("unexpected transport call")
        }
    }
    fn response(status: u16, text: &str) -> BrokerResult<TransportResponse> {
        Ok(TransportResponse {
            status,
            body: text.as_bytes().to_vec(),
            redirected: false,
        })
    }

    #[test]
    fn secret_and_identifier_validation_never_expose_values() {
        let secret = Secret::new(API_KEY.into()).unwrap();
        assert_eq!(secret.expose_secret(), API_KEY);
        assert!(!format!("{secret:?}").contains(API_KEY));
        for invalid in [
            "".into(),
            "line\r\nbreak".into(),
            "nul\0".into(),
            "x".repeat(MAX_SECRET_BYTES + 1),
        ] {
            assert!(Secret::new(invalid).is_err());
        }
        for invalid in [
            "",
            "API_KEY",
            "00000000-0000-0000-0000-000000000000",
            "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE",
        ] {
            assert_eq!(
                CredentialId::new(invalid).unwrap_err().code,
                "INVALID_CREDENTIAL_ID"
            );
        }
        assert_eq!(
            credential_id().as_str(),
            "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"
        );
    }

    #[test]
    fn injects_credentials_only_into_fixed_host_transport_headers() {
        for endpoint in [
            ModelEndpoint::OpenAiResponses,
            ModelEndpoint::AnthropicMessages,
            ModelEndpoint::DeepseekChat,
        ] {
            let sessions = Arc::new(HostSessions::default());
            let session = sessions.bind(scopes(endpoint)).unwrap();
            let store = Arc::new(FakeStore::default());
            let transport = Arc::new(FakeTransport::new(
                endpoint,
                response(200, r#"{"output":"hello"}"#),
            ));
            let broker = ModelBroker::new(store.clone(), sessions, transport.clone());
            assert_eq!(
                broker.send(&session, endpoint, &body(endpoint)).unwrap(),
                br#"{"output":"hello"}"#
            );
            assert_eq!(store.calls.load(Ordering::SeqCst), 1);
            assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
        }
    }

    #[test]
    fn denies_unbound_revoked_wrong_endpoint_or_model_before_secret_lookup() {
        let sessions = Arc::new(HostSessions::default());
        let store = Arc::new(FakeStore::default());
        let transport = Arc::new(FakeTransport::new(
            ModelEndpoint::OpenAiResponses,
            response(200, "{}"),
        ));
        let broker = ModelBroker::new(store.clone(), sessions.clone(), transport.clone());
        let unknown = SessionId(Uuid::new_v4());
        assert_eq!(
            broker
                .send(
                    &unknown,
                    ModelEndpoint::OpenAiResponses,
                    &body(ModelEndpoint::OpenAiResponses)
                )
                .unwrap_err()
                .code,
            "SESSION_DENIED"
        );
        let session = sessions
            .bind(scopes(ModelEndpoint::OpenAiResponses))
            .unwrap();
        assert_eq!(
            broker
                .send(
                    &session,
                    ModelEndpoint::AnthropicMessages,
                    &body(ModelEndpoint::AnthropicMessages)
                )
                .unwrap_err()
                .code,
            "SESSION_DENIED"
        );
        assert_eq!(
            broker
                .send(
                    &session,
                    ModelEndpoint::OpenAiResponses,
                    br#"{"model":"other","input":"hello"}"#
                )
                .unwrap_err()
                .code,
            "SESSION_DENIED"
        );
        sessions.revoke(&session).unwrap();
        assert_eq!(
            broker
                .send(
                    &session,
                    ModelEndpoint::OpenAiResponses,
                    &body(ModelEndpoint::OpenAiResponses)
                )
                .unwrap_err()
                .code,
            "SESSION_DENIED"
        );
        assert_eq!(store.calls.load(Ordering::SeqCst), 0);
        assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
        assert!(sessions.bind(BTreeMap::new()).is_err());
        let mut invalid = scopes(ModelEndpoint::OpenAiResponses);
        invalid.values_mut().next().unwrap().models.clear();
        assert!(sessions.bind(invalid).is_err());
    }

    #[test]
    fn invalid_requests_and_overrides_do_not_touch_credentials() {
        let sessions = Arc::new(HostSessions::default());
        let session = sessions
            .bind(scopes(ModelEndpoint::OpenAiResponses))
            .unwrap();
        let store = Arc::new(FakeStore::default());
        let broker = ModelBroker::new(
            store.clone(),
            sessions,
            Arc::new(FakeTransport::new(
                ModelEndpoint::OpenAiResponses,
                response(200, "{}"),
            )),
        );
        for invalid in [b"".as_slice(), b"{", b"[]", br#"{"input":"hello"}"#, br#"{"model":"test-model","url":"https://evil","input":"hello"}"#,
            br#"{"model":"test-model","headers":{"authorization":"guest"},"input":"hello"}"#,
            br#"{"model":"test-model","input":"hello","tools":[{"type":"mcp","server_url":"https://evil"}]}"#] {
            assert_eq!(broker.send(&session, ModelEndpoint::OpenAiResponses, invalid).unwrap_err().code, "INVALID_REQUEST");
        }
        assert_eq!(
            broker
                .send(
                    &session,
                    ModelEndpoint::OpenAiResponses,
                    br#"{"model":"test-model","input":"hello","stream":true}"#
                )
                .unwrap_err()
                .code,
            "STREAMING_UNAVAILABLE"
        );
        assert_eq!(
            broker
                .send(
                    &session,
                    ModelEndpoint::OpenAiResponses,
                    &vec![b'x'; MAX_REQUEST_BYTES + 1]
                )
                .unwrap_err()
                .code,
            "INVALID_REQUEST"
        );
        assert_eq!(store.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn missing_credentials_and_transport_errors_are_sanitized() {
        let endpoint = ModelEndpoint::OpenAiResponses;
        let sessions = Arc::new(HostSessions::default());
        let session = sessions.bind(scopes(endpoint)).unwrap();
        let transport = Arc::new(FakeTransport::new(endpoint, response(200, "{}")));
        let broker = ModelBroker::new(
            Arc::new(FakeStore {
                calls: AtomicUsize::new(0),
                fail: true,
            }),
            sessions.clone(),
            transport.clone(),
        );
        let error = broker
            .send(&session, endpoint, &body(endpoint))
            .unwrap_err();
        assert_eq!(error.code, "CREDENTIAL_UNAVAILABLE");
        assert!(!format!("{error:?}").contains(API_KEY));
        assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
        let broker = ModelBroker::new(
            Arc::new(FakeStore::default()),
            sessions,
            Arc::new(FakeTransport::new(
                endpoint,
                Err(BrokerError::new(API_KEY, API_KEY)),
            )),
        );
        let error = broker
            .send(&session, endpoint, &body(endpoint))
            .unwrap_err();
        assert_eq!(error.code, "UPSTREAM_UNAVAILABLE");
        assert!(!format!("{error}").contains(API_KEY));
    }

    #[test]
    fn rejects_redirect_error_oversized_invalid_and_credential_echo_responses() {
        let endpoint = ModelEndpoint::OpenAiResponses;
        let cases = vec![
            (
                TransportResponse {
                    status: 200,
                    body: b"{}".to_vec(),
                    redirected: true,
                },
                "REDIRECT_DENIED",
            ),
            (response(302, "{}").unwrap(), "REDIRECT_DENIED"),
            (response(401, API_KEY).unwrap(), "UPSTREAM_REJECTED"),
            (
                TransportResponse {
                    status: 200,
                    body: vec![b'x'; MAX_RESPONSE_BYTES + 1],
                    redirected: false,
                },
                "RESPONSE_TOO_LARGE",
            ),
            (response(200, "not json").unwrap(), "INVALID_RESPONSE"),
            (
                response(200, &format!("{{\"echo\":\"{API_KEY}\"}}")).unwrap(),
                "CREDENTIAL_LEAK_BLOCKED",
            ),
            (
                response(200, r#"{"echo":"host-private-api-ke\u0079"}"#).unwrap(),
                "CREDENTIAL_LEAK_BLOCKED",
            ),
        ];
        for (response, expected) in cases {
            assert!(!format!("{response:?}").contains(API_KEY));
            let sessions = Arc::new(HostSessions::default());
            let session = sessions.bind(scopes(endpoint)).unwrap();
            let broker = ModelBroker::new(
                Arc::new(FakeStore::default()),
                sessions,
                Arc::new(FakeTransport::new(endpoint, Ok(response))),
            );
            let error = broker
                .send(&session, endpoint, &body(endpoint))
                .unwrap_err();
            assert_eq!(error.code, expected);
            assert!(!format!("{error:?}").contains(API_KEY));
        }
    }

    struct RevokingStore {
        sessions: Arc<HostSessions>,
        session: SessionId,
    }
    impl CredentialStore for RevokingStore {
        fn resolve(&self, _: &CredentialId) -> BrokerResult<Secret> {
            self.sessions.revoke(&self.session).unwrap();
            Secret::new(API_KEY.into())
        }
    }
    #[test]
    fn revocation_during_credential_lookup_prevents_dispatch() {
        let endpoint = ModelEndpoint::OpenAiResponses;
        let sessions = Arc::new(HostSessions::default());
        let session = sessions.bind(scopes(endpoint)).unwrap();
        let transport = Arc::new(FakeTransport::new(endpoint, response(200, "{}")));
        let store = Arc::new(RevokingStore {
            sessions: sessions.clone(),
            session: session.clone(),
        });
        let broker = ModelBroker::new(store, sessions, transport.clone());
        assert_eq!(
            broker
                .send(&session, endpoint, &body(endpoint))
                .unwrap_err()
                .code,
            "SESSION_DENIED"
        );
        assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
    }
}
