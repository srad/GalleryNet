use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, Notify};
use tracing::{info, error};
use crate::application::{FixThumbnailsUseCase, IndexFacesUseCase, GroupFacesUseCase};
use crate::presentation::WsMessage;
use serde_json;

pub struct TaskRunner {
    fix_thumbnails_use_case: Arc<FixThumbnailsUseCase>,
    index_faces_use_case: Arc<IndexFacesUseCase>,
    group_faces_use_case: Arc<GroupFacesUseCase>,
    search_use_case: Arc<crate::application::SearchSimilarUseCase>,
    repo: Arc<dyn crate::domain::MediaRepository>,
    tx: broadcast::Sender<Arc<str>>,
    wakeup_notify: Arc<Notify>,
}

impl TaskRunner {
    pub fn new(
        fix_thumbnails_use_case: Arc<FixThumbnailsUseCase>,
        index_faces_use_case: Arc<IndexFacesUseCase>,
        group_faces_use_case: Arc<GroupFacesUseCase>,
        search_use_case: Arc<crate::application::SearchSimilarUseCase>,
        repo: Arc<dyn crate::domain::MediaRepository>,
        tx: broadcast::Sender<Arc<str>>,
    ) -> Self {
        Self {
            fix_thumbnails_use_case,
            index_faces_use_case,
            group_faces_use_case,
            search_use_case,
            repo,
            tx,
            wakeup_notify: Arc::new(Notify::new()),
        }
    }

    pub fn get_wakeup_notify(&self) -> Arc<Notify> {
        self.wakeup_notify.clone()
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
        
        // Start face indexer task
        let r = runner.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(30)).await;
            
            loop {
                info!("Starting face indexer task...");
                let result = r.index_faces_use_case.execute(200).await;
                
                match result {
                    Ok(count) => {
                        if count > 0 {
                            info!("Face indexer processed {} unscanned items.", count);
                            
                            // If we processed items, there might be more. Yield and continue immediately.
                            tokio::task::yield_now().await;
                            continue;
                        } else {
                            // Backlog is empty! NOW is the time to trigger auto-clustering.
                            // This ensures we only cluster once we have all the data.
                            let r_cloned = r.clone();
                            tokio::task::spawn_blocking(move || {
                                info!("Backlog drained. Starting background auto-clustering pass...");
                                match r_cloned.group_faces_use_case.execute_sync(0.6) {
                                    Ok(_) => info!("Auto-clustering pass completed."),
                                    Err(e) => error!("Auto-clustering pass failed: {}", e),
                                }
                            });
                        }
                    }
                    Err(e) => {
                        error!("Face indexer task failed: {}", e);
                    }
                }
                
                // If we reach here, either the backlog is empty or an error occurred.
                // Wait for a wakeup signal or a periodic long timeout.
                tokio::select! {
                    _ = r.wakeup_notify.notified() => {
                        info!("Face indexer woken up by signal.");
                    }
                    _ = tokio::time::sleep(Duration::from_secs(3600)) => {
                        info!("Face indexer waking up for periodic check.");
                    }
                }
            }
        });
        
        // Start vector indexer task (find missing similarity embeddings)
        let r = runner.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(45)).await;
            loop {
                info!("Checking for media missing visual similarity embeddings...");
                if let Ok(missing) = r.repo.find_media_missing_embeddings() {
                    let count = missing.len();
                    if count > 0 {
                        info!("Found {} items missing embeddings. Re-indexing...", count);
                        for item in missing {
                            // We don't need to do anything complex, just run search extraction
                            // which updates the vector if missing.
                            let _ = r.search_use_case.reindex_item(&item).await;
                        }
                    }
                }
                tokio::time::sleep(Duration::from_secs(3600)).await; // Hourly
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
