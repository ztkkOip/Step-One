//! Host HTTPS transport. No redirects, ambient proxy, cookies or user certificates.
use crate::broker::{BrokerError, HostRequest, TransportResponse, TrustedTransport};
use reqwest::blocking::Client;
use std::io::Read;
use std::time::Duration;

pub struct HttpsModelTransport {
    client: Client,
}

impl HttpsModelTransport {
    pub fn new() -> Result<Self, BrokerError> {
        let client = Client::builder()
            .https_only(true)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|_| transport_error())?;
        Ok(Self { client })
    }
}

impl TrustedTransport for HttpsModelTransport {
    fn send(&self, request: &HostRequest) -> Result<TransportResponse, BrokerError> {
        // HostRequest only exposes a static, enumerated provider URL. Guest
        // requests cannot supply a scheme, host, path, proxy or arbitrary header.
        let mut pending = self
            .client
            .post(request.url())
            .body(request.body().to_vec());
        for (name, value) in request.headers() {
            let mut header =
                reqwest::header::HeaderValue::from_str(value).map_err(|_| transport_error())?;
            header.set_sensitive(true);
            pending = pending.header(name, header);
        }
        let response = pending.send().map_err(|_| transport_error())?;
        let status = response.status();
        if status.is_redirection() || !status.is_success() {
            // Drop body without returning upstream diagnostics or auth challenges.
            return Ok(TransportResponse {
                status: status.as_u16(),
                body: vec![],
                redirected: status.is_redirection(),
            });
        }
        let body = read_bounded(response, 8 * 1024 * 1024)?;
        Ok(TransportResponse {
            status: status.as_u16(),
            body,
            redirected: false,
        })
    }
}

fn read_bounded(reader: impl Read, limit: usize) -> Result<Vec<u8>, BrokerError> {
    let mut body = Vec::new();
    reader
        .take((limit + 1) as u64)
        .read_to_end(&mut body)
        .map_err(|_| transport_error())?;
    if body.len() > limit {
        return Err(BrokerError::new(
            "RESPONSE_TOO_LARGE",
            "模型响应超过允许大小。",
        ));
    }
    Ok(body)
}

fn transport_error() -> BrokerError {
    BrokerError::new(
        "UPSTREAM_UNAVAILABLE",
        "模型上游请求未完成，请检查连接或重试。",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_response_refuses_oversized_payloads() {
        assert_eq!(read_bounded(&b"1234"[..], 4).unwrap(), b"1234");
        assert_eq!(
            read_bounded(&b"12345"[..], 4).unwrap_err().code,
            "RESPONSE_TOO_LARGE"
        );
    }
    #[test]
    fn io_failures_never_expose_remote_content() {
        struct Broken;
        impl Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("credential-value"))
            }
        }
        let error = read_bounded(Broken, 10).unwrap_err();
        assert!(!error.message.contains("credential-value"));
    }
    #[test]
    fn client_can_be_constructed_without_a_credential_or_request() {
        assert!(HttpsModelTransport::new().is_ok());
    }
}
