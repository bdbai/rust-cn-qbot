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

pub struct WebhookServer {
    graceful: GracefulShutdown,
}

impl WebhookServer {
    pub async fn serve<H: Clone + QBotEventMessageHandler + Send + Sync + 'static>(
        listen_addr: SocketAddr,
        bot_secret: &str,
        handler: H,
        quit_signal: &Notify,
    ) -> io::Result<Self> {
        let service = QBotWebhookService {
            handler: handler,
            challenge_generator: Arc::new(ChallengeGenerator::new(bot_secret)),
        };
        let graceful = GracefulShutdown::new();
        let listener = TcpListener::bind(listen_addr).await?;

        'serve_loop: loop {
            let listen_res = tokio::select! {
                biased;

                listen_res = listener.accept() => listen_res,
                _ = quit_signal.notified() => {
                    break 'serve_loop;
                },
            };
            let (stream, _) = listen_res?;

            let io = TokioIo::new(stream);
            let service = service.clone();
            let service = service_fn(move |req| {
                let service = service.clone();
                async move { service.call(req).await.map(|res| res.map(Full::new)) }
            });
            let conn = http1::Builder::new().serve_connection(io, service);
            let conn = graceful.watch(conn);

            tokio::task::spawn(async {
                if let Err(err) = conn.await {
                    error!("Error serving connection: {:?}", err);
                }
            });
        }

        Ok(Self { graceful })
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
