use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum RequestId {
    Number(i64),
    String(String),
}

#[derive(Debug, Clone, Deserialize)]
pub struct Request {
    pub jsonrpc: String,
    #[serde(default)]
    pub id: Option<RequestId>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

impl Request {
    /// Parses one JSON-RPC request from JSON text.
    ///
    /// # Errors
    ///
    /// Returns a [`serde_json::Error`] when `input` is not a valid request.
    pub fn parse(input: &str) -> serde_json::Result<Self> {
        serde_json::from_str(input)
    }

    #[must_use]
    pub const fn is_notification(&self) -> bool {
        self.id.is_none()
    }
}

#[derive(Debug, Serialize)]
pub struct ErrorObject {
    pub code: i32,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct Response {
    pub jsonrpc: &'static str,
    pub id: Option<RequestId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorObject>,
}

impl Response {
    #[must_use]
    pub fn success(id: RequestId, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id: Some(id),
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: Option<RequestId>, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(ErrorObject {
                code,
                message: message.into(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Request, RequestId, Response};
    use serde_json::json;

    #[test]
    fn request_accepts_numeric_and_string_ids() {
        let numeric =
            Request::parse(r#"{"jsonrpc":"2.0","id":7,"method":"ping"}"#).expect("numeric ID");
        let string =
            Request::parse(r#"{"jsonrpc":"2.0","id":"abc","method":"ping"}"#).expect("string ID");
        assert_eq!(numeric.id, Some(RequestId::Number(7)));
        assert_eq!(string.id, Some(RequestId::String("abc".into())));
    }

    #[test]
    fn missing_id_is_notification() {
        let request = Request::parse(r#"{"jsonrpc":"2.0","method":"notifications/cancelled"}"#)
            .expect("notification");
        assert!(request.is_notification());
    }

    #[test]
    fn success_response_serializes_result_without_error() {
        let response = Response::success(RequestId::Number(7), json!({"pong": true}));

        assert_eq!(
            serde_json::to_value(response).expect("serialize success response"),
            json!({
                "jsonrpc": "2.0",
                "id": 7,
                "result": {"pong": true}
            })
        );
    }

    #[test]
    fn error_response_serializes_error_without_result() {
        let response = Response::error(None, -32700, "Parse error");

        assert_eq!(
            serde_json::to_value(response).expect("serialize error response"),
            json!({
                "jsonrpc": "2.0",
                "id": null,
                "error": {"code": -32700, "message": "Parse error"}
            })
        );
    }
}
