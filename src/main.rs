use std::env::VarError;
use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;

use tokio::sync::Notify;
use tracing::{error, info};

pub mod controller;
pub mod crawler;
pub mod handler;
pub mod post;
pub mod qbot;
use qbot::{event::ws::QBotWebSocketAuthGroup, QBotEventError};

#[derive(Debug, thiserror::Error)]
enum CliError {
    #[error("QBotApiError: {0}")]
    QBotApiError(#[from] qbot::QBotApiError),
    #[error("QBotWsError: {0}")]
    QBotWsError(#[from] qbot::QBotEventError),
}

enum EnvRun<A, H> {
    Ws {
        ws_gateway: String,
        authorizer: Arc<A>,
        handler: H,
    },
    Webhook {
        listen_addr: SocketAddr,
        client_secret: String,
        handler: H,
    },
}

trait RunLoop {
    fn run_loop(
        self,
        quit_signal: &Notify,
        auth_group: &QBotWebSocketAuthGroup,
    ) -> impl Future<Output = qbot::QBotEventResult<()>> + Send;
}

impl<
        A: qbot::QBotAuthorizer + Send + Sync,
        H: qbot::event::QBotEventMessageHandler + Clone + Send + Sync + 'static,
    > RunLoop for EnvRun<A, H>
{
    async fn run_loop(
        self,
        quit_signal: &Notify,
        auth_group: &QBotWebSocketAuthGroup,
    ) -> qbot::QBotEventResult<()> {
        match self {
            EnvRun::Ws {
                ws_gateway,
                authorizer,
                handler,
            } => {
                qbot::event::ws::run_loop(
                    ws_gateway,
                    &*authorizer,
                    handler,
                    quit_signal,
                    auth_group,
                )
                .await
            }
            EnvRun::Webhook {
                listen_addr,
                client_secret,
                handler,
            } => {
                let server_factory = qbot::event::webhook::WebhookServerFactory::new();
                let server = server_factory
                    .bind(listen_addr, &client_secret, handler, quit_signal)
                    .await
                    .map_err(QBotEventError::WebhookServeError)?;
                server
                    .serve()
                    .await
                    .map_err(QBotEventError::WebhookServeError)?;
                server_factory.shutdown().await;
                Ok(())
            }
        }
    }
}

async fn run_env(
    webhook_listen_addr: Option<std::net::SocketAddr>,
    crawler: Arc<crawler::CrawlerImpl>,
    api_base_url: String,
    app_id: &str,
    news_channel_id: String,
) -> Result<impl RunLoop, CliError> {
    let client_secret = std::env::var("QBOT_CLIENT_SECRET").unwrap();
    let authorizer = qbot::QBotCachingAuthorizerImpl::create_and_authorize(
        "https://bots.qq.com".into(),
        app_id.into(),
        client_secret.clone(),
    )
    .await
    .expect("failed to create authorizer"); // TODO: better error handling
    let authorizer = Arc::new(authorizer);
    let api_client = Arc::new(qbot::QBotApiClientImpl::new(
        api_base_url,
        app_id,
        authorizer.clone(),
    ));
    let controller = controller::ControllerImpl::new(api_client.clone(), crawler, news_channel_id);
    let handler = handler::EventHandler::new(api_client.clone(), controller);

    Ok(if let Some(listen_addr) = webhook_listen_addr {
        EnvRun::Webhook {
            listen_addr,
            client_secret,
            handler,
        }
    } else {
        let ws_gateway = api_client.get_ws_gateway().await?;
        EnvRun::Ws {
            ws_gateway,
            authorizer,
            handler,
        }
    })
}

async fn run_production(
    enabled: bool,
    webhook_listen_addr: Option<std::net::SocketAddr>,
    crawler: Arc<crawler::CrawlerImpl>,
    app_id: &str,
) -> Result<Option<impl RunLoop>, CliError> {
    if enabled {
        info!("running production");
        let news_channel_id = std::env::var("QBOT_PRODUCTION_NEWS_CHANNEL_ID").unwrap();
        Ok(Some(
            run_env(
                webhook_listen_addr,
                crawler,
                "https://api.sgroup.qq.com".into(),
                app_id,
                news_channel_id,
            )
            .await?,
        ))
    } else {
        info!("production disabled");
        Ok(None)
    }
}

async fn run_sandbox(
    enabled: bool,
    crawler: Arc<crawler::CrawlerImpl>,
    app_id: &str,
) -> Result<Option<impl RunLoop>, CliError> {
    if enabled {
        info!("running sandbox");
        let news_channel_id = std::env::var("QBOT_SANDBOX_NEWS_CHANNEL_ID").unwrap();
        Ok(Some(
            run_env(
                None,
                crawler,
                "https://sandbox.api.sgroup.qq.com".into(),
                app_id,
                news_channel_id,
            )
            .await?,
        ))
    } else {
        info!("sandbox disabled");
        Ok(None)
    }
}

#[tokio::main]
async fn main() {
    use std::pin::pin;

    use futures::future::try_join;
    use tokio::signal::ctrl_c;
    use tokio::sync::Notify;

    tracing_subscriber::fmt::init();

    let app_id = std::env::var("QBOT_APP_ID").unwrap();

    let quit_signal = Notify::const_new();
    let crawler = Arc::new(crawler::CrawlerImpl::new("https://rustcc.cn".into()));
    let production_enabled = std::env::var("QBOT_PRODUCTION_ENABLED")
        .as_deref()
        .unwrap_or("false")
        .parse()
        .expect("QBOT_PRODUCTION_ENABLED must be a boolean");
    let sandbox_enabled = std::env::var("QBOT_SANDBOX_ENABLED")
        .as_deref()
        .unwrap_or("false")
        .parse()
        .expect("QBOT_SANDBOX_ENABLED must be a boolean");
    let production_webhook_listen_addr =
        match std::env::var("QBOT_PRODUCTION_WEBHOOK_LISTEN_ADDR").as_deref() {
            Ok(addr) => {
                let addr = addr
                    .parse()
                    .expect("QBOT_PRODUCTION_WEBHOOK_LISTEN_ADDR must be a valid address");
                info!("production webhook listen addr: {}", addr);
                Some(addr)
            }
            Err(VarError::NotPresent) => None,
            Err(VarError::NotUnicode(_)) => {
                panic!("QBOT_PRODUCTION_WEBHOOK_LISTEN_ADDR must be a valid address")
            }
        };
    let fut_production = run_production(
        production_enabled,
        production_webhook_listen_addr,
        crawler.clone(),
        &app_id,
    )
    .await
    .expect("Starting production");
    let fut_sandbox = run_sandbox(sandbox_enabled, crawler, &app_id)
        .await
        .expect("Starting sandbox");
    let auth_group = QBotWebSocketAuthGroup::new();
    let mut ws_fut = pin!(try_join(
        async {
            if let Some(fut) = fut_production {
                fut.run_loop(&quit_signal, &auth_group).await?;
            }
            qbot::QBotEventResult::Ok(())
        },
        async {
            if let Some(fut) = fut_sandbox {
                fut.run_loop(&quit_signal, &auth_group).await?;
            }
            Ok(())
        }
    ));
    let mut ctrlc_hit = false;
    let ws_res = 'ctrlc_loop: loop {
        tokio::select! {
            biased;
            _ = ctrl_c() => {
                info!("received ctrl-c");
                if std::mem::replace(&mut ctrlc_hit, true) {
                    info!("force quit");
                    return;
                }
                quit_signal.notify_waiters();
            },
            res = ws_fut.as_mut() => break 'ctrlc_loop res,
        }
    };
    match &ws_res {
        Ok(((), ())) => {
            info!("shutting down");
        }
        Err(err) => {
            error!("ws loop fatal error: {:?} {}", err, err);
            std::process::exit(101);
        }
    }
}
