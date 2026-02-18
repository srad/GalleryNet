import type { MediaItem } from './types';

/**
 * Dispatches a custom 'gallerynet-media-update' event.
 * GalleryView instances listen for this to selectively update their local state.
 */
export function fireMediaUpdate(id: string, item: Partial<MediaItem>, action: 'update' | 'delete' | 'create' = 'update') {
    if (typeof window !== 'undefined' && window.dispatchEvent) {
        window.dispatchEvent(new CustomEvent('gallerynet-media-update', { 
            detail: { id, item, action } 
        }));
    }
}
