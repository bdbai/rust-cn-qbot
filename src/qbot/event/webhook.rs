use std::{io, net::SocketAddr, time::Duration};

use http_body_util::BodyExt;
use hyper::{
    body::Body, header::USER_AGENT, server::conn::http1, service::service_fn, Request, Response,
    StatusCode,
};
use hyper_util::{rt::TokioIo, server::graceful::GracefulShutdown};
use tokio::{net::TcpListener, sync::Notify, time::timeout};
use tracing::{debug, error, info, warn};

use super::{payload::QBotWebSocketPayload, QBotWsMessageHandler};

async fn serve_request(
    req: Request<hyper::body::Incoming>,
    handler: impl QBotWsMessageHandler,
) -> Result<Response<String>, hyper::Error> {
    let ua = req
        .headers()
        .get(USER_AGENT)
        .map(|v| v.to_str().ok())
        .flatten()
        .unwrap_or("unknown");
    let app_id = req
        .headers()
        .get("X-Bot-Appid")
        .map(|v| v.to_str().ok())
        .flatten()
        .unwrap_or("unknown");
    debug!(%ua, %app_id, "Received request");
    let upper = req.body().size_hint().upper().unwrap_or(u64::MAX);
    if upper > 1024 * 64 {
        error!("Request body too large: {}", upper);
        return Ok(Response::builder()
            .status(StatusCode::PAYLOAD_TOO_LARGE)
            .body("Request body too large".to_string())
            .unwrap());
    }
    let whole_body = req.collect().await?.to_bytes();
    //QBotWebSocketPayload;

    todo!()
}

pub async fn serve<H: Clone + QBotWsMessageHandler + Send + Sync + 'static>(
    listen_addr: SocketAddr,
    handler: &H,
    quit_signal: &Notify,
) -> io::Result<()> {
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
        let handler = handler.clone();
        let service = service_fn(move |req| {
            let handler = handler.clone();
            async move { serve_request(req, handler).await }
        });
        let conn = http1::Builder::new().serve_connection(io, service);
        let conn = graceful.watch(conn);

        tokio::task::spawn(async {
            if let Err(err) = conn.await {
                error!("Error serving connection: {:?}", err);
            }
        });
    }

    info!("Gracefully closing webhook connections...");
    let shutdown_res = timeout(Duration::from_secs(10), graceful.shutdown()).await;
    match shutdown_res {
        Ok(()) => info!("Webhook connections closed."),
        Err(_) => warn!("Timeout while closing webhook connections."),
    }
    Ok(())
}
