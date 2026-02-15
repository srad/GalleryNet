import { useState, useEffect, useCallback, useRef } from 'react';
import type { MediaItem } from '../types';
import { apiFetch } from '../auth';
import { HeartIcon } from './Icons';

interface MediaModalProps {
    item: MediaItem;
    onClose: () => void;
    onPrev: (() => void) | null;
    onNext: (() => void) | null;
    onFindSimilar?: (id: string) => void;
    onToggleFavorite?: () => void;
}

const VIDEO_EXTENSIONS = new Set(['mp4', 'mov', 'avi', 'webm', 'mkv', 'flv', 'wmv']);

function isVideo(filename: string): boolean {
    const ext = filename.split('.').pop()?.toLowerCase() ?? '';
    return VIDEO_EXTENSIONS.has(ext);
}

function formatDate(iso: string): string {
    try {
        const d = new Date(iso);
        return d.toLocaleDateString(undefined, {
            year: 'numeric',
            month: 'long',
            day: 'numeric',
            hour: '2-digit',
            minute: '2-digit',
        });
    } catch {
        return iso;
    }
}

function formatBytes(bytes: number): string {
    if (bytes === 0) return '0 B';
    const units = ['B', 'KB', 'MB', 'GB', 'TB'];
    const i = Math.floor(Math.log(bytes) / Math.log(1024));
    const val = bytes / Math.pow(1024, i);
    return `${val.toFixed(i > 0 ? 1 : 0)} ${units[i]}`;
}

export default function MediaModal({ item, onClose, onPrev, onNext, onFindSimilar, onToggleFavorite }: MediaModalProps) {
    const backdropRef = useRef<HTMLDivElement>(null);
    const video = isVideo(item.filename);
    const mediaUrl = `/uploads/${item.filename}`;

    const [detail, setDetail] = useState<MediaItem | null>(null);
    const [exifOpen, setExifOpen] = useState(false);

    // Fetch full details (including exif_json) when item changes
    useEffect(() => {
        setDetail(null);
        setExifOpen(false);
        if (!item.id) return;

        let cancelled = false;
        apiFetch(`/api/media/${item.id}`)
            .then(res => res.ok ? res.json() : null)
            .then(data => { if (!cancelled && data) setDetail(data); })
            .catch(() => {});
        return () => { cancelled = true; };
    }, [item.id]);

    // Use fetched detail for fields when available, fallback to the summary item
    const displayItem = detail || item;
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
        if (e.key === 'Escape') onClose();
        else if (e.key === 'ArrowLeft' && onPrev) onPrev();
        else if (e.key === 'ArrowRight' && onNext) onNext();
    }, [onClose, onPrev, onNext]);

    useEffect(() => {
        document.addEventListener('keydown', handleKeyDown);
        const prev = document.body.style.overflow;
        document.body.style.overflow = 'hidden';
        return () => {
            document.removeEventListener('keydown', handleKeyDown);
            document.body.style.overflow = prev;
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

    return (
        <div
            ref={backdropRef}
            onClick={handleBackdropClick}
            onTouchStart={handleTouchStart}
            onTouchEnd={handleTouchEnd}
            className="fixed inset-0 z-50 flex items-center justify-center bg-black/80 backdrop-blur-sm"
        >
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
                <div className="flex items-center justify-center min-w-0 flex-1">
                    {video ? (
                        <video
                            key={item.filename}
                            src={mediaUrl}
                            controls
                            autoPlay
                            className="max-w-full max-h-[60vh] lg:max-h-[94vh] rounded-lg shadow-2xl"
                        />
                    ) : (
                        <img
                            key={item.filename}
                            src={mediaUrl}
                            alt={item.original_filename || item.filename}
                            className="max-w-full max-h-[60vh] lg:max-h-[94vh] rounded-lg shadow-2xl object-contain"
                        />
                    )}
                </div>

                {/* Details panel */}
                <div className="w-full lg:w-72 flex-shrink-0 bg-white/10 backdrop-blur-md rounded-xl p-3 sm:p-4 text-white overflow-y-auto max-h-[30vh] lg:max-h-[94vh]">
                    <h3 className="text-sm font-semibold text-white/90 mb-4 break-words" title={displayItem.original_filename || displayItem.filename}>
                        {displayItem.original_filename || displayItem.filename}
                    </h3>

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
                            <svg className="w-4 h-4 animate-spin text-white/40" viewBox="0 0 24 24" fill="none">
                                <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
                                <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
                            </svg>
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
                                    if (item.id) {
                                        onFindSimilar(item.id);
                                        onClose();
                                    }
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
