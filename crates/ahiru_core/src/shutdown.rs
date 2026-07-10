use std::sync::OnceLock;

use tokio::sync::watch;



static SHUTDOWN_TX: OnceLock<watch::Sender<bool>> = OnceLock::new();



pub fn shutdown_receiver() -> watch::Receiver<bool> {

    let (tx, rx) = watch::channel(false);

    let _ = SHUTDOWN_TX.set(tx);

    rx

}



pub fn reset_shutdown() {

    if let Some(tx) = SHUTDOWN_TX.get() {

        let _ = tx.send(false);

    }

}



pub fn trigger_shutdown() {

    if let Some(tx) = SHUTDOWN_TX.get() {

        let _ = tx.send(true);

    }

}



pub async fn wait_for_shutdown(rx: watch::Receiver<bool>, drain_secs: u64) {
    #[cfg(unix)]
    {
        wait_for_shutdown_signal(rx, drain_secs).await;
        return;
    }
    #[cfg(not(unix))]
    wait_for_shutdown_common(rx, drain_secs).await;
}

#[cfg(not(unix))]
async fn wait_for_shutdown_common(mut rx: watch::Receiver<bool>, drain_secs: u64) {

    tokio::select! {

        _ = tokio::signal::ctrl_c() => {},

        _ = async {

            loop {

                if *rx.borrow() {

                    break;

                }

                if rx.changed().await.is_err() {

                    break;

                }

            }

        } => {},

    }

    if drain_secs > 0 {

        tokio::time::sleep(std::time::Duration::from_secs(drain_secs)).await;

    }

}



#[cfg(unix)]

pub async fn wait_for_shutdown_signal(mut rx: watch::Receiver<bool>, drain_secs: u64) {

    use tokio::signal::unix::{signal, SignalKind};

    let mut term = signal(SignalKind::terminate()).ok();

    tokio::select! {

        _ = tokio::signal::ctrl_c() => {},

        _ = async {

            if let Some(ref mut s) = term {

                let _ = s.recv().await;

            } else {

                std::future::pending::<()>().await;

            }

        } => {},

        _ = async {

            loop {

                if *rx.borrow() { break; }

                if rx.changed().await.is_err() { break; }

            }

        } => {},

    }

    if drain_secs > 0 {

        tokio::time::sleep(std::time::Duration::from_secs(drain_secs)).await;

    }

}

