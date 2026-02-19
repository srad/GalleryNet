import { useState, useEffect, useCallback, useRef } from 'react';
import type { MediaItem } from '../types';
import { apiClient } from '../api';
import { fireMediaUpdate } from '../events';

import { HeartIcon, TagIcon, SearchIcon } from './Icons';
import TagInput from './TagInput';
import ConfirmDialog from './ConfirmDialog';
import LoadingIndicator from './LoadingIndicator';

interface MediaModalProps {
    item: MediaItem;
    onClose: () => void;
    onPrev: (() => void) | null;
    onNext: (() => void) | null;
    onFindSimilar?: (id: string) => void;
    onToggleFavorite?: () => void;
    onDelete?: () => void;
    onTagsChanged?: () => void;
}

const VIDEO_EXTENSIONS = new Set(['mp4', 'mov', 'avi', 'webm', 'mkv', 'flv', 'wmv']);

function isVideo(filename: string): boolean {
    const ext = filename.split('.').pop()?.toLowerCase() ?? '';
    return VIDEO_EXTENSIONS.has(ext);
}

function formatBytes(bytes: number): string {
    if (bytes === 0) return '0 B';
    const units = ['B', 'KB', 'MB', 'GB', 'TB'];
    const i = Math.floor(Math.log(bytes) / Math.log(1024));
    const val = bytes / Math.pow(1024, i);
    return `${val.toFixed(i > 0 ? 1 : 0)} ${units[i]}`;
}

function formatDate(dateStr: string): string {
    const d = new Date(dateStr);
    if (isNaN(d.getTime())) return dateStr;
    return d.toLocaleString();
}

export default function MediaModal({ item, onClose, onPrev, onNext, onFindSimilar, onToggleFavorite, onDelete }: MediaModalProps) {


    const backdropRef = useRef<HTMLDivElement>(null);
    const video = isVideo(item.filename);
    const mediaUrl = `/uploads/${item.filename}`;

    const [detail, setDetail] = useState<MediaItem | null>(null);
    const [exifOpen, setExifOpen] = useState(false);
    const [prevId, setPrevId] = useState(item.id);
    const videoRef = useRef<HTMLVideoElement>(null);

    // Initial volume from localStorage
    useEffect(() => {
        if (video && videoRef.current) {
            const savedVolume = localStorage.getItem('galleryVideoVolume');
            const savedMuted = localStorage.getItem('galleryVideoMuted');
            
            if (savedVolume !== null) {
                videoRef.current.volume = parseFloat(savedVolume);
            }
            if (savedMuted !== null) {
                videoRef.current.muted = savedMuted === 'true';
            }
        }
    }, [video, item.id]);

    // Reset video element when navigating between videos (Firefox fix)
    useEffect(() => {
        if (!video || !videoRef.current) return;
        
        const videoEl = videoRef.current;
        videoEl.pause();
        videoEl.removeAttribute('src');
        videoEl.src = mediaUrl;
        videoEl.load();
    }, [video, mediaUrl]);

    const handleVolumeChange = useCallback(() => {
        if (videoRef.current) {
            localStorage.setItem('galleryVideoVolume', videoRef.current.volume.toString());
            localStorage.setItem('galleryVideoMuted', videoRef.current.muted.toString());
        }
    }, []);

    // Reset state when item changes (derived state pattern)

    if (item.id !== prevId) {
        setPrevId(item.id);
        setDetail(null);
        setExifOpen(false);
    }

    // Fetch full details (including exif_json) when item changes
    useEffect(() => {
        if (!item.id) return;

        let cancelled = false;
        apiClient.getMediaItem(item.id)
            .then(data => { if (!cancelled && data) setDetail(data); })
            .catch(e => console.error('Failed to load media details:', e));
        return () => { cancelled = true; };
    }, [item.id]);

    // Use fetched detail for fields when available, fallback to the summary item
    const displayItem = detail ? { ...detail, is_favorite: item.is_favorite } : item;
    const exifData: Record<string, string> | null = (() => {
        if (!detail?.exif_json) return null;
        try {
            return JSON.parse(detail.exif_json);
        } catch {
            return null;
        }
    })();

    // Keyboard navigation
    const handleKeyDown = useCallback((e: KeyboardEvent) => {
        // Ignore if user is typing in an input/textarea
        const target = e.target as HTMLElement;
        if (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.isContentEditable) return;

        if (e.key === 'Escape') onClose();
        else if (e.key === 'ArrowLeft' && onPrev) onPrev();
        else if (e.key === 'ArrowRight' && onNext) onNext();
        else if (e.key === 'Delete' && onDelete) onDelete();
    }, [onClose, onPrev, onNext, onDelete]);

    useEffect(() => {
        document.addEventListener('keydown', handleKeyDown);
        const prev = document.body.style.overflow;
        document.body.style.overflow = 'hidden';
        return () => {
            document.removeEventListener('keydown', handleKeyDown);
            document.body.style.overflow = prev;
            // Ensure any videos in this modal instance are stopped when unmounting
            const videos = backdropRef.current?.querySelectorAll('video');
            videos?.forEach(v => {
                v.pause();
                v.src = "";
                v.load();
            });
        };
    }, [handleKeyDown]);


    // Close on backdrop click
    const handleBackdropClick = useCallback((e: React.MouseEvent) => {
        if (e.target === backdropRef.current) onClose();
    }, [onClose]);

    // Touch swipe for mobile navigation
    const touchStartRef = useRef<{ x: number; y: number } | null>(null);

    const handleTouchStart = useCallback((e: React.TouchEvent) => {
        if (e.touches.length === 1) {
            touchStartRef.current = { x: e.touches[0].clientX, y: e.touches[0].clientY };
        }
    }, []);

    const handleTouchEnd = useCallback((e: React.TouchEvent) => {
        if (!touchStartRef.current || e.changedTouches.length === 0) return;
        const dx = e.changedTouches[0].clientX - touchStartRef.current.x;
        const dy = e.changedTouches[0].clientY - touchStartRef.current.y;
        touchStartRef.current = null;

        // Only trigger if horizontal swipe is dominant and > 60px
        if (Math.abs(dx) > 60 && Math.abs(dx) > Math.abs(dy) * 1.5) {
            if (dx > 0 && onPrev) onPrev();
            else if (dx < 0 && onNext) onNext();
        }
    }, [onPrev, onNext]);

    const handleDragStart = useCallback((e: React.DragEvent) => {
        if (item.id) {
            e.dataTransfer.setData('application/x-gallerynet-media', JSON.stringify([item.id]));
            e.dataTransfer.effectAllowed = 'copyMove';
            e.dataTransfer.setData('text/plain', `Media: 1 item`);
        }
    }, [item.id]);

    const handleTagsChange = useCallback(async (newTags: string[]) => {

        if (!displayItem.id) return;
        // Optimistic update locally
        const newTagDetails = newTags.map(name => ({ name, is_auto: false }));
        setDetail(prev => prev ? { ...prev, tags: newTagDetails } : { ...item, tags: newTagDetails });
        
        try {
            await apiClient.updateMediaTags(displayItem.id, newTags);
            if (displayItem.id) {
                fireMediaUpdate(displayItem.id, { tags: newTagDetails });
            }
        } catch (e) {
            console.error('Failed to update tags', e);
        }
    }, [displayItem.id, item]);


    return (
        <div
            ref={backdropRef}
            onClick={handleBackdropClick}
            onTouchStart={handleTouchStart}
            onTouchEnd={handleTouchEnd}
            className="fixed inset-0 z-50 flex items-center justify-center bg-black/80 backdrop-blur-sm"
        >
            {/* Mobile Toolbar */}
            <div className="lg:hidden absolute top-3 left-3 z-50 flex flex-row gap-4">
                {onToggleFavorite && (
                    <button 
                        onClick={onToggleFavorite} 
                        className={`w-10 h-10 flex items-center justify-center rounded-full bg-black/40 backdrop-blur-md transition-colors ${displayItem.is_favorite ? 'text-red-500 hover:text-red-400' : 'text-white/80 hover:text-white hover:bg-black/60'}`}
                    >
                        <HeartIcon solid={displayItem.is_favorite} className="w-6 h-6" />
                    </button>
                )}
                {onFindSimilar && item.id && (
                    <button 
                        onClick={() => { if(item.id) onFindSimilar(item.id); }} 
                        className="w-10 h-10 flex items-center justify-center rounded-full bg-black/40 text-white/80 hover:text-white hover:bg-black/60 backdrop-blur-md transition-colors"
                    >
                        <SearchIcon className="w-6 h-6" />
                    </button>
                )}
                <a 
                    href={mediaUrl} 
                    target="_blank" 
                    rel="noreferrer" 
                    className="w-10 h-10 flex items-center justify-center rounded-full bg-black/40 text-white/80 hover:text-white hover:bg-black/60 backdrop-blur-md transition-colors"
                >
                    <svg className="w-6 h-6" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor">
                        <path strokeLinecap="round" strokeLinejoin="round" d="M13.5 6H5.25A2.25 2.25 0 003 8.25v10.5A2.25 2.25 0 005.25 21h10.5A2.25 2.25 0 0018 18.75V10.5m-10.5 6L21 3m0 0h-5.25M21 3v5.25" />
                    </svg>
                </a>
                {onDelete && (
                    <button 
                        onClick={onDelete} 
                        className="w-10 h-10 flex items-center justify-center rounded-full bg-black/40 text-red-200 hover:text-red-100 hover:bg-black/60 backdrop-blur-md transition-colors"
                    >
                        <svg className="w-6 h-6" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor">
                            <path strokeLinecap="round" strokeLinejoin="round" d="M14.74 9l-.346 9m-4.788 0L9.26 9m9.968-3.21c.342.052.682.107 1.022.166m-1.022-.165L18.16 19.673a2.25 2.25 0 01-2.244 2.077H8.084a2.25 2.25 0 01-2.244-2.077L4.772 5.79m14.456 0a48.108 48.108 0 00-3.478-.397m-12 .562c.34-.059.68-.114 1.022-.165m0 0a48.11 48.11 0 013.478-.397m7.5 0v-.916c0-1.18-.91-2.164-2.09-2.201a51.964 51.964 0 00-3.32 0c-1.18.037-2.09 1.022-2.09 2.201v.916m7.5 0a48.667 48.667 0 00-7.5 0" />
                        </svg>
                    </button>
                )}
            </div>

            {/* Close button */}
            <button
                onClick={onClose}
                className="absolute top-3 right-3 sm:top-4 sm:right-4 z-10 p-2.5 sm:p-2 rounded-full bg-black/40 text-white/80 hover:text-white hover:bg-black/60 transition-colors"
            >
                <svg className="w-5 h-5 sm:w-6 sm:h-6" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor">
                    <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
                </svg>
            </button>

            {/* Previous button */}
            {onPrev && (
                <button
                    onClick={onPrev}
                    className="absolute left-1 sm:left-4 top-1/2 -translate-y-1/2 z-10 p-2.5 sm:p-2 rounded-full bg-black/40 text-white/80 hover:text-white hover:bg-black/60 transition-colors"
                >
                    <svg className="w-5 h-5 sm:w-6 sm:h-6" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor">
                        <path strokeLinecap="round" strokeLinejoin="round" d="M15.75 19.5L8.25 12l7.5-7.5" />
                    </svg>
                </button>
            )}

            {/* Next button */}
            {onNext && (
                <button
                    onClick={onNext}
                    className="absolute right-1 sm:right-4 top-1/2 -translate-y-1/2 z-10 p-2.5 sm:p-2 rounded-full bg-black/40 text-white/80 hover:text-white hover:bg-black/60 transition-colors"
                >
                    <svg className="w-5 h-5 sm:w-6 sm:h-6" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor">
                        <path strokeLinecap="round" strokeLinejoin="round" d="M8.25 4.5l7.5 7.5-7.5 7.5" />
                    </svg>
                </button>
            )}

            {/* Content area */}
            <div className="flex flex-col lg:flex-row max-w-[98vw] max-h-[96vh] gap-3 px-2 sm:px-6 lg:px-12">
                {/* Media display */}
                <div className="relative flex items-center justify-center min-w-0 flex-1">

                    {video ? (
                        <video
                            key={item.filename}
                            ref={videoRef}
                            src={mediaUrl}
                            controls
                            autoPlay
                            draggable
                            onDragStart={handleDragStart}
                            onVolumeChange={handleVolumeChange}
                            className="max-w-full max-h-[90vh] lg:max-h-[94vh] rounded-lg shadow-2xl cursor-grab active:cursor-grabbing"
                        />
                    ) : (

                        <img
                            key={item.filename}
                            src={mediaUrl}
                            alt={item.original_filename || item.filename}
                            draggable
                            onDragStart={handleDragStart}
                            className="max-w-full max-h-[90vh] lg:max-h-[94vh] rounded-lg shadow-2xl object-contain cursor-grab active:cursor-grabbing"
                        />
                    )}

                </div>

                {/* Details panel */}
                <div className="hidden lg:block w-72 flex-shrink-0 bg-white/10 backdrop-blur-md rounded-xl p-4 text-white overflow-y-auto max-h-[94vh]">


                    <div className="mb-4">
                        <div className="flex items-center gap-2 mb-1.5 text-xs font-semibold text-white/50 uppercase tracking-wider">
                            <TagIcon /> Tags
                        </div>
                        <TagInput
                            value={displayItem.tags || []}
                            onChange={handleTagsChange}
                            placeholder="Add tags..."
                        />
                    </div>

                    <dl className="space-y-3 text-sm">
                        <div>
                            <dt className="text-white/50 text-xs uppercase tracking-wider">Date taken</dt>
                            <dd className="text-white/90 mt-0.5">{formatDate(displayItem.original_date)}</dd>
                        </div>
                        <div>
                            <dt className="text-white/50 text-xs uppercase tracking-wider">Uploaded</dt>
                            <dd className="text-white/90 mt-0.5">{formatDate(displayItem.uploaded_at)}</dd>
                        </div>
                        <div>
                            <dt className="text-white/50 text-xs uppercase tracking-wider">Type</dt>
                            <dd className="text-white/90 mt-0.5 capitalize">{displayItem.media_type}</dd>
                        </div>
                        {displayItem.size_bytes != null && (
                            <div>
                                <dt className="text-white/50 text-xs uppercase tracking-wider">Size</dt>
                                <dd className="text-white/90 mt-0.5">{formatBytes(displayItem.size_bytes)}</dd>
                            </div>
                        )}
                        {displayItem.width != null && displayItem.height != null && (
                            <div>
                                <dt className="text-white/50 text-xs uppercase tracking-wider">Dimensions</dt>
                                <dd className="text-white/90 mt-0.5">{displayItem.width} x {displayItem.height}</dd>
                            </div>
                        )}
                    </dl>

                    {/* EXIF section */}
                    {exifData && Object.keys(exifData).length > 0 && (
                        <div className="mt-4 border-t border-white/10 pt-3">
                            <button
                                onClick={() => setExifOpen(o => !o)}
                                className="flex items-center justify-between w-full text-left"
                            >
                                <span className="text-white/50 text-xs uppercase tracking-wider font-semibold">EXIF Data</span>
                                <svg
                                    className={`w-4 h-4 text-white/50 transition-transform ${exifOpen ? 'rotate-180' : ''}`}
                                    fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor"
                                >
                                    <path strokeLinecap="round" strokeLinejoin="round" d="M19.5 8.25l-7.5 7.5-7.5-7.5" />
                                </svg>
                            </button>
                            {exifOpen && (
                                <dl className="mt-2 space-y-1.5 text-xs">
                                    {Object.entries(exifData).map(([key, value]) => (
                                        <div key={key} className="flex justify-between gap-2">
                                            <dt className="text-white/40 truncate flex-shrink-0 max-w-[40%]" title={key}>{key}</dt>
                                            <dd className="text-white/80 text-right break-all min-w-0">{value}</dd>
                                        </div>
                                    ))}
                                </dl>
                            )}
                        </div>
                    )}

                    {/* Loading indicator for details */}
                    {item.id && !detail && (
                        <div className="mt-4 flex justify-center">
                            <LoadingIndicator size="sm" color="text-white/40" />
                        </div>
                    )}

                    {/* Actions */}
                    <div className="mt-5 flex flex-col gap-2">
                        {/* Favorite Button */}
                        {onToggleFavorite && (
                            <button
                                onClick={onToggleFavorite}
                                className={`flex items-center justify-center gap-2 w-full px-3 py-2 text-xs font-medium rounded-lg transition-colors border ${
                                    displayItem.is_favorite
                                        ? 'bg-red-500 text-white border-red-600 hover:bg-red-600'
                                        : 'bg-white/10 text-white hover:bg-white/20 border-white/10'
                                }`}
                            >
                                <HeartIcon solid={displayItem.is_favorite} />
                                {displayItem.is_favorite ? 'Favorited' : 'Add to Favorites'}
                            </button>
                        )}

                        {/* Find Similar Button */}
                        {onFindSimilar && item.id && (
                            <button
                                onClick={() => {
                                    if (item.id) onFindSimilar(item.id);
                                }}
                                className="flex items-center justify-center gap-2 w-full px-3 py-2 text-xs font-medium rounded-lg bg-purple-500/20 text-purple-100 hover:bg-purple-500/30 hover:text-white transition-colors border border-purple-500/30"
                                title="Find visually similar items"
                            >
                                <svg className="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor">
                                    <path strokeLinecap="round" strokeLinejoin="round" d="M21 21l-5.197-5.197m0 0A7.5 7.5 0 105.196 5.196a7.5 7.5 0 0010.607 10.607z" />
                                </svg>
                                Find Similar
                            </button>
                        )}

                        {/* Delete Button */}
                        {onDelete && (
                            <button
                                onClick={onDelete}
                                className="flex items-center justify-center gap-2 w-full px-3 py-2 text-xs font-medium rounded-lg bg-red-500/20 text-red-200 hover:bg-red-500/30 hover:text-white transition-colors border border-red-500/30"
                            >
                                <svg className="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor">
                                    <path strokeLinecap="round" strokeLinejoin="round" d="M14.74 9l-.346 9m-4.788 0L9.26 9m9.968-3.21c.342.052.682.107 1.022.166m-1.022-.165L18.16 19.673a2.25 2.25 0 01-2.244 2.077H8.084a2.25 2.25 0 01-2.244-2.077L4.772 5.79m14.456 0a48.108 48.108 0 00-3.478-.397m-12 .562c.34-.059.68-.114 1.022-.165m0 0a48.11 48.11 0 013.478-.397m7.5 0v-.916c0-1.18-.91-2.164-2.09-2.201a51.964 51.964 0 00-3.32 0c-1.18.037-2.09 1.022-2.09 2.201v.916m7.5 0a48.667 48.667 0 00-7.5 0" />
                                </svg>
                                Delete
                            </button>
                        )}

                        {/* Open original link */}
                        <a
                            href={mediaUrl}
                            target="_blank"
                            rel="noreferrer"
                            className="flex items-center justify-center gap-2 w-full px-3 py-2 text-xs font-medium rounded-lg bg-white/15 text-white/90 hover:bg-white/25 transition-colors"
                        >
                            <svg className="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor">
                                <path strokeLinecap="round" strokeLinejoin="round" d="M13.5 6H5.25A2.25 2.25 0 003 8.25v10.5A2.25 2.25 0 005.25 21h10.5A2.25 2.25 0 0018 18.75V10.5m-10.5 6L21 3m0 0h-5.25M21 3v5.25" />
                            </svg>
                            Open original
                        </a>
                    </div>
                </div>
            </div>
        </div>
    );
}
