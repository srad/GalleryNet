import { apiFetch } from './auth';
import type { MediaItem, MediaGroup, MediaFilter, Stats, TagCount, Folder } from './types';

export interface DownloadPart {
    id: string;
    filename: string;
    size_estimate: number;
}

export interface DownloadPlan {
    plan_id: string;
    parts: DownloadPart[];
}

export interface ProgressInfo {
    received: number;
    total: number | null;
    partIndex?: number;
    partCount?: number;
}

export class ApiClient {
    private getUrl(path: string): string {
        if (typeof window !== 'undefined' && window.location) {
            return path;
        }
        // Fallback for tests
        return `http://localhost:3000${path}`;
    }

    async checkAuth(): Promise<{ authenticated: boolean; required: boolean }> {
        const res = await apiFetch(this.getUrl('/api/auth-check'));
        if (!res.ok) throw new Error('Failed to check auth');
        return res.json();
    }

    async logout(): Promise<void> {
        await apiFetch(this.getUrl('/api/logout'), { method: 'POST' });
    }

    async getFolders(): Promise<Folder[]> {
        const res = await apiFetch(this.getUrl('/api/folders'));
        if (!res.ok) throw new Error('Failed to fetch folders');
        return res.json();
    }

    async getTags(): Promise<TagCount[]> {
        const res = await apiFetch(this.getUrl('/api/tags'));
        if (!res.ok) throw new Error('Failed to fetch tags');
        return res.json();
    }

    async getStats(): Promise<Stats> {
        const res = await apiFetch(this.getUrl('/api/stats'));
        if (!res.ok) throw new Error('Failed to fetch stats');
        return res.json();
    }

    async createFolder(name: string): Promise<Folder> {
        const res = await apiFetch(this.getUrl('/api/folders'), {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ name }),
        });
        if (!res.ok) throw new Error('Failed to create folder');
        return res.json();
    }


    async deleteFolder(id: string): Promise<void> {
        const res = await apiFetch(this.getUrl(`/api/folders/${id}`), {
            method: 'DELETE',
        });
        if (!res.ok) throw new Error('Failed to delete folder');
    }

    async renameFolder(id: string, name: string): Promise<void> {
        const res = await apiFetch(this.getUrl(`/api/folders/${id}`), {
            method: 'PUT',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ name }),
        });
        if (!res.ok) throw new Error('Failed to rename folder');
    }

    async reorderFolders(ids: string[]): Promise<void> {
        const res = await apiFetch(this.getUrl('/api/folders/reorder'), {
            method: 'PUT',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify(ids),
        });
        if (!res.ok) throw new Error('Failed to reorder folders');
    }

    async updateMediaTags(id: string, tags: string[]): Promise<void> {
        const res = await apiFetch(this.getUrl(`/api/media/${id}/tags`), {
            method: 'PUT',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ tags }),
        });
        if (!res.ok) throw new Error('Failed to update tags');
    }

    async getDownloadPlan(ids: string[], signal?: AbortSignal): Promise<DownloadPlan> {
        const res = await apiFetch(this.getUrl('/api/media/download/plan'), {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify(ids),
            signal
        });
        if (!res.ok) throw new Error('Failed to get download plan');
        return res.json();
    }

    async getFolderDownloadPlan(folderId: string, signal?: AbortSignal): Promise<DownloadPlan> {
        const res = await apiFetch(this.getUrl(`/api/folders/${folderId}/download`), { signal });
        if (!res.ok) throw new Error('Failed to get folder download plan');
        return res.json();
    }

    async downloadStreamPart(
        part: DownloadPart,
        partIndex: number,
        partCount: number,
        onProgress: (progress: ProgressInfo) => void,
        signal?: AbortSignal
    ): Promise<{ blob: Blob; headers: Headers }> {
        const res = await apiFetch(this.getUrl(`/api/media/download/stream/${part.id}`), { signal });
        if (!res.ok) throw new Error(`Part ${partIndex} download failed`);

        const total = res.headers.get('Content-Length') ? parseInt(res.headers.get('Content-Length')!, 10) : null;
        onProgress({ received: 0, total, partIndex, partCount });

        const reader = res.body?.getReader();
        if (!reader) {
            const blob = await res.blob();
            onProgress({ received: blob.size, total: blob.size, partIndex, partCount });
            return { blob, headers: res.headers };
        }

        const chunks: Uint8Array[] = [];
        let received = 0;
        try {
            while (true) {
                const { done, value } = await reader.read();
                if (done) break;
                chunks.push(value);
                received += value.length;
                onProgress({ received, total, partIndex, partCount });
            }
        } finally {
            reader.releaseLock();
        }

        return { blob: new Blob(chunks as BlobPart[]), headers: res.headers };
    }

    async getMedia(params: {
        page: number;
        limit: number;
        sort: 'asc' | 'desc';
        sort_by?: 'date' | 'size';
        media_type?: MediaFilter;
        favorite?: boolean;
        tags?: string[];
        folder_id?: string;
    }): Promise<MediaItem[]> {
        const query = new URLSearchParams({
            page: String(params.page),
            limit: String(params.limit),
            sort: params.sort,
        });
        if (params.sort_by && params.sort_by !== 'date') query.set('sort_by', params.sort_by);
        if (params.media_type && params.media_type !== 'all') query.set('media_type', params.media_type);
        if (params.favorite) query.set('favorite', 'true');
        if (params.tags && params.tags.length > 0) query.set('tags', params.tags.join(','));

        const url = params.folder_id 
            ? this.getUrl(`/api/folders/${params.folder_id}/media?${query}`)
            : this.getUrl(`/api/media?${query}`);

        const res = await apiFetch(url);
        if (!res.ok) throw new Error('Failed to fetch media');
        return res.json();
    }

    async getGroups(params: {
        folder_id?: string;
        similarity: number;
    }): Promise<MediaGroup[]> {
        const res = await apiFetch(this.getUrl('/api/media/group'), {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify(params),
        });
        if (!res.ok) throw new Error('Failed to fetch groups');
        return res.json();
    }

    async toggleFavorite(id: string, favorite: boolean): Promise<void> {
        const res = await apiFetch(this.getUrl(`/api/media/${id}/favorite`), {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ favorite }),
        });
        if (!res.ok) throw new Error('Failed to toggle favorite');
    }

    async deleteMediaBatch(ids: string[]): Promise<number> {
        const res = await apiFetch(this.getUrl('/api/media/batch-delete'), {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify(ids),
        });
        if (!res.ok) throw new Error('Failed to delete media');
        const data = await res.json();
        return data.deleted;
    }

    async updateMediaTagsBatch(ids: string[], tags: string[]): Promise<void> {
        const res = await apiFetch(this.getUrl('/api/media/batch-tags'), {
            method: 'PUT',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ ids, tags }),
        });
        if (!res.ok) throw new Error('Failed to update tags');
    }

    async countAutoTags(folderId?: string, signal?: AbortSignal): Promise<number> {
        const query = new URLSearchParams();
        if (folderId) query.set('folder_id', folderId);
        const res = await apiFetch(this.getUrl(`/api/tags/count?${query}`), { signal });
        if (!res.ok) throw new Error('Failed to count auto tags');
        const data = await res.json();
        return data.count;
    }

    async getTagModels(signal?: AbortSignal): Promise<[number, string][]> {
        const res = await apiFetch(this.getUrl('/api/tags/models'), { signal });
        if (!res.ok) throw new Error('Failed to fetch trained models');
        return res.json();
    }

    async applyTagModel(tagId: number, folderId?: string, signal?: AbortSignal): Promise<number> {
        const res = await apiFetch(this.getUrl(`/api/tags/${tagId}/apply`), {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ folder_id: folderId || null }),
            signal,
        });
        if (!res.ok) throw new Error('Failed to apply tag model');
        const data = await res.json();
        return data.auto_tagged_count;
    }

    async addMediaToFolder(folderId: string, mediaIds: string[]): Promise<void> {
        const res = await apiFetch(this.getUrl(`/api/folders/${folderId}/media`), {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify(mediaIds),
        });
        if (!res.ok) throw new Error('Failed to add media to folder');
    }

    async removeMediaFromFolder(folderId: string, mediaIds: string[]): Promise<void> {
        const res = await apiFetch(this.getUrl(`/api/folders/${folderId}/media/remove`), {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ media_ids: mediaIds }),
        });
        if (!res.ok) throw new Error('Failed to remove media from folder');
    }

    async deleteMedia(id: string): Promise<void> {
        const res = await apiFetch(this.getUrl(`/api/media/${id}`), {
            method: 'DELETE',
        });
        if (!res.ok) throw new Error('Failed to delete media');
    }

    async getMediaItem(id: string): Promise<MediaItem | null> {
        const res = await apiFetch(this.getUrl(`/api/media/${id}`));
        if (!res.ok) return null;
        return res.json();
    }

    async searchSimilarById(id: string, similarity: number): Promise<MediaItem[]> {
        const res = await apiFetch(this.getUrl(`/api/media/${id}/similar?similarity=${similarity}`));
        if (!res.ok) throw new Error('Search failed');
        return res.json();
    }

    async searchSimilarByFile(file: File, similarity: number): Promise<MediaItem[]> {
        const formData = new FormData();
        formData.append('similarity', similarity.toString());
        formData.append('file', file);
        const res = await apiFetch(this.getUrl('/api/search'), {
            method: 'POST',
            body: formData,
        });
        if (!res.ok) throw new Error('Search failed');
        return res.json();
    }
}

export const apiClient = new ApiClient();
