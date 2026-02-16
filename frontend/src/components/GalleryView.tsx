import { useState, useEffect, useRef, useCallback, Fragment } from 'react';
import { useParams, useSearchParams } from 'react-router-dom';
import type { MediaItem, MediaFilter, Folder, MediaGroup } from '../types';
import { PhotoIcon, UploadIcon, PlusIcon, HeartIcon, TagIcon, LogoutIcon } from './Icons';
import MediaCard from './MediaCard';
import MediaModal from './MediaModal';
import TagFilter from './TagFilter';
import TagInput from './TagInput';
import { apiFetch, fireUnauthorized } from '../auth';


import ConfirmDialog from './ConfirmDialog';
import AlertDialog from './AlertDialog';
import LoadingIndicator from './LoadingIndicator';

interface LibraryPickerProps {
    onPick: (ids: string[]) => void;
    onCancel: () => void;
    refreshKey: number;
    folders: Folder[];
    onFoldersChanged: () => void;
    singleSelect?: boolean;
}

export function LibraryPicker({ onPick, onCancel, refreshKey, folders, onFoldersChanged, singleSelect }: LibraryPickerProps) {
    const [filter, setFilter] = useState<MediaFilter>('all');
    return (
        <div className="fixed inset-0 z-50 bg-gray-50 flex flex-col">
            <div className="flex items-center justify-between px-4 sm:px-8 py-3 sm:py-4 bg-white border-b border-gray-200">
                <h2 className="text-lg sm:text-xl font-bold">
                    {singleSelect ? 'Select Reference Image' : 'Select Media to Add'}
                </h2>
                <button onClick={onCancel} className="p-2 hover:bg-gray-100 rounded-full" title="Close">
                    <svg className="w-6 h-6 text-gray-500" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor">
                        <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
                    </svg>
                </button>
            </div>
            <main className="flex-1 overflow-y-auto">
                <GalleryView
                    filter={filter}
                    onFilterChange={setFilter}
                    refreshKey={refreshKey}
                    folders={folders}
                    onFoldersChanged={onFoldersChanged}
                    isPicker={true}
                    onPick={onPick}
                    onCancelPick={onCancel}
                    singleSelect={singleSelect}
                />
            </main>
        </div>
    );
}

interface GalleryViewProps {
    filter: MediaFilter;
    onFilterChange: (filter: MediaFilter) => void;
    /** Incremented externally (e.g. after upload) to signal a refresh */
    refreshKey: number;
    /** If set, show media from this folder instead of the main gallery */
    folderId?: string;
    folderName?: string;
    onBackToGallery?: () => void;
    folders: Folder[];
    onFoldersChanged: () => void;
    /** Called after a successful upload */
    onUploadComplete?: () => void;
    /** Called when group computation starts/stops — parent should lock navigation */
    onBusyChange?: (busy: boolean) => void;
    /** Picker mode props */
    isPicker?: boolean;
    onPick?: (ids: string[]) => void;
    onCancelPick?: () => void;
    onFindSimilar?: (id: string) => void;
    favoritesOnly?: boolean;
    singleSelect?: boolean;
    onLogout?: () => void;
}

type SortOrder = 'desc' | 'asc';

const PAGE_SIZE = 60;

function formatBytes(bytes: number): string {
    if (bytes === 0) return '0 B';
    const units = ['B', 'KB', 'MB', 'GB', 'TB'];
    const i = Math.floor(Math.log(bytes) / Math.log(1024));
    const val = bytes / Math.pow(1024, i);
    return `${val.toFixed(i > 0 ? 1 : 0)} ${units[i]}`;
}

const FILTERS: { value: MediaFilter; label: string }[] = [
    { value: 'all', label: 'All' },
    { value: 'image', label: 'Photos' },
    { value: 'video', label: 'Videos' },
];

export default function GalleryView({ filter, onFilterChange, refreshKey, folderId, folderName, onBackToGallery, folders, onFoldersChanged, onUploadComplete, onBusyChange, isPicker, onPick, onCancelPick, onFindSimilar, favoritesOnly, singleSelect, onLogout }: GalleryViewProps) {
    const { folderId: routeFolderId } = useParams();
    const [searchParams, setSearchParams] = useSearchParams();
    const activeFolderId = folderId || routeFolderId;
    
    // Determine folder name if not provided
    const activeFolderName = folderName || folders.find(f => f.id === activeFolderId)?.name;

    const selectedMediaId = searchParams.get('media');

    const [media, setMedia] = useState<MediaItem[]>([]);
    const [standaloneItem, setStandaloneItem] = useState<MediaItem | null>(null); // For deep linking to items not in current page
    const [hasMore, setHasMore] = useState(true);
    const [isLoading, setIsLoading] = useState(false);
    const [initialLoad, setInitialLoad] = useState(true);
    const [sortOrder, setSortOrder] = useState<SortOrder>(() => {
        const saved = localStorage.getItem('gallerySortOrder');
        return saved === 'asc' ? 'asc' : 'desc';
    });
    const [viewFavorites, setViewFavorites] = useState(favoritesOnly || false);
    const [filterTags, setFilterTags] = useState<string[]>([]);
    // Removed local selectedFilename state in favor of URL param
    const [showBatchTagModal, setShowBatchTagModal] = useState(false);
    const [showClearTagsConfirm, setShowClearTagsConfirm] = useState(false);
    const [batchTags, setBatchTags] = useState<string[]>([]);
    const [isBatchTagging, setIsBatchTagging] = useState(false);
    const [isAutoTagging, setIsAutoTagging] = useState(false);
    const [autoTagProgress, setAutoTagProgress] = useState<{ current: number; total: number; label: string } | null>(null);
    const abortControllerRef = useRef<AbortController | null>(null);
    const [showCancelAutoTagConfirm, setShowCancelAutoTagConfirm] = useState(false);
    const [showStartAutoTagConfirm, setShowStartAutoTagConfirm] = useState(false);
    const [showBatchDeleteConfirm, setShowBatchDeleteConfirm] = useState(false);
    const [showRemoveFromFolderConfirm, setShowRemoveFromFolderConfirm] = useState(false);
    const [autoTagResult, setAutoTagResult] = useState<{ title: string; message: string } | null>(null);


    // --- Grouping state ---
    const [isGrouped, setIsGrouped] = useState(false);
    const [groups, setGroups] = useState<MediaGroup[]>([]);
    const [groupSimilarity, setGroupSimilarity] = useState(70);

    // --- Selection state ---
    const [selectionMode, setSelectionMode] = useState(false);
    const [selected, setSelected] = useState<Set<string>>(new Set());
    const [showPicker, setShowPicker] = useState(false);
    const [isDeleting, setIsDeleting] = useState(false);
    const [isDownloading, setIsDownloading] = useState(false);
    /** Download progress: received bytes so far, and total (if known from Content-Length) */
    const [downloadProgress, setDownloadProgress] = useState<{ received: number; total: number | null } | null>(null);
    // For shift-click range selection
    const lastClickedRef = useRef<string | null>(null);

    // --- Add-to-folder picker state ---
    const [showFolderPicker, setShowFolderPicker] = useState(false);
    const [isAddingToFolder, setIsAddingToFolder] = useState(false);

    // True while the server is computing similarity groups or processing batch actions
    const isBusy = (isGrouped && isLoading) || isBatchTagging || isDeleting || isAddingToFolder || isAutoTagging;

    // Notify parent when the busy state changes (for locking navigation)
    const prevBusyRef = useRef(false);
    useEffect(() => {
        if (prevBusyRef.current !== isBusy) {
            prevBusyRef.current = isBusy;
            onBusyChange?.(isBusy);
        }
    }, [isBusy, onBusyChange]);

    // Prevent accidental page close while busy
    useEffect(() => {
        if (!isBusy) return;
        const handler = (e: BeforeUnloadEvent) => { e.preventDefault(); };
        window.addEventListener('beforeunload', handler);
        return () => window.removeEventListener('beforeunload', handler);
    }, [isBusy]);

    // --- Marquee (rubber-band) selection state ---
    const [marquee, setMarquee] = useState<{ x: number; y: number; w: number; h: number } | null>(null);
    const marqueeStartRef = useRef<{ x: number; y: number; scrollTop: number } | null>(null);
    const isDraggingRef = useRef(false);
    /** Selection snapshot taken at drag start — used for additive marquee (Shift/Ctrl held) */
    const preDragSelectionRef = useRef<Set<string>>(new Set());
    const gridRef = useRef<HTMLDivElement>(null);

    // --- Inline upload state ---
    const fileInputRef = useRef<HTMLInputElement>(null);
    const [uploadState, setUploadState] = useState<{
        total: number; done: number; skipped: number; failed: number; active: boolean;
        errors: { filename: string; reason: string }[];
    } | null>(null);
    const uploadActiveRef = useRef(0);
    const uploadQueueRef = useRef<File[]>([]);
    const UPLOAD_CONCURRENCY = 3;

    const sentinelRef = useRef<HTMLDivElement>(null);
    const scrollContainerRef = useRef<HTMLDivElement>(null);

    // Tracks the current fetch to avoid race conditions
    const fetchIdRef = useRef(0);
    // Use a ref for page to avoid stale closures in loadMore / IntersectionObserver
    const pageRef = useRef(1);
    const isLoadingRef = useRef(false);
    const hasMoreRef = useRef(true);
    const sortOrderRef = useRef<SortOrder>(sortOrder);
    const prevRefreshKeyRef = useRef(refreshKey);
    const mediaRef = useRef<MediaItem[]>(media);
    mediaRef.current = media;

    // --- Exit selection mode helper ---
    const exitSelectionMode = useCallback(() => {
        setSelectionMode(false);
        setSelected(new Set());
        setShowFolderPicker(false);
        lastClickedRef.current = null;
    }, []);

    const handleDragStartMedia = useCallback((item: MediaItem, e: React.DragEvent) => {
        // Cancel any active marquee selection to prevent it from getting stuck during drag
        marqueeStartRef.current = null;
        isDraggingRef.current = false;
        setMarquee(null);

        let ids: string[] = [];
        if (selected.has(item.filename)) {
            // Dragging one of the selected items - drag all selected
            ids = mediaRef.current
                .filter(m => selected.has(m.filename) && m.id)
                .map(m => m.id!);
        } else if (item.id) {
            // Dragging an unselected item - drag just that one
            ids = [item.id];
        }

        if (ids.length > 0) {
            e.dataTransfer.setData('application/x-gallerynet-media', JSON.stringify(ids));
            e.dataTransfer.effectAllowed = 'copyMove';
            e.dataTransfer.setData('text/plain', `Media: ${ids.length} items`);
        }
    }, [selected]);

    // --- Inline upload helpers ---

    const processNextUpload = useCallback(() => {
        while (uploadActiveRef.current < UPLOAD_CONCURRENCY && uploadQueueRef.current.length > 0) {
            const file = uploadQueueRef.current.shift()!;
            uploadActiveRef.current++;

            const formData = new FormData();
            formData.append('file', file);
            const xhr = new XMLHttpRequest();

            xhr.addEventListener('load', () => {
                uploadActiveRef.current--;
                if (xhr.status >= 200 && xhr.status < 300) {
                    // Add to folder if in folder view
                    try {
                        const json = JSON.parse(xhr.responseText);
                        if (json.id && activeFolderId) {
                            apiFetch(`/api/folders/${activeFolderId}/media`, {
                                method: 'POST',
                                headers: { 'Content-Type': 'application/json' },
                                body: JSON.stringify([json.id]),
                            }).catch(() => {});
                        }
                    } catch { /* ignore */ }
                    setUploadState(prev => prev ? { ...prev, done: prev.done + 1 } : prev);
                    onUploadComplete?.();
                } else if (xhr.status === 401) {
                    fireUnauthorized();
                } else if (xhr.status === 409) {
                    setUploadState(prev => prev ? { ...prev, skipped: prev.skipped + 1 } : prev);
                } else {
                    let reason = `HTTP ${xhr.status}`;
                    try {
                        const json = JSON.parse(xhr.responseText);
                        if (json.error) reason = json.error;
                    } catch { /* ignore */ }
                    setUploadState(prev => prev ? {
                        ...prev,
                        failed: prev.failed + 1,
                        errors: [...prev.errors, { filename: file.name, reason }],
                    } : prev);
                }
                // Check if all done
                if (uploadActiveRef.current === 0 && uploadQueueRef.current.length === 0) {
                    setUploadState(prev => prev ? { ...prev, active: false } : prev);
                }
                processNextUpload();
            });

            xhr.addEventListener('error', () => {
                uploadActiveRef.current--;
                setUploadState(prev => prev ? {
                    ...prev,
                    failed: prev.failed + 1,
                    errors: [...prev.errors, { filename: file.name, reason: 'Network error' }],
                } : prev);
                if (uploadActiveRef.current === 0 && uploadQueueRef.current.length === 0) {
                    setUploadState(prev => prev ? { ...prev, active: false } : prev);
                }
                processNextUpload();
            });

            xhr.open('POST', '/api/upload');
            xhr.send(formData);
        }
    }, [activeFolderId, onUploadComplete]);

    const handleUpload = useCallback((files: FileList) => {
        if (files.length === 0) return;
        const fileArray = Array.from(files);
        uploadQueueRef.current.push(...fileArray);
        setUploadState(prev => ({
            total: (prev?.total ?? 0) + fileArray.length,
            done: prev?.done ?? 0,
            skipped: prev?.skipped ?? 0,
            failed: prev?.failed ?? 0,
            active: true,
            errors: prev?.errors ?? [],
        }));
        processNextUpload();
    }, [processNextUpload]);

    const clearUploadState = useCallback(() => {
        setUploadState(null);
    }, []);

    // --- Fetch groups ---
    const fetchGroups = useCallback(async (similarityOverride?: number) => {
        setIsLoading(true);
        isLoadingRef.current = true;
        try {
            const body = {
                folder_id: activeFolderId || null,
                similarity: similarityOverride ?? groupSimilarity,
            };
            const res = await apiFetch('/api/media/group', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify(body),
            });
            if (res.ok) {
                setGroups(await res.json());
            }
        } catch (e) {
            console.error('Failed to fetch groups:', e);
        } finally {
            setIsLoading(false);
            isLoadingRef.current = false;
            setInitialLoad(false);
        }
    }, [activeFolderId, groupSimilarity]);

    // --- Fetch groups when the slider is released (not while dragging) ---
    const commitGroupSimilarity = useCallback(() => {
        if (!isGrouped) return;
        fetchGroups();
    }, [isGrouped, fetchGroups]);

    // --- Build fetch URL based on folder context ---
    const buildUrl = useCallback((pageNum: number, currentFilter: MediaFilter, currentSort: SortOrder): string => {
        const params = new URLSearchParams({
            page: String(pageNum),
            limit: String(PAGE_SIZE),
            sort: currentSort,
        });
        if (currentFilter !== 'all') {
            params.set('media_type', currentFilter);
        }
        if (viewFavorites) {
            params.set('favorite', 'true');
        }
        if (filterTags.length > 0) {
            params.set('tags', filterTags.join(','));
        }
        if (activeFolderId) {
            return `/api/folders/${activeFolderId}/media?${params}`;
        }
        return `/api/media?${params}`;
    }, [activeFolderId, viewFavorites, filterTags]);

    // --- Fetch a single page ---
    const fetchPage = useCallback(async (pageNum: number, currentFilter: MediaFilter, currentSort: SortOrder, append: boolean) => {
        const id = ++fetchIdRef.current;
        setIsLoading(true);
        isLoadingRef.current = true;

        try {
            const url = buildUrl(pageNum, currentFilter, currentSort);
            const res = await apiFetch(url);
            if (!res.ok || id !== fetchIdRef.current) return;

            const results: MediaItem[] = await res.json();
            if (id !== fetchIdRef.current) return; // stale

            if (append) {
                setMedia(prev => [...prev, ...results]);
            } else {
                setMedia(results);
            }
            const more = results.length >= PAGE_SIZE;
            setHasMore(more);
            hasMoreRef.current = more;
        } catch (e) {
            console.error('Failed to fetch media:', e);
        } finally {
            if (id === fetchIdRef.current) {
                setIsLoading(false);
                isLoadingRef.current = false;
                setInitialLoad(false);
            }
        }
    }, [buildUrl]);

    // --- Silently merge new items and refresh metadata when refreshKey changes ---
    const mergeNewItems = useCallback(async (currentFilter: MediaFilter, currentSort: SortOrder) => {
        const id = ++fetchIdRef.current;
        const currentCount = Math.max(PAGE_SIZE, mediaRef.current.length);

        try {
            // Fetch enough items to cover everything currently loaded, ensuring metadata sync
            const params = new URLSearchParams({
                page: '1',
                limit: String(currentCount),
                sort: currentSort,
            });
            if (currentFilter !== 'all') params.set('media_type', currentFilter);
            if (viewFavorites) params.set('favorite', 'true');
            if (filterTags.length > 0) params.set('tags', filterTags.join(','));
            
            const url = activeFolderId 
                ? `/api/folders/${activeFolderId}/media?${params}`
                : `/api/media?${params}`;

            const res = await apiFetch(url);
            if (!res.ok || id !== fetchIdRef.current) return;

            const freshItems: MediaItem[] = await res.json();
            if (id !== fetchIdRef.current) return;

            setMedia(prev => {
                // Map for quick lookup of fresh items by filename
                const freshMap = new Map(freshItems.map(m => [m.filename, m]));
                
                // 1. Update metadata for existing items
                const updated = prev.map(item => {
                    const fresh = freshMap.get(item.filename);
                    if (fresh) {
                        return { 
                            ...item, 
                            tags: fresh.tags, 
                            is_favorite: fresh.is_favorite,
                            size_bytes: fresh.size_bytes,
                            original_filename: fresh.original_filename
                        };
                    }
                    return item;
                });

                // 2. Add truly new items (that appeared since last fetch, e.g. new uploads)
                const existingFilenames = new Set(prev.map(m => m.filename));
                const reallyNew = freshItems.filter(m => !existingFilenames.has(m.filename));

                if (reallyNew.length === 0) return updated;

                if (currentSort === 'desc') {
                    return [...reallyNew, ...updated];
                } else {
                    return [...updated, ...reallyNew];
                }
            });
        } catch (e) {
            console.error('Failed to merge/refresh media:', e);
        }
    }, [activeFolderId, viewFavorites, filterTags]);

    // --- Full reset when filter, sortOrder, or grouping mode changes ---
    const fetchGroupsRef = useRef(fetchGroups);
    fetchGroupsRef.current = fetchGroups;

    useEffect(() => {
        if (selectedMediaId && !media.find(m => m.id === selectedMediaId) && 
            (!isGrouped || !groups.some(g => g.items.some(m => m.id === selectedMediaId)))) {
            
            apiFetch(`/api/media/${selectedMediaId}`)
                .then(res => res.ok ? res.json() : null)
                .then(data => {
                    if (data) setStandaloneItem(data);
                })
                .catch(() => {});
        } else {
            setStandaloneItem(null);
        }
    }, [selectedMediaId, media, isGrouped, groups]);

    useEffect(() => {
        if (isGrouped) {
            setMedia([]); // Clear flat list
            fetchGroupsRef.current();
            return;
        }

        sortOrderRef.current = sortOrder;
        setMedia([]);
        pageRef.current = 1;
        setHasMore(true);
        hasMoreRef.current = true;
        setInitialLoad(true);
        exitSelectionMode();
        fetchPage(1, filter, sortOrder, false);
    }, [filter, sortOrder, fetchPage, exitSelectionMode, isGrouped, viewFavorites, filterTags]);

    // --- Picker mode auto-select ---
    useEffect(() => {
        if (isPicker) {
            setSelectionMode(true);
        }
    }, [isPicker]);

    // --- Toggle favorite handler ---
    const handleToggleFavorite = useCallback(async (item: MediaItem) => {
        if (!item.id) return;
        const newStatus = !item.is_favorite;
        
        // Optimistic update
        setMedia(prev => prev.map(m => m.id === item.id ? { ...m, is_favorite: newStatus } : m));
        
        try {
            await apiFetch(`/api/media/${item.id}/favorite`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ favorite: newStatus }),
            });
        } catch (e) {
            console.error('Toggle favorite error:', e);
            // Revert
            setMedia(prev => prev.map(m => m.id === item.id ? { ...m, is_favorite: !newStatus } : m));
        }
    }, []);

    const handleBatchTag = useCallback(async (force = false) => {
        if (!batchTags.length && !force) {
            setShowClearTagsConfirm(true);
            return;
        }
        
        const ids = media
            .filter(m => selected.has(m.filename) && m.id)
            .map(m => m.id!);
            
        if (ids.length === 0) return;

        setIsBatchTagging(true);
        try {
            await apiFetch('/api/media/batch-tags', {
                method: 'PUT',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ ids, tags: batchTags }),
            });
            
            // Optimistic update for immediate feedback
            const newTagDetails = batchTags.map(name => ({ name, is_auto: false }));
            setMedia(prev => prev.map(m => {
                if (m.id && ids.includes(m.id)) {
                    return { ...m, tags: newTagDetails };
                }
                return m;
            }));

            setShowBatchTagModal(false);
            setShowClearTagsConfirm(false);
            setBatchTags([]);
            exitSelectionMode();
            // Refresh via refreshKey ensures full sync (including is_auto from other processes)
            onUploadComplete?.();
        } catch (e) {
            console.error('Batch tag error', e);
            alert('Failed to update tags');
        } finally {
            setIsBatchTagging(false);
        }
    }, [batchTags, media, selected, exitSelectionMode, onUploadComplete]);



    const handleAutoTag = useCallback(async () => {
        setIsAutoTagging(true);
        const ac = new AbortController();
        abortControllerRef.current = ac;
        setAutoTagProgress({ current: 0, total: 0, label: 'Initializing...' });
        
        try {
            // 1. Get initial count
            const countParams = new URLSearchParams();
            if (activeFolderId) countParams.set('folder_id', activeFolderId);
            const initialRes = await apiFetch(`/api/tags/count?${countParams}`, { signal: ac.signal });
            const initialCount = initialRes.ok ? (await initialRes.json()).count : 0;

            if (ac.signal.aborted) return;

            // 2. Get models to process
            const modelsRes = await apiFetch('/api/tags/models', { signal: ac.signal });
            if (!modelsRes.ok) throw new Error('Failed to fetch trained models');
            const models: [number, string][] = await modelsRes.json();
            
            if (models.length === 0) {
                setAutoTagResult({
                    title: "No Tag Models Found",
                    message: "No auto-taggable tags found. Please manually tag at least 3 items with the same name first so the AI can learn what they look like!"
                });
                return;
            }

            if (ac.signal.aborted) return;

            setAutoTagProgress({ current: 0, total: models.length, label: `Processing ${models[0][1]}...` });

            const errors: string[] = [];
            
            // 3. Process models one by one for progress feedback
            for (let i = 0; i < models.length; i++) {
                if (ac.signal.aborted) break;

                const [tagId, tagName] = models[i];
                setAutoTagProgress({ current: i, total: models.length, label: `Applying "${tagName}"...` });
                
                try {
                    const res = await apiFetch(`/api/tags/${tagId}/apply`, {
                        method: 'POST',
                        headers: { 'Content-Type': 'application/json' },
                        body: JSON.stringify({ folder_id: activeFolderId || null }),
                        signal: ac.signal,
                    });
                    
                    if (!res.ok) {
                        const err = await res.json();
                        errors.push(`${tagName}: ${err.error || 'Unknown error'}`);
                    }
                } catch (e: any) {
                    if (e.name === 'AbortError') throw e;
                    errors.push(`${tagName}: Network error or crash`);
                }
            }

            if (ac.signal.aborted) return;

            setAutoTagProgress({ current: models.length, total: models.length, label: 'Finalizing...' });
            
            // 4. Get final count
            const finalRes = await apiFetch(`/api/tags/count?${countParams}`, { signal: ac.signal });
            const finalCount = finalRes.ok ? (await finalRes.json()).count : 0;

            let message = `Total auto-tags before: ${initialCount}\nTotal auto-tags after: ${finalCount}\nChange: ${finalCount >= initialCount ? '+' : ''}${finalCount - initialCount}`;
            
            if (errors.length > 0) {
                message += `\n\nSome tags encountered issues:\n• ${errors.join('\n• ')}`;
            }
            
            setAutoTagResult({
                title: "Auto-Tagging Complete",
                message
            });
            onUploadComplete?.(); // Refresh metadata
        } catch (e: any) {
            if (e.name === 'AbortError') {
                console.log('Auto-tagging cancelled');
            } else {
                console.error('Auto-tag error', e);
                setAutoTagResult({
                    title: "Error",
                    message: "Auto-tagging failed. Please check the console for details."
                });
            }
        } finally {
            if (abortControllerRef.current === ac) {
                setIsAutoTagging(false);
                setAutoTagProgress(null);
                abortControllerRef.current = null;
            }
        }
    }, [activeFolderId, onUploadComplete]);

    // --- Handle items picked from library (when acting as picker parent) ---
    const handlePickItems = useCallback(async (ids: string[]) => {
        if (!activeFolderId || ids.length === 0) return;
        try {
            await apiFetch(`/api/folders/${activeFolderId}/media`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify(ids),
            });
            setShowPicker(false);
            onUploadComplete?.(); // Trigger refresh
        } catch (e) {
            console.error('Pick items error:', e);
        }
    }, [activeFolderId, onUploadComplete]);

    // --- Gentle merge when refreshKey changes (uploads) ---
    useEffect(() => {
        if (prevRefreshKeyRef.current === refreshKey) return;
        prevRefreshKeyRef.current = refreshKey;
        if (isGrouped) {
            fetchGroups();
        } else {
            mergeNewItems(filter, sortOrderRef.current);
        }
    }, [refreshKey, filter, mergeNewItems, isGrouped, fetchGroups]);

    // --- Load next page ---
    const loadMore = useCallback(() => {
        if (isGrouped) return; // No pagination for groups yet
        if (isLoadingRef.current || !hasMoreRef.current) return;
        const nextPage = pageRef.current + 1;
        pageRef.current = nextPage;
        fetchPage(nextPage, filter, sortOrderRef.current, true);
    }, [filter, fetchPage]);

    // --- IntersectionObserver on sentinel ---
    useEffect(() => {
        const sentinel = sentinelRef.current;
        if (!sentinel) return;

        const observer = new IntersectionObserver(
            (entries) => {
                if (entries[0].isIntersecting) {
                    loadMore();
                }
            },
            {
                root: null, // use viewport — the actual scroll container is a parent <main>
                rootMargin: '400px', // trigger 400px before sentinel is visible
                threshold: 0,
            },
        );

        observer.observe(sentinel);
        return () => observer.disconnect();
    }, [loadMore]);

    // --- Selection handlers ---
    const handleSelect = useCallback((filename: string, e: React.MouseEvent) => {
        const currentMedia = mediaRef.current;
        setSelected(prev => {
            if (singleSelect) {
                const next = new Set<string>();
                next.add(filename);
                lastClickedRef.current = filename;
                return next;
            }

            const next = new Set(prev);

            if (e.shiftKey && lastClickedRef.current) {
                // Range select: select everything between lastClicked and current
                const lastIdx = currentMedia.findIndex(m => m.filename === lastClickedRef.current);
                const curIdx = currentMedia.findIndex(m => m.filename === filename);
                if (lastIdx !== -1 && curIdx !== -1) {
                    const start = Math.min(lastIdx, curIdx);
                    const end = Math.max(lastIdx, curIdx);
                    for (let i = start; i <= end; i++) {
                        next.add(currentMedia[i].filename);
                    }
                }
            } else {
                // Toggle single item
                if (next.has(filename)) {
                    next.delete(filename);
                } else {
                    next.add(filename);
                }
            }

            lastClickedRef.current = filename;
            return next;
        });
    }, []); // Stable callback

    const handleCardClick = useCallback((item: MediaItem) => {
        if (item.id) {
            setSearchParams(prev => {
                const next = new URLSearchParams(prev);
                next.set('media', item.id!);
                return next;
            });
        }
    }, [setSearchParams]);

    const handleSelectAll = useCallback(() => {
        const currentMedia = mediaRef.current;
        if (selected.size === currentMedia.length) {
            // Deselect all
            setSelected(new Set());
        } else {
            // Select all currently loaded
            setSelected(new Set(currentMedia.map(m => m.filename)));
        }
    }, [selected.size]);

    // --- Batch delete ---
    const handleBatchDelete = useCallback(async (force = false) => {
        if (selected.size === 0) return;
        if (!force) {
            setShowBatchDeleteConfirm(true);
            return;
        }

        setIsDeleting(true);
        try {
            // Resolve filenames -> IDs
            const ids = media
                .filter(m => selected.has(m.filename) && m.id)
                .map(m => m.id!);

            const res = await apiFetch('/api/media/batch-delete', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify(ids),
            });

            if (res.ok) {
                // Remove deleted items from local state
                const deletedFilenames = new Set(selected);
                setMedia(prev => prev.filter(m => !deletedFilenames.has(m.filename)));
                exitSelectionMode();
                setShowBatchDeleteConfirm(false);
                if (activeFolderId) onFoldersChanged();
            } else {
                console.error('Batch delete failed:', res.status);
            }
        } catch (e) {
            console.error('Batch delete error:', e);
        } finally {
            setIsDeleting(false);
        }
    }, [selected, media, exitSelectionMode, activeFolderId, onFoldersChanged]);

    const handleSingleDelete = useCallback(async (item: MediaItem) => {
        if (!item.id) return;
        setIsDeleting(true);
        try {
            const res = await apiFetch(`/api/media/${item.id}`, {
                method: 'DELETE',
            });

            if (res.ok) {
                setMedia(prev => prev.filter(m => m.id !== item.id));
                setSearchParams(prev => {
                    const next = new URLSearchParams(prev);
                    next.delete('media');
                    return next;
                });
                if (activeFolderId) onFoldersChanged();
            }
        } catch (e) {
            console.error('Delete error:', e);
        } finally {
            setIsDeleting(false);
        }
    }, [activeFolderId, onFoldersChanged, setSearchParams]);

    // --- Remove from folder (when viewing a folder) ---
    const handleRemoveFromFolder = useCallback(async (force = false) => {
        if (selected.size === 0 || !activeFolderId) return;
        if (!force) {
            setShowRemoveFromFolderConfirm(true);
            return;
        }

        setIsDeleting(true);
        try {
            const ids = media
                .filter(m => selected.has(m.filename) && m.id)
                .map(m => m.id!);

            const res = await apiFetch(`/api/folders/${activeFolderId}/media/remove`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ media_ids: ids }),
            });

            if (res.ok) {
                const removedFilenames = new Set(selected);
                setMedia(prev => prev.filter(m => !removedFilenames.has(m.filename)));
                exitSelectionMode();
                setShowRemoveFromFolderConfirm(false);
                onFoldersChanged();
            }
        } catch (e) {
            console.error('Remove from folder error:', e);
        } finally {
            setIsDeleting(false);
        }
    }, [selected, media, activeFolderId, exitSelectionMode, onFoldersChanged]);


    // --- Add selected items to a folder ---
    const handleAddToFolder = useCallback(async (targetFolderId: string) => {
        if (selected.size === 0) return;
        setIsAddingToFolder(true);
        try {
            const ids = media
                .filter(m => selected.has(m.filename) && m.id)
                .map(m => m.id!);

            const res = await apiFetch(`/api/folders/${targetFolderId}/media`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify(ids),
            });

            if (res.ok) {
                setShowFolderPicker(false);
                exitSelectionMode();
                onFoldersChanged();
            }
        } catch (e) {
            console.error('Add to folder error:', e);
        } finally {
            setIsAddingToFolder(false);
        }
    }, [selected, media, exitSelectionMode, onFoldersChanged]);

    // --- Helper: trigger browser download from a blob ---
    const triggerDownload = useCallback((blob: Blob, headers: Headers) => {
        const url = URL.createObjectURL(blob);
        const a = document.createElement('a');
        a.href = url;
        const disposition = headers.get('Content-Disposition');
        let downloadName = 'gallerynet-download.zip';
        if (disposition) {
            const match = disposition.match(/filename="?([^"]+)"?/);
            if (match) downloadName = match[1];
        }
        a.download = downloadName;
        document.body.appendChild(a);
        a.click();
        document.body.removeChild(a);
        URL.revokeObjectURL(url);
    }, []);

    // --- Folder download ---
    const handleDownloadFolder = useCallback(async () => {
        if (!activeFolderId) return;

        setIsDownloading(true);
        setDownloadProgress({ received: 0, total: null });
        try {
            const res = await apiFetch(`/api/folders/${activeFolderId}/download`);

            if (!res.ok) {
                console.error('Folder download failed:', res.status);
                return;
            }

            // Stream the response to track progress
            const contentLength = res.headers.get('Content-Length');
            const total = contentLength ? parseInt(contentLength, 10) : null;
            setDownloadProgress({ received: 0, total });

            const reader = res.body?.getReader();
            if (!reader) {
                const blob = await res.blob();
                triggerDownload(blob, res.headers);
                return;
            }

            const chunks: Uint8Array[] = [];
            let received = 0;

            while (true) {
                const { done, value } = await reader.read();
                if (done) break;
                chunks.push(value);
                received += value.length;
                setDownloadProgress({ received, total });
            }

            const blob = new Blob(chunks as BlobPart[]);
            triggerDownload(blob, res.headers);
        } catch (e) {
            console.error('Download error:', e);
        } finally {
            setIsDownloading(false);
            setDownloadProgress(null);
        }
    }, [activeFolderId, triggerDownload]);

    // --- Batch download ---
    const handleBatchDownload = useCallback(async () => {
        if (selected.size === 0) return;

        setIsDownloading(true);
        setDownloadProgress({ received: 0, total: null });
        try {
            // Resolve filenames -> IDs
            const ids = media
                .filter(m => selected.has(m.filename) && m.id)
                .map(m => m.id!);

            const res = await apiFetch('/api/media/download', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify(ids),
            });

            if (!res.ok) {
                console.error('Download failed:', res.status);
                return;
            }

            // Stream the response to track progress
            const contentLength = res.headers.get('Content-Length');
            const total = contentLength ? parseInt(contentLength, 10) : null;
            setDownloadProgress({ received: 0, total });

            const reader = res.body?.getReader();
            if (!reader) {
                // Fallback: no streaming support
                const blob = await res.blob();
                triggerDownload(blob, res.headers);
                exitSelectionMode();
                return;
            }

            const chunks: Uint8Array[] = [];
            let received = 0;

            while (true) {
                const { done, value } = await reader.read();
                if (done) break;
                chunks.push(value);
                received += value.length;
                setDownloadProgress({ received, total });
            }

            const blob = new Blob(chunks as BlobPart[]);
            triggerDownload(blob, res.headers);
            exitSelectionMode();
        } catch (e) {
            console.error('Download error:', e);
        } finally {
            setIsDownloading(false);
            setDownloadProgress(null);
        }
    }, [selected, media, triggerDownload, exitSelectionMode]);

    // --- Escape key to exit selection mode ---
    useEffect(() => {
        if (!selectionMode) return;
        const handler = (e: KeyboardEvent) => {
            if (e.key === 'Escape') exitSelectionMode();
        };
        document.addEventListener('keydown', handler);
        return () => document.removeEventListener('keydown', handler);
    }, [selectionMode, exitSelectionMode]);

    // --- Close folder picker when clicking outside ---
    useEffect(() => {
        if (!showFolderPicker) return;
        const handler = (e: MouseEvent) => {
            const target = e.target as HTMLElement;
            if (!target.closest('[data-folder-picker]')) {
                setShowFolderPicker(false);
            }
        };
        document.addEventListener('mousedown', handler);
        return () => document.removeEventListener('mousedown', handler);
    }, [showFolderPicker]);

    // --- Marquee selection helpers ---

    /** Get the scrollable parent (<main> element) */
    const getScrollParent = useCallback((): HTMLElement | null => {
        return scrollContainerRef.current?.closest('main') ?? null;
    }, []);

    /** Compute which cards intersect a rectangle (in page coordinates relative to the grid's offset parent) */
    const getCardsInRect = useCallback((rect: { x: number; y: number; w: number; h: number }): Set<string> => {
        const grid = gridRef.current;
        if (!grid) return new Set();

        const result = new Set<string>();
        const cards = grid.querySelectorAll<HTMLElement>('[data-filename]');
        const scrollParent = getScrollParent();
        const scrollLeft = scrollParent?.scrollLeft ?? 0;
        const scrollTop = scrollParent?.scrollTop ?? 0;

        // Marquee rect in absolute page coordinates
        const mLeft = rect.x;
        const mTop = rect.y;
        const mRight = rect.x + rect.w;
        const mBottom = rect.y + rect.h;

        for (const card of cards) {
            const cr = card.getBoundingClientRect();
            // Convert card's viewport rect to page coords (accounting for scroll)
            const cLeft = cr.left + scrollLeft;
            const cTop = cr.top + scrollTop;
            const cRight = cr.right + scrollLeft;
            const cBottom = cr.bottom + scrollTop;

            // AABB intersection test
            if (mLeft < cRight && mRight > cLeft && mTop < cBottom && mBottom > cTop) {
                const fn = card.getAttribute('data-filename');
                if (fn) result.add(fn);
            }
        }
        return result;
    }, [getScrollParent]);

    // Marquee: mousedown on the grid (but not on a card button directly when not in selection mode)
    const handleGridMouseDown = useCallback((e: React.MouseEvent) => {
        // Only left button
        if (e.button !== 0 || singleSelect) return;
        // Don't start marquee if clicking directly on a card's interactive element
        const target = e.target as HTMLElement;
        if (target.closest('[data-filename]')) return;

        // Enter selection mode automatically when starting a marquee
        if (!selectionMode) setSelectionMode(true);


        const scrollParent = getScrollParent();
        const scrollLeft = scrollParent?.scrollLeft ?? 0;
        const scrollTop = scrollParent?.scrollTop ?? 0;

        // Store the start position in absolute page coordinates
        const startX = e.clientX + scrollLeft;
        const startY = e.clientY + scrollTop;

        marqueeStartRef.current = { x: startX, y: startY, scrollTop };
        isDraggingRef.current = false;
        // Snapshot current selection so additive drag (Shift/Ctrl) can merge with it
        preDragSelectionRef.current = new Set(selected);
        // Don't set marquee yet — wait until the mouse moves enough (deadzone)
    }, [selectionMode, getScrollParent, selected]);

    // Marquee: mousemove & mouseup — attach to document so drag works even outside the grid
    useEffect(() => {
        const handleMouseMove = (e: MouseEvent) => {
            const start = marqueeStartRef.current;
            if (!start) return;

            const scrollParent = getScrollParent();
            const scrollLeft = scrollParent?.scrollLeft ?? 0;
            const scrollTop = scrollParent?.scrollTop ?? 0;

            const curX = e.clientX + scrollLeft;
            const curY = e.clientY + scrollTop;

            // Deadzone: require at least 5px of movement before showing the marquee
            if (!isDraggingRef.current) {
                const dx = curX - start.x;
                const dy = curY - start.y;
                if (Math.abs(dx) < 5 && Math.abs(dy) < 5) return;
                isDraggingRef.current = true;
            }

            const x = Math.min(start.x, curX);
            const y = Math.min(start.y, curY);
            const w = Math.abs(curX - start.x);
            const h = Math.abs(curY - start.y);

            setMarquee({ x, y, w, h });

            // Live-update selection to show which items are being selected
            const underMarquee = getCardsInRect({ x, y, w, h });
            const base = preDragSelectionRef.current;
            if ((e.shiftKey || e.ctrlKey || e.metaKey) && base.size > 0) {
                // Additive: merge marquee hits with the pre-drag selection
                const next = new Set(base);
                for (const fn of underMarquee) next.add(fn);
                setSelected(next);
            } else {
                setSelected(underMarquee);
            }
        };

        const handleMouseUp = () => {
            if (marqueeStartRef.current && !isDraggingRef.current) {
                // Was a click, not a drag — don't change selection
            }
            marqueeStartRef.current = null;
            isDraggingRef.current = false;
            setMarquee(null);
        };

        document.addEventListener('mousemove', handleMouseMove);
        document.addEventListener('mouseup', handleMouseUp);
        return () => {
            document.removeEventListener('mousemove', handleMouseMove);
            document.removeEventListener('mouseup', handleMouseUp);
        };
    }, [getScrollParent, getCardsInRect]);

    // Keyboard shortcuts (Ctrl+A to select all, Delete to remove/delete)
    useEffect(() => {
        const handleKeyDown = (e: KeyboardEvent) => {
            // Check if the user is typing in an input/textarea
            const target = e.target as HTMLElement;
            if (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.isContentEditable) {
                return;
            }

            // Ctrl+A or Cmd+A (Select All)
            if ((e.ctrlKey || e.metaKey) && e.key === 'a') {
                e.preventDefault();
                if (media.length > 0) {
                    if (!selectionMode) setSelectionMode(true);
                    const allFilenames = new Set(media.map(m => m.filename));
                    setSelected(allFilenames);
                }
            }

            // Delete key
            if (e.key === 'Delete' || e.key === 'Backspace') {
                if (selectionMode && selected.size > 0) {
                    e.preventDefault();
                    if (activeFolderId) {
                        handleRemoveFromFolder();
                    } else {
                        handleBatchDelete();
                    }
                }
            }
        };

        window.addEventListener('keydown', handleKeyDown);
        return () => window.removeEventListener('keydown', handleKeyDown);
    }, [media, selectionMode, selected, activeFolderId, handleRemoveFromFolder, handleBatchDelete]);

    return (

        <div ref={scrollContainerRef} className="max-w-7xl mx-auto px-4 md:px-8 pb-12">
            {/* Modern Toolbar */}
            <div 
                id="gallery-toolbar"
                className="sticky top-0 z-30 -mx-4 px-4 md:-mx-8 md:px-8 py-4 mb-6
                           bg-white/80 backdrop-blur-xl border-b border-gray-200/60 shadow-sm
                           transition-all duration-300 flex flex-col gap-4"
            >
                {/* Top Bar: Title & Main Actions */}
                <div className="flex flex-col md:flex-row md:items-center justify-between gap-4">
                    {/* Left: Navigation & Title */}
                    <div className="flex items-center gap-3 min-w-0">
                        {activeFolderId && onBackToGallery && (
                            <button
                                onClick={onBackToGallery}
                                disabled={isBusy}
                                className="group p-2 -ml-2 rounded-full hover:bg-gray-100 text-gray-500 hover:text-gray-900 transition-all disabled:opacity-50"
                                title="Back to gallery"
                            >
                                <svg className="w-5 h-5 transition-transform group-hover:-translate-x-0.5" fill="none" viewBox="0 0 24 24" strokeWidth={2.5} stroke="currentColor">
                                    <path strokeLinecap="round" strokeLinejoin="round" d="M10.5 19.5L3 12m0 0l7.5-7.5M3 12h18" />
                                </svg>
                            </button>
                        )}
                        <h2 className="text-2xl md:text-3xl font-bold tracking-tight text-gray-900 truncate">
                            {activeFolderId 
                                ? (activeFolderName || 'Loading...') 
                                : (favoritesOnly ? 'Favorites' : 'Gallery')}
                        </h2>
                    </div>

                    {/* Right: Primary Controls */}
                    <div className="flex flex-wrap items-center gap-2 sm:gap-3">
                        {/* 1. Filter Segmented Control */}
                        <div className={`flex p-1 bg-gray-100/80 rounded-xl border border-gray-200/50 ${isBusy ? 'opacity-50 pointer-events-none' : ''}`}>
                            {FILTERS.map(({ value, label }) => (
                                <button
                                    key={value}
                                    onClick={() => onFilterChange(value)}
                                    disabled={isBusy}
                                    className={`
                                        px-3 py-1.5 text-xs sm:text-sm font-semibold rounded-lg transition-all duration-200
                                        ${filter === value
                                            ? 'bg-white text-gray-900 shadow-sm ring-1 ring-black/5'
                                            : 'text-gray-500 hover:text-gray-700 hover:bg-gray-200/50'
                                        }
                                    `}
                                >
                                    {label}
                                </button>
                            ))}
                        </div>

                        <div className="w-px h-6 bg-gray-200 mx-1 hidden sm:block"></div>

                        {/* 2. Upload & Logout (Primary Actions) */}
                        {!isPicker && (
                            <div className="flex items-center gap-2">
                                <div className="relative">
                                    <button
                                        onClick={() => fileInputRef.current?.click()}
                                        disabled={uploadState?.active || isBusy}
                                        className="flex items-center gap-2 px-4 py-2 text-sm font-semibold text-white bg-gray-900 rounded-xl shadow-sm hover:bg-gray-800 hover:shadow-md active:scale-95 transition-all disabled:opacity-70 disabled:pointer-events-none disabled:active:scale-100"
                                        title={activeFolderId ? "Upload files to this folder" : "Upload media"}
                                    >
                                        <UploadIcon />
                                        <span className="hidden sm:inline">Upload</span>
                                    </button>
                                    <input
                                        ref={fileInputRef}
                                        type="file"
                                        accept="image/*,video/*"
                                        multiple
                                        className="hidden"
                                        onChange={(e) => {
                                            if (e.target.files && e.target.files.length > 0) {
                                                handleUpload(e.target.files);
                                                e.target.value = '';
                                            }
                                        }}
                                    />
                                </div>

                                {onLogout && (
                                    <button
                                        onClick={onLogout}
                                        className="flex items-center gap-2 px-3 py-2 text-sm font-semibold text-gray-500 hover:text-gray-900 hover:bg-gray-100 rounded-xl transition-all active:scale-95"
                                        title="Log out"
                                    >
                                        <LogoutIcon />
                                    </button>
                                )}
                            </div>
                        )}
                    </div>
                </div>

                {/* Bottom Bar: Filters & View Options */}
                <div className="flex flex-wrap items-center justify-between gap-3">
                    {/* Left: Filters & Search */}
                    <div className="flex flex-wrap items-center gap-2">
                         <TagFilter selectedTags={filterTags} onChange={setFilterTags} refreshKey={refreshKey} />
                         
                         {/* Favorites Toggle */}
                        {!favoritesOnly && (
                            <button
                                onClick={() => setViewFavorites(v => !v)}
                                disabled={isBusy}
                                className={`
                                    group flex items-center gap-2 px-3 py-1.5 text-sm font-medium rounded-lg border transition-all
                                    ${viewFavorites
                                        ? 'bg-red-50 text-red-600 border-red-200 hover:bg-red-100'
                                        : 'bg-white text-gray-600 border-gray-200 hover:border-gray-300 hover:bg-gray-50'
                                    }
                                `}
                                title={viewFavorites ? "Show all items" : "Show favorites only"}
                            >
                                <HeartIcon solid={viewFavorites} />
                                <span className="hidden sm:inline">Favorites</span>
                            </button>
                        )}
                    </div>

                    {/* Right: View Options */}
                    <div className="flex flex-wrap items-center gap-2">
                        {/* Sort */}
                        <button
                            onClick={() => setSortOrder(s => {
                                const next = s === 'desc' ? 'asc' : 'desc';
                                localStorage.setItem('gallerySortOrder', next);
                                return next;
                            })}
                            disabled={isBusy}
                            className="flex items-center gap-2 px-3 py-1.5 text-sm font-medium text-gray-600 bg-white border border-gray-200 rounded-lg hover:bg-gray-50 hover:border-gray-300 transition-all active:scale-95"
                            title={sortOrder === 'desc' ? 'Newest first' : 'Oldest first'}
                        >
                             <svg className="w-4 h-4 text-gray-400" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor">
                                {sortOrder === 'desc' ? (
                                    <path strokeLinecap="round" strokeLinejoin="round" d="M3 4.5h14.25M3 9h9.75M3 13.5h5.25m5.25-.75L17.25 9m0 0L21 12.75M17.25 9v12" />
                                ) : (
                                    <path strokeLinecap="round" strokeLinejoin="round" d="M3 4.5h14.25M3 9h9.75M3 13.5h9.75m4.5-4.5v12m0 0l-3.75-3.75M17.25 21L21 17.25" />
                                )}
                            </svg>
                            <span className="hidden sm:inline">{sortOrder === 'desc' ? 'Newest' : 'Oldest'}</span>
                        </button>

                        <div className="w-px h-5 bg-gray-200 mx-1 hidden sm:block"></div>

                        {/* Grouping */}
                        <div className="flex items-center gap-2">
                            <button
                                onClick={() => setIsGrouped(g => !g)}
                                disabled={isBusy}
                                className={`
                                    flex items-center gap-2 px-3 py-1.5 text-sm font-medium rounded-lg border transition-all active:scale-95
                                    ${isGrouped
                                        ? 'bg-purple-50 text-purple-700 border-purple-200 hover:bg-purple-100'
                                        : 'bg-white text-gray-600 border-gray-200 hover:border-gray-300 hover:bg-gray-50'
                                    }
                                `}
                                title="Group similar images"
                            >
                                <svg className={`w-4 h-4 ${isGrouped ? 'text-purple-600' : 'text-gray-400'}`} fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor">
                                    <path strokeLinecap="round" strokeLinejoin="round" d="M3.75 6A2.25 2.25 0 016 3.75h2.25A2.25 2.25 0 0110.5 6v2.25a2.25 2.25 0 01-2.25 2.25H6a2.25 2.25 0 01-2.25-2.25V6zM3.75 15.75A2.25 2.25 0 016 13.5h2.25a2.25 2.25 0 012.25 2.25V18a2.25 2.25 0 01-2.25 2.25H6A2.25 2.25 0 013.75 18v-2.25zM13.5 6a2.25 2.25 0 012.25-2.25H18A2.25 2.25 0 0120.25 6v2.25A2.25 2.25 0 0118 10.5h-2.25a2.25 2.25 0 01-2.25-2.25V6zM13.5 15.75a2.25 2.25 0 012.25-2.25H18a2.25 2.25 0 012.25 2.25V18A2.25 2.25 0 0118 20.25h-2.25A2.25 2.25 0 0113.5 18v-2.25z" />
                                </svg>
                                <span className="hidden sm:inline">Group</span>
                            </button>

                            {isGrouped && (
                                <div className="flex items-center gap-3 bg-white border border-gray-200 rounded-lg px-3 py-1.5 shadow-sm animate-in fade-in slide-in-from-left-2 duration-200">
                                    <span className="text-xs font-semibold text-gray-500 whitespace-nowrap">
                                        Similarity: <span className="text-purple-600">{groupSimilarity}%</span>
                                    </span>
                                    <input
                                        type="range"
                                        min="50"
                                        max="99"
                                        value={groupSimilarity}
                                        onChange={(e) => setGroupSimilarity(Number(e.target.value))}
                                        onPointerUp={commitGroupSimilarity}
                                        onKeyUp={(e) => { if (e.key === 'ArrowLeft' || e.key === 'ArrowRight') commitGroupSimilarity(); }}
                                        disabled={isBusy}
                                        className="w-24 h-1.5 bg-gray-100 rounded-lg appearance-none cursor-pointer accent-purple-600 hover:accent-purple-700"
                                        title="Adjust similarity grouping threshold"
                                    />
                                </div>
                            )}
                        </div>

                        {/* Auto Tag */}
                        {!isPicker && (
                            <button
                                onClick={isAutoTagging ? () => setShowCancelAutoTagConfirm(true) : () => setShowStartAutoTagConfirm(true)}
                                disabled={isBusy && !isAutoTagging}
                                className={`
                                    flex items-center gap-2 px-3 py-1.5 text-sm font-medium rounded-lg border transition-all active:scale-95
                                    ${isAutoTagging
                                        ? 'bg-amber-50 text-amber-700 border-amber-200 hover:bg-amber-100'
                                        : 'bg-white text-gray-600 border-gray-200 hover:border-gray-300 hover:bg-gray-50'
                                    }
                                `}
                                title={isAutoTagging ? "Cancel auto-tagging" : "Auto-tag current view using learned models"}
                            >
                                {isAutoTagging ? (
                                    <LoadingIndicator size="sm" color="text-amber-700" />
                                ) : (
                                    <svg className="w-4 h-4 text-amber-500" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor">
                                        <path strokeLinecap="round" strokeLinejoin="round" d="M9.813 15.904L9 18.75l-.813-2.846a4.5 4.5 0 00-3.09-3.09L2.25 12l2.846-.813a4.5 4.5 0 003.09-3.09L9 5.25l.813 2.846a4.5 4.5 0 003.09 3.09L15.75 12l-2.846.813a4.5 4.5 0 00-3.09 3.09z" />
                                    </svg>
                                )}
                                <span className="hidden sm:inline">{isAutoTagging ? 'Cancel' : 'Auto Tag'}</span>
                            </button>
                        )}

                        {/* Select Toggle */}
                        {media.length > 0 && !isGrouped && (
                            <button
                                onClick={() => selectionMode ? exitSelectionMode() : setSelectionMode(true)}
                                className={`
                                    flex items-center gap-2 px-3 py-1.5 text-sm font-medium rounded-lg border transition-all active:scale-95
                                    ${selectionMode
                                        ? 'bg-blue-50 text-blue-700 border-blue-200 hover:bg-blue-100'
                                        : 'bg-white text-gray-600 border-gray-200 hover:border-gray-300 hover:bg-gray-50'
                                    }
                                `}
                                title={selectionMode ? 'Exit selection' : 'Select items'}
                            >
                                <svg className={`w-4 h-4 ${selectionMode ? 'text-blue-600' : 'text-gray-400'}`} fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor">
                                    <path strokeLinecap="round" strokeLinejoin="round" d="M9 12.75L11.25 15 15 9.75M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
                                </svg>
                                <span className="hidden sm:inline">{selectionMode ? 'Done' : 'Select'}</span>
                            </button>
                        )}
                        
                        {/* Folder View Specific Actions */}
                        {activeFolderId && !isPicker && (
                            <>
                                <div className="w-px h-5 bg-gray-200 mx-1 hidden sm:block"></div>
                                
                                <button
                                    onClick={() => setShowPicker(true)}
                                    disabled={isBusy}
                                    className="flex items-center gap-2 px-3 py-1.5 text-sm font-medium text-gray-600 bg-white border border-gray-200 rounded-lg hover:bg-gray-50 hover:border-gray-300 transition-all active:scale-95"
                                    title="Add existing media from library"
                                >
                                    <PlusIcon />
                                    <span className="hidden sm:inline">Add</span>
                                </button>

                                {media.length > 0 && (
                                    <button
                                        onClick={handleDownloadFolder}
                                        disabled={isDownloading || isBusy}
                                        className="flex items-center gap-2 px-3 py-1.5 text-sm font-medium text-gray-600 bg-white border border-gray-200 rounded-lg hover:bg-gray-50 hover:border-gray-300 transition-all active:scale-95"
                                        title="Download all items in folder"
                                    >
                                        <svg className="w-4 h-4 text-gray-400" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor">
                                            <path strokeLinecap="round" strokeLinejoin="round" d="M3 16.5v2.25A2.25 2.25 0 005.25 21h13.5A2.25 2.25 0 0021 18.75V16.5M16.5 12L12 16.5m0 0L7.5 12m4.5 4.5V3" />
                                        </svg>
                                        <span className="hidden xl:inline">Download All</span>
                                    </button>
                                )}
                            </>
                        )}
                    </div>
                </div>


            {/* Auto-tag progress bar */}
            {autoTagProgress && (
                <div className="mb-4 bg-indigo-50 border border-indigo-100 rounded-xl shadow-sm p-3">
                    <div className="flex items-center justify-between mb-1.5">
                        <LoadingIndicator 
                            size="sm" 
                            color="text-indigo-600" 
                            label={autoTagProgress.label}
                            className="text-xs font-semibold text-indigo-900"
                        />
                        <div className="flex items-center gap-2">
                            <span className="text-[10px] font-bold text-indigo-400 uppercase tracking-wider">
                                {autoTagProgress.total > 0 ? Math.round((autoTagProgress.current / autoTagProgress.total) * 100) : 0}%
                            </span>
                        </div>
                    </div>
                    <div className="w-full bg-indigo-100/50 rounded-full h-1.5 overflow-hidden">
                        <div
                            className="h-1.5 bg-indigo-600 rounded-full transition-all duration-500 ease-out"
                            style={{ width: `${autoTagProgress.total > 0 ? (autoTagProgress.current / autoTagProgress.total) * 100 : 0}%` }}
                        />
                    </div>
                </div>
            )}

            {/* Upload progress bar */}
            {uploadState && (
                <div className="mb-4 bg-white border border-gray-200 rounded-xl shadow-sm p-3">
                    <div className="flex items-center justify-between mb-1.5">
                        <div className="text-xs text-gray-600">
                            {uploadState.active ? (
                                <span>
                                    Uploading {uploadState.done + uploadState.skipped + uploadState.failed}/{uploadState.total}
                                </span>
                            ) : (
                                <span>
                                    {uploadState.done} uploaded
                                    {uploadState.skipped > 0 ? `, ${uploadState.skipped} skipped` : ''}
                                    {uploadState.failed > 0 ? `, ${uploadState.failed} failed` : ''}
                                </span>
                            )}
                        </div>
                        {!uploadState.active && (
                            <button onClick={clearUploadState} className="text-[11px] text-gray-400 hover:text-gray-600 transition-colors">
                                Dismiss
                            </button>
                        )}
                    </div>
                    <div className="w-full bg-gray-100 rounded-full h-1.5 overflow-hidden">
                        <div
                            className={`h-1.5 rounded-full transition-all duration-300 ease-out ${
                                uploadState.active ? 'bg-blue-500' : uploadState.failed > 0 ? 'bg-amber-500' : 'bg-green-500'
                            }`}
                            style={{ width: `${uploadState.total > 0 ? Math.round(((uploadState.done + uploadState.skipped + uploadState.failed) / uploadState.total) * 100) : 0}%` }}
                        />
                    </div>
                    {uploadState.errors.length > 0 && (
                        <ul className="mt-2 space-y-1">
                            {uploadState.errors.map((err, i) => (
                                <li key={i} className="flex items-start gap-1.5 text-xs text-red-600">
                                    <svg className="w-3.5 h-3.5 flex-shrink-0 mt-0.5" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor">
                                        <path strokeLinecap="round" strokeLinejoin="round" d="M12 9v3.75m9-.75a9 9 0 11-18 0 9 9 0 0118 0zm-9 3.75h.008v.008H12v-.008z" />
                                    </svg>
                                    <span><span className="font-medium">{err.filename}</span> &mdash; {err.reason}</span>
                                </li>
                            ))}
                        </ul>
                    )}
                </div>
            )}
            </div>

            {/* Empty state — only after the initial load finishes with 0 results */}
            {!initialLoad && !isGrouped && media.length === 0 && (
                <div className="flex flex-col items-center justify-center py-20 bg-white rounded-2xl border border-dashed border-gray-300">
                    <PhotoIcon />
                    <p className="mt-4 text-gray-500">
                        {activeFolderId
                            ? 'This folder is empty.'
                            : filter === 'all' ? 'No media uploaded yet.' : `No ${filter}s found.`}
                    </p>
                    <button onClick={() => fileInputRef.current?.click()} className="mt-4 text-blue-600 hover:underline">
                        {activeFolderId ? 'Upload files to this folder' : 'Upload your first image'}
                    </button>
                </div>
            )}

            {/* Grid */}
            {!isGrouped && media.length > 0 && (
                <div
                    ref={gridRef}
                    onMouseDown={handleGridMouseDown}
                    className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-6 xl:grid-cols-8 gap-0.5 select-none"
                >
                    {media.map((item, index) => {
                        const date = new Date(item.original_date);
                        const year = isNaN(date.getTime()) ? 'Unknown' : date.getFullYear();
                        
                        let showHeader = false;
                        if (index === 0) {
                            showHeader = true;
                        } else {
                            const prevDate = new Date(media[index - 1].original_date);
                            const prevYear = isNaN(prevDate.getTime()) ? 'Unknown' : prevDate.getFullYear();
                            showHeader = year !== prevYear;
                        }

                        return (
                            <Fragment key={item.filename}>
                                {showHeader && (
                                    <div className="col-span-full pt-4 pb-2 px-1 first:pt-0">
                                        <h3 className="text-xl font-bold text-gray-500/50 select-text">
                                            {year}
                                        </h3>
                                    </div>
                                )}
                                <MediaCard
                                    item={item}
                                    onClick={() => handleCardClick(item)}
                                    selected={selected.has(item.filename)}
                                    selectionMode={selectionMode}
                                    onSelect={(e) => handleSelect(item.filename, e)}
                                    onToggleFavorite={() => handleToggleFavorite(item)}
                                    onDragStart={(e) => handleDragStartMedia(item, e)}
                                />

                            </Fragment>
                        );
                    })}
                </div>
            )}

            {/* Grouped View */}
            {isGrouped && (
                <div className="relative space-y-4 pb-12">
                    {/* Loading overlay when computing similarity groups */}
                    {isLoading && (
                        <LoadingIndicator 
                            variant="overlay" 
                            label="Computing similarity groups..." 
                            color="text-purple-600" 
                            className="pt-32 bg-gray-50/80"
                        />
                    )}
                    {groups.length === 0 && !isLoading && (
                        <div className="flex flex-col items-center justify-center py-20 bg-white rounded-2xl border border-dashed border-gray-300">
                            <p className="text-gray-500">No similar groups found.</p>
                        </div>
                    )}
                    {groups.map((group) => (
                        <div key={group.id} className="bg-white border border-gray-200 rounded-xl p-4 shadow-sm">
                            <div className="flex items-center gap-2 mb-3">
                                <span className="text-sm font-semibold text-gray-700 bg-gray-100 px-2.5 py-1 rounded-md">
                                    Group {group.id + 1}
                                </span>
                                <span className="text-xs text-gray-500">
                                    {group.items.length} items
                                </span>
                            </div>
                            <div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-5 lg:grid-cols-7 gap-0.5">
                                {group.items.map((item) => (
                                    <MediaCard
                        key={item.filename}
                        item={item}
                        onClick={() => handleCardClick(item)}
                        selected={selected.has(item.filename)}
                        selectionMode={selectionMode}
                        onSelect={(e) => handleSelect(item.filename, e)}
                        onToggleFavorite={() => handleToggleFavorite(item)}
                        onDragStart={(e) => handleDragStartMedia(item, e)}
                    />

                ))}
            </div>
                        </div>
                    ))}
                </div>
            )}

            {/* Sentinel for IntersectionObserver */}
            {hasMore && !isGrouped && <div ref={sentinelRef} className="h-1" />}

            {/* Loading indicator (flat list pagination only — grouped view has its own overlay) */}
            {isLoading && !isGrouped && (
                <div className="flex justify-center py-8">
                    <LoadingIndicator 
                        label="Loading..." 
                        color="text-gray-400" 
                        className="text-sm"
                    />
                </div>
            )}

            {/* End of list */}
            {!hasMore && media.length > 0 && !isLoading && (
                <p className="text-center text-xs text-gray-400 py-6">
                    {activeFolderId ? 'End of folder' : 'End of gallery'}
                </p>
            )}

            {/* Marquee selection rectangle */}
            {marquee && (() => {
                const scrollParent = getScrollParent();
                const scrollLeft = scrollParent?.scrollLeft ?? 0;
                const scrollTop = scrollParent?.scrollTop ?? 0;
                return (
                    <div
                        className="fixed pointer-events-none z-30 border-2 border-blue-500 bg-blue-500/10"
                        style={{
                            left: marquee.x - scrollLeft,
                            top: marquee.y - scrollTop,
                            width: marquee.w,
                            height: marquee.h,
                        }}
                    />
                );
            })()}

            {/* Floating selection toolbar */}
            {selectionMode && selected.size > 0 && (
                <div className="fixed bottom-4 sm:bottom-6 left-2 right-2 sm:left-1/2 sm:right-auto sm:-translate-x-1/2 z-40 flex flex-wrap items-center justify-center gap-2 sm:gap-3 px-3 sm:px-5 py-2.5 sm:py-3 bg-gray-900 text-white rounded-2xl shadow-2xl">
                    <span className="text-xs sm:text-sm font-medium mr-1">
                        {selected.size} selected
                    </span>

                    {/* Select all / deselect all */}
                    {!singleSelect && (
                        <button
                            onClick={handleSelectAll}
                            className="px-2.5 sm:px-3 py-1.5 text-xs font-medium rounded-lg bg-white/15 hover:bg-white/25 transition-colors"
                        >
                            {selected.size === media.length ? 'Deselect all' : 'Select all'}
                        </button>
                    )}

                    {isPicker ? (
                        /* Picker actions */
                        <>
                            <button
                                onClick={() => {
                                    const ids = media
                                        .filter(m => selected.has(m.filename) && m.id)
                                        .map(m => m.id!);
                                    onPick?.(ids);
                                }}
                                className="flex items-center gap-1.5 px-2.5 sm:px-3 py-1.5 text-xs font-medium rounded-lg bg-blue-600 hover:bg-blue-700 transition-colors"
                            >
                                <PlusIcon />
                                <span className="hidden sm:inline">
                                    {singleSelect ? 'Select Image' : 'Add selection'}
                                </span>
                                <span className="sm:hidden">
                                    {singleSelect ? 'Select' : 'Add'}
                                </span>
                            </button>
                            <button
                                onClick={onCancelPick}
                                className="ml-1 p-1.5 rounded-lg hover:bg-white/15 transition-colors"
                                title="Cancel"
                            >
                                <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor">
                                    <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
                                </svg>
                            </button>
                        </>
                    ) : (
                        /* Normal actions */
                        <>
                            {/* Batch Tags */}
                            <button
                                onClick={() => setShowBatchTagModal(true)}
                                className="flex items-center gap-1.5 px-2.5 sm:px-3 py-1.5 text-xs font-medium rounded-lg bg-indigo-600 hover:bg-indigo-700 transition-colors"
                            >
                                <TagIcon />
                                <span className="hidden sm:inline">Tags</span>
                            </button>

                            {/* Add to folder */}
                    <div className="relative" data-folder-picker>
                        <div
                            onClick={() => !isAddingToFolder && setShowFolderPicker(prev => !prev)}
                            className={`flex items-center gap-1.5 px-2.5 sm:px-3 py-1.5 text-xs font-medium rounded-lg bg-amber-600 hover:bg-amber-700 transition-colors cursor-pointer ${isAddingToFolder ? 'opacity-50 pointer-events-none' : ''}`}
                        >
                            <svg className="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor">
                                <path strokeLinecap="round" strokeLinejoin="round" d="M12 10.5v6m3-3H9m4.06-7.19l-2.12-2.12a1.5 1.5 0 00-1.061-.44H4.5A2.25 2.25 0 002.25 6v12a2.25 2.25 0 002.25 2.25h15A2.25 2.25 0 0021.75 18V9a2.25 2.25 0 00-2.25-2.25h-5.379a1.5 1.5 0 01-1.06-.44z" />
                            </svg>
                            <span className="hidden sm:inline">{isAddingToFolder ? 'Adding...' : 'Add to folder'}</span>
                        </div>

                        {/* Folder picker dropdown */}
                        {showFolderPicker && (
                            <div className="absolute bottom-full mb-2 left-1/2 -translate-x-1/2 sm:left-0 sm:translate-x-0 bg-white text-gray-800 rounded-xl shadow-2xl border border-gray-200 py-1 min-w-[180px] max-h-60 overflow-y-auto">
                                {folders.length === 0 ? (
                                    <p className="px-3 py-2 text-xs text-gray-400">No folders yet</p>
                                ) : (
                                    folders.map(f => (
                                        <button
                                            key={f.id}
                                            onClick={() => handleAddToFolder(f.id)}
                                            className="w-full flex items-center gap-2 px-3 py-2 text-sm hover:bg-gray-100 transition-colors text-left"
                                        >
                                            <svg className="w-4 h-4 text-gray-400 flex-shrink-0" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor">
                                                <path strokeLinecap="round" strokeLinejoin="round" d="M2.25 12.75V12A2.25 2.25 0 014.5 9.75h15A2.25 2.25 0 0121.75 12v.75m-8.69-6.44l-2.12-2.12a1.5 1.5 0 00-1.061-.44H4.5A2.25 2.25 0 002.25 6v12a2.25 2.25 0 002.25 2.25h15A2.25 2.25 0 0021.75 18V9a2.25 2.25 0 00-2.25-2.25h-5.379a1.5 1.5 0 01-1.06-.44z" />
                                            </svg>
                                            <span className="truncate">{f.name}</span>
                                            <span className="text-[10px] text-gray-400 ml-auto flex-shrink-0">{f.item_count}</span>
                                        </button>
                                    ))
                                )}
                            </div>
                        )}
                    </div>

                    {/* Remove from folder (only when viewing a folder) */}
                    {activeFolderId && (
                                <button
                                    onClick={() => handleRemoveFromFolder()}
                                    disabled={isDeleting}
                                    className="flex items-center gap-1.5 px-2.5 sm:px-3 py-1.5 text-xs font-medium rounded-lg bg-orange-600 hover:bg-orange-700 disabled:opacity-50 transition-colors"
                                >

                            <svg className="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor">
                                <path strokeLinecap="round" strokeLinejoin="round" d="M15 12H9m12 0a9 9 0 11-18 0 9 9 0 0118 0z" />
                            </svg>
                            <span className="hidden sm:inline">Remove</span>
                        </button>
                    )}

                    {/* Download */}
                    <button
                        onClick={handleBatchDownload}
                        disabled={isDownloading}
                        className="flex items-center gap-1.5 px-2.5 sm:px-3 py-1.5 text-xs font-medium rounded-lg bg-blue-600 hover:bg-blue-700 disabled:opacity-50 transition-colors"
                    >
                        <svg className="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor">
                            <path strokeLinecap="round" strokeLinejoin="round" d="M3 16.5v2.25A2.25 2.25 0 005.25 21h13.5A2.25 2.25 0 0021 18.75V16.5M16.5 12L12 16.5m0 0L7.5 12m4.5 4.5V3" />
                        </svg>
                        <span className="hidden sm:inline">{isDownloading ? 'Downloading...' : 'Download'}</span>
                    </button>

                    {/* Delete */}
                    <button
                        onClick={() => handleBatchDelete()}
                        disabled={isDeleting}
                        className="flex items-center gap-1.5 px-2.5 sm:px-3 py-1.5 text-xs font-medium rounded-lg bg-red-600 hover:bg-red-700 disabled:opacity-50 transition-colors"
                    >

                        <svg className="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor">
                            <path strokeLinecap="round" strokeLinejoin="round" d="M14.74 9l-.346 9m-4.788 0L9.26 9m9.968-3.21c.342.052.682.107 1.022.166m-1.022-.165L18.16 19.673a2.25 2.25 0 01-2.244 2.077H8.084a2.25 2.25 0 01-2.244-2.077L4.772 5.79m14.456 0a48.108 48.108 0 00-3.478-.397m-12 .562c.34-.059.68-.114 1.022-.165m0 0a48.11 48.11 0 013.478-.397m7.5 0v-.916c0-1.18-.91-2.164-2.09-2.201a51.964 51.964 0 00-3.32 0c-1.18.037-2.09 1.022-2.09 2.201v.916m7.5 0a48.667 48.667 0 00-7.5 0" />
                        </svg>
                        <span className="hidden sm:inline">{isDeleting ? 'Deleting...' : 'Delete'}</span>
                    </button>

                    {/* Close toolbar */}
                    <button
                        onClick={exitSelectionMode}
                        className="ml-1 p-1.5 rounded-lg hover:bg-white/15 transition-colors"
                        title="Cancel selection"
                    >
                        <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor">
                            <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
                        </svg>
                    </button>
                    </>
                )}
                </div>
            )}

            {/* Busy overlay for batch operations */}
            {(isDeleting || isAddingToFolder) && (
                <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm">
                    <div className="bg-white rounded-2xl shadow-2xl px-8 py-6 flex flex-col items-center gap-4 mx-4">
                        <LoadingIndicator 
                            variant="centered" 
                            label={isAddingToFolder ? "Adding to folder..." : (activeFolderId ? "Removing..." : "Deleting...")} 
                            size="lg" 
                            color={isAddingToFolder ? "text-amber-600" : (activeFolderId ? "text-orange-600" : "text-red-600")} 
                        />
                    </div>
                </div>
            )}

            {/* Download progress overlay */}

            {isDownloading && downloadProgress && (
                <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm">
                    <div className="bg-white rounded-2xl shadow-2xl px-6 sm:px-8 py-5 sm:py-6 flex flex-col items-center gap-4 mx-4 w-[calc(100%-2rem)] max-w-xs sm:mx-0 sm:w-auto sm:min-w-[280px]">
                        <LoadingIndicator 
                            variant="centered" 
                            label="Preparing download..." 
                            size="lg" 
                            color="text-blue-600" 
                        />
                        {downloadProgress.total ? (
                            <>
                                <div className="w-full bg-gray-200 rounded-full h-2 overflow-hidden">
                                    <div
                                        className="bg-blue-600 h-full rounded-full transition-all duration-200"
                                        style={{ width: `${Math.min(100, (downloadProgress.received / downloadProgress.total) * 100)}%` }}
                                    />
                                </div>
                                <p className="text-xs text-gray-500">
                                    {formatBytes(downloadProgress.received)} / {formatBytes(downloadProgress.total)}
                                </p>
                            </>
                        ) : (
                            <p className="text-xs text-gray-500">
                                {formatBytes(downloadProgress.received)} received
                            </p>
                        )}
                    </div>
                </div>
            )}

            {/* Detail modal */}
            {(() => {
                if (selectedMediaId === null || selectionMode) return null;
                
                // Find item in flat list or groups
                let item: MediaItem | undefined;
                let prevItem: MediaItem | undefined;
                let nextItem: MediaItem | undefined;

                if (isGrouped) {
                    // This is inefficient but functional for now. Groups are usually small.
                    // Flattening every render is bad, but groups state doesn't change often.
                    const allItems = groups.flatMap(g => g.items);
                    const idx = allItems.findIndex(m => m.id === selectedMediaId);
                    if (idx !== -1) {
                        item = allItems[idx];
                        prevItem = allItems[idx - 1];
                        nextItem = allItems[idx + 1];
                    }
                } else {
                    const idx = media.findIndex(m => m.id === selectedMediaId);
                    if (idx !== -1) {
                        item = media[idx];
                        prevItem = media[idx - 1];
                        nextItem = media[idx + 1];
                    }
                }

                if (!item) {
                    item = standaloneItem || undefined;
                    // prevItem/nextItem remain undefined, so nav buttons will be hidden
                }

                if (!item) return null;

                const handleSetMediaId = (id: string) => {
                     setSearchParams(prev => {
                        const next = new URLSearchParams(prev);
                        next.set('media', id);
                        return next;
                    });
                };

                return (
                    <MediaModal
                        item={item}
                        onClose={() => setSearchParams(prev => { const next = new URLSearchParams(prev); next.delete('media'); return next; })}
                        onPrev={prevItem && prevItem.id ? () => handleSetMediaId(prevItem!.id!) : null}
                        onNext={nextItem && nextItem.id ? () => handleSetMediaId(nextItem!.id!) : null}
                        onFindSimilar={onFindSimilar}
                        onToggleFavorite={() => handleToggleFavorite(item!)}
                        onDelete={() => handleSingleDelete(item!)}
                        onTagsChanged={onUploadComplete}
                    />
                );
            })()}

            {/* Batch Tag Modal */}
            {showBatchTagModal && (
                <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm p-4">
                    <div className="bg-white rounded-xl shadow-xl w-full max-w-md overflow-hidden">
                        <div className="px-4 py-3 border-b border-gray-100 flex items-center justify-between">
                            <h3 className="font-semibold text-gray-900">Tag {selected.size} items</h3>
                            <button onClick={() => setShowBatchTagModal(false)} className="text-gray-400 hover:text-gray-600">
                                <svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor">
                                    <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
                                </svg>
                            </button>
                        </div>
                        <div className="p-4">
                            <p className="text-xs text-gray-500 mb-3">
                                Enter tags to apply to all selected items. This will <strong>overwrite</strong> existing tags for these items.
                            </p>
                            <TagInput
                                value={batchTags}
                                onChange={setBatchTags}
                                placeholder="Enter tags..."
                                autoFocus={true}
                            />
                        </div>
                        <div className="px-4 py-3 bg-gray-50 flex justify-end gap-2">
                            <button
                                onClick={() => setShowBatchTagModal(false)}
                                disabled={isBatchTagging}
                                className="px-3 py-1.5 text-sm font-medium text-gray-600 hover:bg-gray-100 rounded-lg transition-colors disabled:opacity-50"
                            >
                                Cancel
                            </button>
                            <button
                                onClick={() => handleBatchTag()}
                                disabled={isBatchTagging}
                                className="px-3 py-1.5 text-sm font-medium text-white bg-indigo-600 hover:bg-indigo-700 rounded-lg shadow-sm disabled:opacity-50 transition-colors flex items-center gap-1.5"
                            >

                                {isBatchTagging ? (
                                    <LoadingIndicator size="sm" color="text-white" label="Saving..." />
                                ) : 'Apply Tags'}
                            </button>
                        </div>
                    </div>
                </div>
            )}

            {/* Start Auto Tag Confirmation */}
            <ConfirmDialog
                isOpen={showStartAutoTagConfirm}
                title="Start Auto-Tagging?"
                message="This process will analyze all media items in the current view and apply tags based on learned models. It may take a significant amount of time depending on the number of items and models."
                confirmLabel="Start"
                cancelLabel="Cancel"
                isDestructive={false}
                onConfirm={() => {
                    handleAutoTag();
                    setShowStartAutoTagConfirm(false);
                }}
                onCancel={() => setShowStartAutoTagConfirm(false)}
            />

            {/* Cancel Auto Tag Confirmation */}
            <ConfirmDialog
                isOpen={showCancelAutoTagConfirm}
                title="Cancel Auto-Tagging?"
                message="Are you sure you want to stop the auto-tagging process? Progress made so far will be saved."
                confirmLabel="Yes, Stop"
                cancelLabel="Continue Tagging"
                isDestructive={true}
                onConfirm={() => {
                    abortControllerRef.current?.abort();
                    setShowCancelAutoTagConfirm(false);
                }}
                onCancel={() => setShowCancelAutoTagConfirm(false)}
            />

            <ConfirmDialog
                isOpen={showClearTagsConfirm}
                title="Clear Tags?"
                message={`This will remove all tags from the ${selected.size} selected items. This cannot be undone.`}
                confirmLabel="Clear Tags"
                cancelLabel="Cancel"
                isDestructive={true}
                onConfirm={() => handleBatchTag(true)}
                onCancel={() => setShowClearTagsConfirm(false)}
            />

            <ConfirmDialog
                isOpen={showBatchDeleteConfirm}
                title="Delete Media?"
                message={`Are you sure you want to delete ${selected.size} items? This will permanently remove the original files and thumbnails.`}
                confirmLabel="Delete Permanently"
                cancelLabel="Cancel"
                isDestructive={true}
                onConfirm={() => handleBatchDelete(true)}
                onCancel={() => setShowBatchDeleteConfirm(false)}
            />

            <ConfirmDialog
                isOpen={showRemoveFromFolderConfirm}
                title="Remove from Folder?"
                message={`Remove ${selected.size} items from "${activeFolderName}"? The media items will remain in your gallery.`}
                confirmLabel="Remove"
                cancelLabel="Cancel"
                isDestructive={true}
                onConfirm={() => handleRemoveFromFolder(true)}
                onCancel={() => setShowRemoveFromFolderConfirm(false)}
            />

            {/* Auto Tag Result Dialog */}

            <AlertDialog
                isOpen={!!autoTagResult}
                title={autoTagResult?.title || ''}
                message={autoTagResult?.message || ''}
                onClose={() => setAutoTagResult(null)}
            />

            {/* Library Picker */}
            {showPicker && activeFolderId && (
                <LibraryPicker
                    onPick={handlePickItems}
                    onCancel={() => setShowPicker(false)}
                    refreshKey={refreshKey}
                    folders={folders}
                    onFoldersChanged={onFoldersChanged}
                />
            )}
        </div>
    );
}
