use std::sync::Arc;

use tokio::sync::Notify;
use tracing::{error, info};

pub mod crawler;
pub mod handler;
pub mod post;
pub mod qbot;

#[derive(Debug, thiserror::Error)]
enum CliError {
    #[error("QBotApiError: {0}")]
    QBotApiError(#[from] qbot::QBotApiError),
    #[error("QBotWsError: {0}")]
    QBotWsError(#[from] qbot::QBotWsError),
}

async fn run_env(
    authorizer: Arc<qbot::QBotCachingAuthorizerImpl>,
    api_base_url: String,
    app_id: &str,
    quit_signal: &Notify,
) -> Result<(), CliError> {
    let api_client = qbot::QBotApiClientImpl::new(api_base_url, app_id, authorizer.clone());
    let ws_gateway = api_client.get_ws_gateway().await?;
    let handler = handler::EventHandler::new(api_client);

    qbot::ws::run_loop(ws_gateway, &*authorizer, handler, quit_signal).await?;
    Ok(())
}

async fn run_production(
    enabled: bool,
    authorizer: Arc<qbot::QBotCachingAuthorizerImpl>,
    app_id: &str,
    quit_signal: &Notify,
) -> Result<(), CliError> {
    if enabled {
        info!("running production");
        run_env(
            authorizer,
            "https://api.sgroup.qq.com".into(),
            app_id,
            quit_signal,
        )
        .await?;
    } else {
        info!("production disabled");
    }
    Ok(())
}

async fn run_sandbox(
    enabled: bool,
    authorizer: Arc<qbot::QBotCachingAuthorizerImpl>,
    app_id: &str,
    quit_signal: &Notify,
) -> Result<(), CliError> {
    if enabled {
        info!("running sandbox");
        run_env(
            authorizer,
            "https://sandbox.api.sgroup.qq.com".into(),
            app_id,
            quit_signal,
        )
        .await?;
    } else {
        info!("sandbox disabled");
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    use std::pin::pin;

    use futures::future::try_join;
    use tokio::signal::ctrl_c;
    use tokio::sync::Notify;

    tracing_subscriber::fmt::init();

    let app_id = std::env::var("QBOT_APP_ID").unwrap();
    let client_secret = std::env::var("QBOT_CLIENT_SECRET").unwrap();
    let authorizer = qbot::QBotCachingAuthorizerImpl::create_and_authorize(
        "https://bots.qq.com".into(),
        app_id.clone(),
        client_secret,
    )
    .await
    .expect("failed to create authorizer"); // TODO: better error handling
    let authorizer = Arc::new(authorizer);

    let quit_signal = Notify::const_new();
    let fut_production = run_production(false, authorizer.clone(), &app_id, &quit_signal);
    let fut_sandbox = run_sandbox(true, authorizer, &app_id, &quit_signal);
    let mut ws_fut = pin!(try_join(fut_production, fut_sandbox));
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
