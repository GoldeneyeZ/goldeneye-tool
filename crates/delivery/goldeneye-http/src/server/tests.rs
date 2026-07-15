use std::net::SocketAddr;
use std::thread;

use serde_json::json;

use crate::{ApiBackend, ApiError, ApiRequest, ApiResponse};

use super::{BoundServer, ServerConfig};

struct NoopBackend;

impl ApiBackend for NoopBackend {
    fn handle(&self, _request: &ApiRequest) -> Result<ApiResponse, ApiError> {
        Ok(ApiResponse::ok(json!({})))
    }
}

#[test]
fn shutdown_handle_stops_the_blocking_server_loop() {
    let server = BoundServer::bind(
        ServerConfig::new(SocketAddr::from(([127, 0, 0, 1], 0))),
        NoopBackend,
    )
    .expect("bind server");
    let shutdown = server.shutdown_handle();
    let clone = shutdown.clone();
    let join = thread::spawn(move || server.run());

    clone.shutdown();
    join.join()
        .expect("server thread")
        .expect("server shutdown");

    assert!(shutdown.is_shutdown());
}
