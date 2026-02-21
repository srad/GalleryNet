export type MediaFilter = 'all' | 'image' | 'video';

export interface TagDetail {
    name: string;
    is_auto: boolean;
    confidence?: number;
}

export interface MediaItem {
    id: string;
    filename: string;
    original_filename: string;
    media_type: string;
    phash: string;
    uploaded_at: string;
    original_date: string;
    width?: number;
    height?: number;
    size_bytes: number;
    exif_json?: string;
    is_favorite?: boolean;
    tags?: TagDetail[];
    faces?: Face[];
    faces_scanned?: boolean;
}

export interface MediaSummary {
    id: string;
    filename: string;
    original_filename: string;
    media_type: string;
    uploaded_at: string;
    original_date: string;
    width?: number;
    height?: number;
    size_bytes: number;
    is_favorite?: boolean;
    tags?: TagDetail[];
}

export interface MediaCounts {
    total: number;
    images: number;
    videos: number;
    total_size_bytes: number;
}

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
    cluster_id: number | null;
    person_id: string | null;
}

export interface FaceStats {
    total_faces: number;
    total_people: number;
    named_people: number;
    hidden_people: number;
    unassigned_faces: number;
    ungrouped_faces: number;
}

export interface Person {

    id: string;
    name: string;
    is_hidden: boolean;
    face_count: number;
}

export type PersonWithFace = [Person, Face | null, MediaSummary | null];

export interface Stats {
    total_files: number;
    total_images: number;
    total_videos: number;
    total_size_bytes: number;
    disk_free_bytes: number;
    disk_total_bytes: number;
    version: string;
}

export interface TagCount {
    name: string;
    count: number;
}

export interface UploadProgress {
    filename: string;
    progress: number;
    status: 'uploading' | 'processing' | 'done' | 'error' | 'skipped';
    error?: string;
}
