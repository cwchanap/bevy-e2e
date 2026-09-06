use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use bevy::remote::BrpRequest;
use serde_json::Value;

use crate::{Error, Result};

/// Synchronous one-response BRP HTTP client (no SSE/watch support).
pub(crate) struct BrpClient {
    url: String,
    timeout: Duration,
    next_id: AtomicU64,
}

#[allow(dead_code)] // consumed by later client façade tasks; exercised in unit tests
impl BrpClient {
    pub(crate) fn new(port: u16, timeout: Duration) -> Self {
        Self {
            url: format!("http://127.0.0.1:{port}/"),
            timeout,
            next_id: AtomicU64::new(1),
        }
    }

    pub(crate) fn request(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request = BrpRequest {
            method: method.to_owned(),
            id: Some(serde_json::json!(id)),
            params: Some(params),
        };

        let mut response = ureq::post(&self.url)
            .config()
            .timeout_global(Some(self.timeout))
            .build()
            .send_json(&request)
            .map_err(|error| Error::Brp {
                method: method.to_owned(),
                message: error.to_string(),
            })?;

        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");
        if content_type.starts_with("text/event-stream") {
            return Err(Error::UnsupportedWatch(method.to_owned()));
        }

        let body: Value = response
            .body_mut()
            .read_json()
            .map_err(|error| Error::Brp {
                method: method.to_owned(),
                message: format!("malformed BRP response: {error}"),
            })?;

        if let Some(result) = body.get("result") {
            return Ok(result.clone());
        }

        if let Some(error) = body.get("error") {
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown BRP error")
                .to_owned();
            return Err(Error::Brp {
                method: method.to_owned(),
                message,
            });
        }

        Err(Error::Brp {
            method: method.to_owned(),
            message: "malformed BRP response: missing result/error".to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::BrpClient;
    use crate::Error;
    use serde_json::{Value, json};
    use std::{
        sync::{Arc, Mutex},
        thread,
        time::Duration,
    };
    use tiny_http::{Header, Response, Server};

    fn serve_one(handler: impl FnOnce(tiny_http::Request) + Send + 'static) -> u16 {
        let server = Server::http("127.0.0.1:0").unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        thread::spawn(move || {
            let request = server.recv().unwrap();
            handler(request);
        });
        port
    }

    #[test]
    fn returns_instant_result() {
        let captured: Arc<Mutex<Option<Value>>> = Arc::new(Mutex::new(None));
        let captured_for_server = Arc::clone(&captured);

        let port = serve_one(move |mut request| {
            let mut body = String::new();
            request.as_reader().read_to_string(&mut body).unwrap();
            let parsed: Value = serde_json::from_str(&body).unwrap();
            *captured_for_server.lock().unwrap() = Some(parsed);

            let response =
                Response::from_string(r#"{"jsonrpc":"2.0","id":1,"result":["bevy_time::Time"]}"#)
                    .with_header(
                        Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                    );
            request.respond(response).unwrap();
        });

        let client = BrpClient::new(port, Duration::from_secs(2));
        let result = client
            .request("world.list_resources", json!({}))
            .expect("instant result");

        assert_eq!(result, json!(["bevy_time::Time"]));

        let request = captured
            .lock()
            .unwrap()
            .clone()
            .expect("captured POST body");
        assert_eq!(request["method"], "world.list_resources");
        assert!(request.get("id").is_some());
    }

    #[test]
    fn preserves_remote_method_and_error_message() {
        let port = serve_one(|request| {
            let response = Response::from_string(
                r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"Method not found"}}"#,
            )
            .with_header(
                Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
            );
            request.respond(response).unwrap();
        });

        let client = BrpClient::new(port, Duration::from_secs(2));
        let err = client
            .request("missing.method", json!({}))
            .expect_err("remote error");

        match err {
            Error::Brp { method, message } => {
                assert_eq!(method, "missing.method");
                assert_eq!(message, "Method not found");
            }
            other => panic!("expected Error::Brp, got {other:?}"),
        }
    }

    #[test]
    fn rejects_event_stream_response() {
        let port = serve_one(|request| {
            let response = Response::from_string("event: update\ndata: {}\n\n").with_header(
                Header::from_bytes(&b"Content-Type"[..], &b"text/event-stream"[..]).unwrap(),
            );
            request.respond(response).unwrap();
        });

        let client = BrpClient::new(port, Duration::from_secs(2));
        let err = client
            .request("world.get_components+watch", json!({}))
            .expect_err("SSE rejected");

        match err {
            Error::UnsupportedWatch(detail) => {
                assert!(
                    detail.contains("world.get_components+watch")
                        || detail.contains("text/event-stream"),
                    "unexpected UnsupportedWatch detail: {detail}"
                );
            }
            other => panic!("expected Error::UnsupportedWatch, got {other:?}"),
        }
    }
}
