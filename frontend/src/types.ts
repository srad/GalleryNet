export interface TagDetail {
    name: string;
    is_auto: boolean;
    confidence?: number;
}

export interface TagCount {
    name: string;
    count: number;
}

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
    tags?: TagDetail[];
    faces?: Face[];
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

export interface Face {
    id: string;
    media_id: string;
    box_x1: number;
    box_y1: number;
    box_x2: number;
    box_y2: number;
    cluster_id?: number;
}

export interface PersonSummary {
    cluster_id: number;
    representative_media: MediaItem;
    representative_face: Face;
}

export interface Stats {

    version: string;
    total_files: number;
    total_images: number;
    total_videos: number;
    total_size_bytes: number;
    disk_free_bytes: number;
    disk_total_bytes: number;
}
