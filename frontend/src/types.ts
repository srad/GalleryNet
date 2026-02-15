export interface MediaItem {
    id?: string;
    filename: string;
    original_filename?: string;
    media_type: string;
    uploaded_at: string;
    original_date: string;
    size_bytes?: number;
    width?: number;
    height?: number;
    exif_json?: string;
    is_favorite?: boolean;
    tags?: string[];
}

export type MediaFilter = 'all' | 'image' | 'video';

export interface Folder {
    id: string;
    name: string;
    created_at: string;
    item_count: number;
    sort_order: number;
}

export interface MediaGroup {
    id: number;
    items: MediaItem[];
}
