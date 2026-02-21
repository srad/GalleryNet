use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tracing::{info, error};
use crate::application::{FixThumbnailsUseCase, ScanFacesUseCase, GroupFacesUseCase};
use crate::presentation::WsMessage;
use serde_json;

pub struct TaskRunner {
    fix_thumbnails_use_case: Arc<FixThumbnailsUseCase>,
    scan_faces_use_case: Arc<ScanFacesUseCase>,
    group_faces_use_case: Arc<GroupFacesUseCase>,
    tx: broadcast::Sender<Arc<str>>,
}

impl TaskRunner {
    pub fn new(
        fix_thumbnails_use_case: Arc<FixThumbnailsUseCase>,
        scan_faces_use_case: Arc<ScanFacesUseCase>,
        group_faces_use_case: Arc<GroupFacesUseCase>,
        tx: broadcast::Sender<Arc<str>>,
    ) -> Self {
        Self {
            fix_thumbnails_use_case,
            scan_faces_use_case,
            group_faces_use_case,
            tx,
        }
    }

    pub fn start(self) {
        let runner = Arc::new(self);
        
        // Start thumbnail fix task
        let r1 = runner.clone();
        tokio::spawn(async move {
            // Initial delay to let the system settle
            tokio::time::sleep(Duration::from_secs(15)).await;
            
            loop {
                info!("Starting scheduled thumbnail fix task...");
                match r1.fix_thumbnails_use_case.execute().await {
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
                                    r1.broadcast(msg);
                                }
                            }
                            
                            r1.broadcast(WsMessage::ThumbnailFixCompleted { count });
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

        // Start face scan task
        let r2 = runner.clone();
        tokio::spawn(async move {
            // Initial delay
            tokio::time::sleep(Duration::from_secs(5)).await;

            // Run clustering once on startup to catch up existing faces
            info!("Running initial face clustering...");
            match r2.group_faces_use_case.execute(0.4).await {
                Ok(groups) => info!("Initial clustering complete. {} groups active.", groups.len()),
                Err(e) => error!("Initial clustering failed: {}", e),
            }

            loop {
                info!("Checking for unscanned faces...");
                match r2.scan_faces_use_case.execute().await {
                    Ok(count) => {
                        if count > 0 {
                            info!("Background face scan processed {} items.", count);
                            
                            // Trigger clustering immediately after scanning new faces
                            info!("Running face clustering...");
                            match r2.group_faces_use_case.execute(0.4).await { // 0.4 similarity threshold for ArcFace
                                Ok(groups) => info!("Clustering complete. {} groups active.", groups.len()),
                                Err(e) => error!("Clustering failed: {}", e),
                            }

                            // If we found items, run again soon to churn through the backlog
                            tokio::time::sleep(Duration::from_secs(5)).await;
                        } else {
                            // No items found, sleep longer
                            tokio::time::sleep(Duration::from_secs(60)).await;
                        }
                    }
                    Err(e) => {
                        error!("Background face scan failed: {}", e);
                        tokio::time::sleep(Duration::from_secs(60)).await;
                    }
                }
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
