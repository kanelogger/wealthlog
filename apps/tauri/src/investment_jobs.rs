//! Runtime-owned journal index maintenance. No fact, purpose or enablement writes.
use crate::context::ServiceContext;
use futures::FutureExt;
use std::{
    panic::AssertUnwindSafe,
    sync::{atomic::Ordering, Arc},
    time::Duration,
};

pub fn spawn_projection(
    context: Arc<ServiceContext>,
    handle: Option<tauri::AppHandle>,
) -> tauri::async_runtime::JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        let mut clear_old_error = true;
        let mut historical_complete = None;
        while context.active.load(Ordering::SeqCst) {
            let step = AssertUnwindSafe(context.investment_projection.maintenance_batch(8))
                .catch_unwind()
                .await;
            let delay = match step {
                Ok(Ok(progress)) => {
                    let changed = progress.scanned > 0
                        || progress.refreshed.processed > 0
                        || historical_complete != Some(progress.historical_complete)
                        || clear_old_error;
                    if clear_old_error {
                        clear_old_error = context
                            .investment_projection
                            .record_background_error(None)
                            .await
                            .is_err();
                    }
                    historical_complete = Some(progress.historical_complete);
                    if changed {
                        if let Some(handle) = &handle {
                            let _ = crate::events::emit_for_profile(
                                handle,
                                &context,
                                "investment:index-updated",
                                (),
                            );
                        }
                    }
                    if progress.historical_complete && progress.refreshed.remaining == 0 {
                        Duration::from_secs(2)
                    } else {
                        Duration::from_millis(50)
                    }
                }
                failure => {
                    if !context.active.load(Ordering::SeqCst) {
                        break;
                    }
                    let code = match &failure {
                        Ok(Err(error)) => error.code(),
                        _ => "projection_panic",
                    };
                    log::warn!("Investment execution index requires retry ({code})");
                    let _ = context
                        .investment_projection
                        .record_background_error(Some(code))
                        .await;
                    if let Some(handle) = &handle {
                        let _ = crate::events::emit_for_profile(
                            handle,
                            &context,
                            "investment:index-updated",
                            (),
                        );
                    }
                    clear_old_error = true;
                    Duration::from_secs(2)
                }
            };
            tokio::time::sleep(delay).await;
        }
    })
}
