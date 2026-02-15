import type { MediaItem } from '../types';
import { HeartIcon } from './Icons';

interface MediaCardProps {
    item: MediaItem;
    onClick?: () => void;
    /** Whether the card is currently selected */
    selected?: boolean;
    /** Whether selection mode is active (shows checkboxes) */
    selectionMode?: boolean;
    /** Called when the selection checkbox is toggled */
    onSelect?: (e: React.MouseEvent) => void;
    onToggleFavorite?: () => void;
}

const VIDEO_EXTENSIONS = new Set(['mp4', 'mov', 'avi', 'webm', 'mkv', 'flv', 'wmv']);

/** Replace file extension with .jpg to match backend thumbnail naming */
function thumbnailUrl(filename: string): string {
    const dotIdx = filename.lastIndexOf('.');
    const base = dotIdx !== -1 ? filename.substring(0, dotIdx) : filename;
    return `/thumbnails/${base}.jpg`;
}

function isVideo(filename: string): boolean {
    const ext = filename.split('.').pop()?.toLowerCase() ?? '';
    return VIDEO_EXTENSIONS.has(ext);
}

export default function MediaCard({ item, onClick, selected, selectionMode, onSelect, onToggleFavorite }: MediaCardProps) {
    const video = isVideo(item.filename);

    return (
        <button
            type="button"
            data-filename={item.filename}
            onClick={selectionMode ? onSelect : onClick}
            className={`group relative block overflow-hidden rounded-lg bg-gray-100 border shadow-sm transition-all hover:shadow-md hover:-translate-y-0.5 w-full text-left ${
                selected
                    ? 'border-blue-500 ring-2 ring-blue-500/40'
                    : 'border-gray-200/60'
            }`}
        >
            <div className="aspect-square w-full">
                <img
                    src={thumbnailUrl(item.filename)}
                    alt={item.original_filename || item.filename}
                    loading="lazy"
                    decoding="async"
                    className="h-full w-full object-cover transition-transform duration-300 group-hover:scale-105"
                    onError={(e) => {
                        const img = e.currentTarget;
                        const fallback = `/uploads/${item.filename}`;
                        // Only attempt fallback once to prevent infinite loop
                        if (img.src !== fallback && !img.src.endsWith(fallback)) {
                            img.src = fallback;
                        }
                    }}
                />
            </div>

            {/* Selection checkbox — visible in selection mode or on hover */}
            {(selectionMode || selected) && (
                <div
                    className="absolute top-1.5 left-1.5 z-10"
                    onClick={(e) => {
                        e.stopPropagation();
                        onSelect?.(e);
                    }}
                >
                    <div className={`w-5 h-5 rounded border-2 flex items-center justify-center transition-colors ${
                        selected
                            ? 'bg-blue-500 border-blue-500'
                            : 'bg-white/80 border-gray-400 hover:border-blue-400'
                    }`}>
                        {selected && (
                            <svg className="w-3 h-3 text-white" fill="none" viewBox="0 0 24 24" strokeWidth={3} stroke="currentColor">
                                <path strokeLinecap="round" strokeLinejoin="round" d="M4.5 12.75l6 6 9-13.5" />
                            </svg>
                        )}
                    </div>
                </div>
            )}

            {/* Favorite button */}
            {!selectionMode && onToggleFavorite && (
                <button
                    onClick={(e) => {
                        e.stopPropagation();
                        onToggleFavorite();
                    }}
                    className={`absolute top-1.5 right-1.5 z-10 p-1.5 rounded-full transition-all ${
                        item.is_favorite
                            ? 'text-red-500 shadow-sm'
                            : 'text-white/70 hover:text-white opacity-0 group-hover:opacity-100'
                    }`}
                    title={item.is_favorite ? "Remove from favorites" : "Add to favorites"}
                >
                    <HeartIcon solid={item.is_favorite} />
                </button>
            )}

            {/* Play button overlay for videos */}
            {video && (
                <div className="absolute inset-0 flex items-center justify-center pointer-events-none">
                    <div className="w-8 h-8 rounded-full bg-black/50 flex items-center justify-center backdrop-blur-sm">
                        <svg className="w-3.5 h-3.5 text-white ml-0.5" fill="currentColor" viewBox="0 0 24 24">
                            <path d="M8 5v14l11-7z" />
                        </svg>
                    </div>
                </div>
            )}
            {/* Overlay gradient on hover */}
            <div className="absolute inset-0 bg-gradient-to-t from-black/60 via-transparent to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100 flex flex-col justify-end p-2">
                <p className="text-white text-[10px] font-medium truncate leading-tight">
                    {item.original_filename || item.filename}
                </p>
                {/* Download button */}
                <a
                    href={`/uploads/${item.filename}`}
                    download={item.original_filename || item.filename}
                    onClick={e => e.stopPropagation()}
                    className="absolute bottom-1.5 right-1.5 w-6 h-6 rounded-full bg-black/50 hover:bg-black/70 flex items-center justify-center backdrop-blur-sm transition-colors"
                    title="Download"
                >
                    <svg className="w-3 h-3 text-white" fill="none" viewBox="0 0 24 24" strokeWidth={2.5} stroke="currentColor">
                        <path strokeLinecap="round" strokeLinejoin="round" d="M3 16.5v2.25A2.25 2.25 0 005.25 21h13.5A2.25 2.25 0 0021 18.75V16.5M16.5 12L12 16.5m0 0L7.5 12m4.5 4.5V3" />
                    </svg>
                </a>
            </div>
        </button>
    );
}
