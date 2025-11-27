use std::{io, net::SocketAddr, sync::Arc, time::Duration};

use challenge::ChallengeGenerator;
use http_body_util::Full;
use hyper::{server::conn::http1, service::service_fn};
use hyper_util::{rt::TokioIo, server::graceful::GracefulShutdown};
use service::QBotWebhookService;
use tokio::{net::TcpListener, sync::Notify, time::timeout};
use tracing::{error, info, warn};

mod challenge;
mod service;

use super::QBotEventMessageHandler;

pub struct WebhookServerFactory {
    graceful: GracefulShutdown,
}

pub struct WebhookServer<'q, H> {
    listener: TcpListener,
    service: QBotWebhookService<H>,
    quit_signal: &'q Notify,
    graceful: &'q GracefulShutdown,
}

impl Default for WebhookServerFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl WebhookServerFactory {
    pub fn new() -> Self {
        Self {
            graceful: GracefulShutdown::new(),
        }
    }

    pub async fn bind<'q, H: Clone + QBotEventMessageHandler + Send + Sync + 'static>(
        &'q self,
        listen_addr: SocketAddr,
        bot_secret: &str,
        handler: H,
        quit_signal: &'q Notify,
    ) -> io::Result<WebhookServer<'q, H>> {
        let service = QBotWebhookService {
            handler,
            challenge_generator: Arc::new(ChallengeGenerator::new(bot_secret)),
        };
        let listener = TcpListener::bind(listen_addr).await?;

        Ok(WebhookServer {
            listener,
            service,
            quit_signal,
            graceful: &self.graceful,
        })
    }

    pub async fn shutdown(self) {
        info!("Gracefully closing webhook connections...");
        let shutdown_res = timeout(Duration::from_secs(10), self.graceful.shutdown()).await;
        match shutdown_res {
            Ok(()) => info!("Webhook connections closed."),
            Err(_) => warn!("Timeout while closing webhook connections."),
        }
    }
}

impl<'q, H> WebhookServer<'q, H> {
    #[cfg(test)]
    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    pub async fn serve(self) -> io::Result<()>
    where
        H: Clone + QBotEventMessageHandler + Send + Sync + 'static,
    {
        'serve_loop: loop {
            let listen_res = tokio::select! {
                biased;

                listen_res = self.listener.accept() => listen_res,
                _ = self.quit_signal.notified() => {
                    break 'serve_loop;
                },
            };
            let (stream, _) = listen_res?;

            let io = TokioIo::new(stream);
            let service = self.service.clone();
            let service = service_fn(move |req| {
                let service = service.clone();
                async move { service.call(req).await.map(|res| res.map(Full::new)) }
            });
            let conn = http1::Builder::new().serve_connection(io, service);
            let conn = self.graceful.watch(conn);

            tokio::task::spawn(async {
                if let Err(err) = conn.await {
                    error!("Error serving connection: {:?}", err);
                }
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use serde_json::json;
    use tokio::sync::mpsc;

    use super::*;
    use crate::qbot::event::payload::AtMessageCreatePayload;
    use crate::qbot::event::MockQBotEventMessageHandler;

    async fn start_test_server(
        handler: Arc<MockQBotEventMessageHandler>,
    ) -> (
        SocketAddr,
        Arc<Notify>,
        tokio::task::JoinHandle<io::Result<()>>,
    ) {
        let quit_signal = Arc::new(Notify::new());

        let quit_signal_clone = quit_signal.clone();
        let factory = WebhookServerFactory::new();
        let (addr_tx, addr_rx) = tokio::sync::oneshot::channel();

        let server_handle = tokio::spawn(async move {
            let server = factory
                .bind(
                    SocketAddr::new(Ipv4Addr::LOCALHOST.into(), 0),
                    "test_secret",
                    handler,
                    &quit_signal_clone,
                )
                .await?;
            let addr = server.local_addr();
            addr_tx.send(addr).ok();
            server.serve().await?;
            factory.shutdown().await;
            Ok::<(), io::Error>(())
        });

        let addr = addr_rx.await.unwrap();
        (addr.unwrap(), quit_signal, server_handle)
    }

    async fn shutdown_server(
        quit_signal: Arc<Notify>,
        server_handle: tokio::task::JoinHandle<io::Result<()>>,
    ) {
        quit_signal.notify_one();
        server_handle.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn test_webhook_server_handles_at_message() {
        let (tx, mut rx) = mpsc::unbounded_channel::<AtMessageCreatePayload>();

        let mut mock_handler = MockQBotEventMessageHandler::new();
        mock_handler
            .expect_handle_at_message()
            .withf(|p: &AtMessageCreatePayload| {
                p.author.id == "user123"
                    && p.author.username == "testuser"
                    && p.channel_id == "channel456"
                    && p.content == "Hello bot!"
                    && p.guild_id == "guild789"
                    && p.id == "msg001"
            })
            .times(1)
            .returning(move |p| {
                let _ = tx.send(p);
            });

        let handler = Arc::new(mock_handler);
        let (addr, quit_signal, server_handle) = start_test_server(handler).await;

        // Send AT_MESSAGE_CREATE request
        let client = reqwest::Client::new();
        let payload = json!({
            "op": 0,
            "t": "AT_MESSAGE_CREATE",
            "d": {
                "author": {
                    "avatar": "http://example.com/avatar.png",
                    "id": "user123",
                    "username": "testuser"
                },
                "channel_id": "channel456",
                "content": "Hello bot!",
                "guild_id": "guild789",
                "id": "msg001",
                "member": {
                    "joined_at": "2024-01-01T00:00:00Z",
                    "roles": []
                },
                "timestamp": "2024-04-11T12:00:00Z",
                "seq": 1
            }
        });

        let res = client
            .post(format!("http://{}", addr))
            .json(&payload)
            .send()
            .await
            .unwrap();

        assert_eq!(res.status(), 200);

        // Wait for handler to be called
        let received = tokio::time::timeout(Duration::from_millis(100), rx.recv())
            .await
            .expect("timeout waiting for message")
            .expect("channel closed");

        assert_eq!(received.author.id, "user123");

        shutdown_server(quit_signal, server_handle).await;
    }

    #[tokio::test]
    async fn test_webhook_server_handles_multiple_messages() {
        let (tx, mut rx) = mpsc::unbounded_channel::<AtMessageCreatePayload>();

        let mut mock_handler = MockQBotEventMessageHandler::new();
        mock_handler
            .expect_handle_at_message()
            .times(3)
            .returning(move |p| {
                let _ = tx.send(p);
            });

        let handler = Arc::new(mock_handler);
        let (addr, quit_signal, server_handle) = start_test_server(handler).await;

        let client = reqwest::Client::new();

        // Send multiple messages
        for i in 0..3 {
            let payload = json!({
                "op": 0,
                "t": "AT_MESSAGE_CREATE",
                "d": {
                    "author": { "avatar": "", "id": format!("user{i}"), "username": "user" },
                    "channel_id": "ch",
                    "content": format!("message {i}"),
                    "guild_id": "guild",
                    "id": format!("msg{i}"),
                    "member": { "joined_at": "", "roles": [] },
                    "timestamp": "",
                    "seq": i
                }
            });

            let res = client
                .post(format!("http://{}", addr))
                .json(&payload)
                .send()
                .await
                .unwrap();
            assert_eq!(res.status(), 200);
        }

        // Verify all 3 messages were received
        for i in 0..3 {
            let received = tokio::time::timeout(Duration::from_millis(100), rx.recv())
                .await
                .expect("timeout waiting for message")
                .expect("channel closed");
            assert_eq!(received.author.id, format!("user{i}"));
            assert_eq!(received.content, format!("message {i}"));
        }

        shutdown_server(quit_signal, server_handle).await;
    }

    #[tokio::test]
    async fn test_webhook_server_ignores_non_at_message_events() {
        let mut mock_handler = MockQBotEventMessageHandler::new();
        mock_handler.expect_handle_at_message().never();

        let handler = Arc::new(mock_handler);
        let (addr, quit_signal, server_handle) = start_test_server(handler).await;

        let client = reqwest::Client::new();

        // Send DIRECT_MESSAGE_CREATE (should not trigger handler)
        let payload = json!({
            "op": 0,
            "t": "DIRECT_MESSAGE_CREATE",
            "d": {
                "author": { "avatar": "", "id": "user", "username": "user" },
                "channel_id": "ch",
                "content": "direct message",
                "guild_id": "guild",
                "id": "msg",
                "member": { "joined_at": "", "roles": [] },
                "timestamp": ""
            }
        });

        let res = client
            .post(format!("http://{}", addr))
            .json(&payload)
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 200);

        // Give some time to ensure handler is not called
        tokio::time::sleep(Duration::from_millis(50)).await;

        shutdown_server(quit_signal, server_handle).await;
        // Mock will panic on drop if handle_at_message was called
    }

    #[tokio::test]
    async fn test_webhook_server_returns_error_for_unknown_opcode() {
        let mut mock_handler = MockQBotEventMessageHandler::new();
        mock_handler.expect_handle_at_message().never();

        let handler = Arc::new(mock_handler);
        let (addr, quit_signal, server_handle) = start_test_server(handler).await;

        let client = reqwest::Client::new();

        // Send unknown opcode (valid u8 but unhandled by the server)
        let payload = json!({
            "op": 99,
            "d": {}
        });

        let res = client
            .post(format!("http://{}", addr))
            .json(&payload)
            .send()
            .await
            .unwrap();

        assert_eq!(res.status(), 400);

        shutdown_server(quit_signal, server_handle).await;
    }

    #[tokio::test]
    async fn test_webhook_server_rejects_large_body() {
        let mut mock_handler = MockQBotEventMessageHandler::new();
        mock_handler.expect_handle_at_message().never();

        let handler = Arc::new(mock_handler);
        let (addr, quit_signal, server_handle) = start_test_server(handler).await;

        let client = reqwest::Client::new();

        // Send oversized body (> 64KB)
        let large_content = "x".repeat(65 * 1024);
        let payload = json!({
            "op": 0,
            "t": "AT_MESSAGE_CREATE",
            "d": {
                "content": large_content
            }
        });

        let res = client
            .post(format!("http://{}", addr))
            .header("content-length", "70000")
            .json(&payload)
            .send()
            .await
            .unwrap();

        assert_eq!(res.status(), 413); // Payload Too Large

        shutdown_server(quit_signal, server_handle).await;
    }
}
