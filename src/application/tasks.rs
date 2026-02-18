use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tracing::{info, error};
use crate::application::FixThumbnailsUseCase;
use crate::presentation::WsMessage;
use serde_json;

pub struct TaskRunner {
    fix_thumbnails_use_case: Arc<FixThumbnailsUseCase>,
    tx: broadcast::Sender<Arc<str>>,
}

impl TaskRunner {
    pub fn new(
        fix_thumbnails_use_case: Arc<FixThumbnailsUseCase>,
        tx: broadcast::Sender<Arc<str>>,
    ) -> Self {
        Self {
            fix_thumbnails_use_case,
            tx,
        }
    }

    pub fn start(self) {
        let runner = Arc::new(self);
        
        // Start thumbnail fix task
        let r = runner.clone();
        tokio::spawn(async move {
            // Initial delay to let the system settle
            tokio::time::sleep(Duration::from_secs(15)).await;
            
            loop {
                info!("Starting scheduled thumbnail fix task...");
                match r.fix_thumbnails_use_case.execute().await {
                    Ok(fixed_items) => {
                        let count = fixed_items.len();
                        if count > 0 {
                            info!("Scheduled thumbnail fix completed. Fixed {} items.", count);
                            
                            // Broadcast updates for each item
                            for item in fixed_items {
                                if let Ok(json_item) = serde_json::to_value(&item) {
                                    let msg = WsMessage::MediaUpdated { 
                                        id: item.id, 
                                        item: json_item 
                                    };
                                    r.broadcast(msg);
                                }
                            }
                            
                            r.broadcast(WsMessage::ThumbnailFixCompleted { count });
                        } else {
                            info!("Scheduled thumbnail fix completed. No items needed fixing.");
                        }
                    }
                    Err(e) => {
                        error!("Scheduled thumbnail fix failed: {}", e);
                    }
                }
                
                // Run once every 24 hours
                tokio::time::sleep(Duration::from_secs(86400)).await;
            }
        });
        
        // Add more background tasks here as needed
    }

    fn broadcast(&self, msg: WsMessage) {
        if let Ok(json) = serde_json::to_string(&msg) {
            let _ = self.tx.send(Arc::from(json));
        }
    }
}
