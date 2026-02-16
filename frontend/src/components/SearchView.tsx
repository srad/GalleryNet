import { useState, useEffect, useCallback, useRef } from 'react';
import { useSearchParams } from 'react-router-dom';
import type { MediaItem, Folder } from '../types';
import MediaCard from './MediaCard';
import MediaModal from './MediaModal';
import { apiFetch } from '../auth';
import { LibraryPicker } from './GalleryView';
import { PhotoIcon, LogoutIcon } from './Icons';
import LoadingIndicator from './LoadingIndicator';

interface SearchViewProps {
    folders: Folder[];
    refreshKey: number;
    onFoldersChanged: () => void;
    onLogout?: () => void;
}

export default function SearchView({ folders, refreshKey, onFoldersChanged, onLogout }: SearchViewProps) {
    const [searchParams, setSearchParams] = useSearchParams();
    const sourceId = searchParams.get('source');
    const selectedMediaId = searchParams.get('media');
    const similarityParam = searchParams.get('similarity');

    const [searchFile, setSearchFile] = useState<File | null>(null);
    const [localSimilarity, setLocalSimilarity] = useState<number>(() => 
        similarityParam ? parseInt(similarityParam, 10) : 70
    );
    const [searchResults, setSearchResults] = useState<MediaItem[]>([]);
    const [isSearching, setIsSearching] = useState(false);
    const [activeId, setActiveId] = useState<string | null>(null);
    const [showPicker, setShowPicker] = useState(false);

    // Track the last search criteria to prevent redundant searches
    const lastSearchRef = useRef<{ source: string | null; file: File | null; sim: number } | null>(null);

    const handleSearchById = useCallback(async (id: string, sim: number) => {
        setIsSearching(true);
        try {
            const res = await apiFetch(`/api/media/${id}/similar?similarity=${sim}`);
            if (res.ok) {
                const results = await res.json();
                setSearchResults(results);
            }
        } catch (e) {
            console.error(e);
            alert('Search failed');
        } finally {
            setIsSearching(false);
        }
    }, []);

    const handleSearchByFile = useCallback(async (file: File, sim: number) => {
        setIsSearching(true);
        const formData = new FormData();
        formData.append('similarity', sim.toString());
        formData.append('file', file);

        try {
            const res = await apiFetch('/api/search', { method: 'POST', body: formData });
            if (res.ok) {
                const results = await res.json();
                setSearchResults(results);
            }
        } catch (e) {
            alert(`Search error: ${e}`);
        } finally {
            setIsSearching(false);
        }
    }, []);

    // Sync local slider state when URL param changes (e.g. back/forward navigation)
    useEffect(() => {
        if (similarityParam) {
            const val = parseInt(similarityParam, 10);
            if (!isNaN(val) && val !== localSimilarity) {
                setLocalSimilarity(val);
            }
        }
    }, [similarityParam]);

    // Main search trigger effect
    useEffect(() => {
        const currentSim = similarityParam ? parseInt(similarityParam, 10) : localSimilarity;
        
        const currentSource = sourceId;
        const currentFile = searchFile;

        // Check if anything meaningful changed
        const last = lastSearchRef.current;
        const changed = !last || 
                        last.source !== currentSource || 
                        last.file !== currentFile || 
                        last.sim !== currentSim;

        if (!changed) return;

        // Reset search results if source is cleared
        if (!currentSource && !currentFile) {
            setSearchResults([]);
            setActiveId(null);
            lastSearchRef.current = null;
            return;
        }

        // Run the appropriate search
        if (currentSource) {
            setActiveId(currentSource);
            setSearchFile(null);
            handleSearchById(currentSource, currentSim);
        } else if (currentFile) {
            setActiveId(null);
            handleSearchByFile(currentFile, currentSim);
        }

        lastSearchRef.current = { source: currentSource, file: currentFile, sim: currentSim };
    }, [sourceId, similarityParam, searchFile, handleSearchById, handleSearchByFile]);

    const handleSimilarityCommit = useCallback(() => {
        setSearchParams(prev => {
            const next = new URLSearchParams(prev);
            next.set('similarity', localSimilarity.toString());
            return next;
        });
    }, [localSimilarity, setSearchParams]);

    const clearSearch = () => {
        setSearchFile(null);
        setSearchResults([]);
        setSearchParams(prev => {
            const next = new URLSearchParams(prev);
            next.delete('source');
            return next;
        });
    };

    const handleCardClick = useCallback((item: MediaItem) => {
        if (item.id) {
            setSearchParams(prev => {
                const next = new URLSearchParams(prev);
                next.set('media', item.id!);
                return next;
            });
        }
    }, [setSearchParams]);

    const handlePickItem = useCallback((ids: string[]) => {
        if (ids.length > 0) {
            setSearchParams(prev => {
                const next = new URLSearchParams(prev);
                next.set('source', ids[0]);
                return next;
            });
        }
        setShowPicker(false);
    }, [setSearchParams]);

    // Helper to get thumbnail URL
    const getThumbnailUrl = (uuid: string) => {
        const p1 = uuid.substring(0, 2);
        const p2 = uuid.substring(2, 4);
        return `/thumbnails/${p1}/${p2}/${uuid}.jpg`;
    };

    // Find similar from modal (updates source param)
    const handleFindSimilar = useCallback((id: string) => {
        setSearchParams(prev => {
            const next = new URLSearchParams(prev);
            next.set('source', id);
            next.delete('media'); // Close modal
            return next;
        });
    }, [setSearchParams]);

    const handleDragStart = useCallback((item: MediaItem, e: React.DragEvent) => {
        if (item.id) {
            e.dataTransfer.setData('application/x-gallerynet-media', JSON.stringify([item.id]));
            e.dataTransfer.effectAllowed = 'copyMove';
            e.dataTransfer.setData('text/plain', `Media: 1 item`);
        }
    }, []);

    // Render modal logic

    const renderModal = () => {
        if (!selectedMediaId) return null;
        
        const idx = searchResults.findIndex(m => m.id === selectedMediaId);
        // Find item in results, or use deep linking logic if needed (handled by MediaModal anyway)
        const item = idx !== -1 ? searchResults[idx] : { id: selectedMediaId, filename: '' } as MediaItem;
        const prevItem = idx > 0 ? searchResults[idx - 1] : undefined;
        const nextItem = idx < searchResults.length - 1 ? searchResults[idx + 1] : undefined;

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
                onPrev={prevItem?.id ? () => handleSetMediaId(prevItem!.id!) : null}
                onNext={nextItem?.id ? () => handleSetMediaId(nextItem!.id!) : null}
                onFindSimilar={handleFindSimilar}
                onToggleFavorite={() => {
                    if (!item.id) return;
                    const newStatus = !item.is_favorite;
                    setSearchResults(prev => prev.map(m => m.id === item.id ? { ...m, is_favorite: newStatus } : m));
                    apiFetch(`/api/media/${item.id}/favorite`, {
                        method: 'POST',
                        headers: { 'Content-Type': 'application/json' },
                        body: JSON.stringify({ favorite: newStatus }),
                    }).catch(e => console.error(e));
                }}
            />
        );
    };

    return (
        <div className="max-w-7xl mx-auto h-full flex flex-col">
            <div className="flex items-center justify-between mb-4 md:mb-6 shrink-0">
                <h2 className="text-2xl md:text-3xl font-bold text-gray-900 truncate">Visual Search</h2>
                {onLogout && (
                    <button
                        onClick={onLogout}
                        className="flex items-center gap-2 px-3 py-2 text-sm font-semibold text-gray-500 hover:text-gray-900 hover:bg-white/50 rounded-xl transition-all active:scale-95 border border-transparent hover:border-gray-200"
                        title="Log out"
                    >
                        <LogoutIcon />
                    </button>
                )}
            </div>

            <div className="bg-white border border-gray-200 p-4 sm:p-6 md:p-8 rounded-2xl shadow-sm mb-4 sm:mb-8 shrink-0">
                <div className="flex flex-col lg:flex-row gap-6 sm:gap-8 items-start lg:items-center">
                    <div className="flex-1 w-full">
                        <label className="block text-sm font-medium text-gray-700 mb-2">Reference Image</label>
                        
                        {activeId ? (
                            <div className="flex items-center gap-4 p-3 border border-gray-200 rounded-xl bg-gray-50">
                                <img 
                                    src={getThumbnailUrl(activeId)} 
                                    alt="Reference" 
                                    className="w-16 h-16 object-cover rounded-lg shadow-sm"
                                />
                                <div className="flex-1">
                                    <p className="text-sm font-medium text-gray-900">Searching by selected image</p>
                                    <button 
                                        onClick={clearSearch}
                                        className="text-xs text-red-600 hover:text-red-800 font-semibold mt-1"
                                    >
                                        Clear / Pick New
                                    </button>
                                </div>
                            </div>
                        ) : searchFile ? (
                            <div className="flex items-center gap-4 p-3 border border-gray-200 rounded-xl bg-gray-50">
                                <div className="w-16 h-16 bg-purple-100 rounded-lg flex items-center justify-center">
                                    <PhotoIcon className="w-8 h-8 text-purple-600" />
                                </div>
                                <div className="flex-1">
                                    <p className="text-sm font-medium text-gray-900">{searchFile.name}</p>
                                    <button 
                                        onClick={clearSearch}
                                        className="text-xs text-red-600 hover:text-red-800 font-semibold mt-1"
                                    >
                                        Clear / Pick New
                                    </button>
                                </div>
                            </div>
                        ) : (
                            <div className="flex flex-col sm:flex-row gap-3">
                                <button
                                    onClick={() => setShowPicker(true)}
                                    className="flex-1 flex items-center justify-center gap-2 px-4 py-2.5 bg-white border border-gray-200 hover:border-blue-400 hover:bg-blue-50 text-gray-700 rounded-xl transition-all font-medium text-sm shadow-sm group"
                                >
                                    <PhotoIcon className="w-5 h-5 text-gray-400 group-hover:text-blue-500" />
                                    Select from Library
                                </button>
                                <div className="relative flex-1">
                                    <input
                                        type="file"
                                        accept="image/*"
                                        onChange={(e) => {
                                            const file = e.target.files?.[0] || null;
                                            if (file) setSearchFile(file);
                                        }}
                                        className="absolute inset-0 opacity-0 cursor-pointer"
                                    />
                                    <div className="flex items-center justify-center gap-2 px-4 py-2.5 bg-white border border-gray-200 hover:border-purple-400 hover:bg-purple-50 text-gray-700 rounded-xl transition-all font-medium text-sm shadow-sm">
                                        <svg className="w-5 h-5 text-gray-400" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor">
                                            <path strokeLinecap="round" strokeLinejoin="round" d="M3 16.5v2.25A2.25 2.25 0 005.25 21h13.5A2.25 2.25 0 0021 18.75V16.5m-13.5-9L12 3m0 0l4.5 4.5M12 3v13.5" />
                                        </svg>
                                        Upload File
                                    </div>
                                </div>
                            </div>
                        )}
                    </div>

                    <div className="w-full lg:w-72">
                        <label htmlFor="similarity" className="block text-sm font-medium text-gray-700 mb-2">
                            Min Similarity: <span className="text-purple-600 font-bold">{localSimilarity}%</span>
                        </label>
                        <input
                            id="similarity"
                            type="range"
                            min="0"
                            max="100"
                            value={localSimilarity}
                            onChange={(e) => setLocalSimilarity(Number(e.target.value))}
                            onPointerUp={handleSimilarityCommit}
                            onKeyUp={(e) => { if (e.key === 'ArrowLeft' || e.key === 'ArrowRight') handleSimilarityCommit(); }}
                            className="w-full accent-purple-600 cursor-pointer h-2 bg-gray-200 rounded-lg appearance-none"
                        />
                    </div>
                </div>
            </div>

            <div className="flex-1 relative min-h-0">
                {isSearching && searchResults.length > 0 && (
                    <LoadingIndicator 
                        variant="overlay" 
                        label="Updating results..." 
                        color="text-purple-600" 
                    />
                )}

                {searchResults.length > 0 ? (
                    <div className="h-full overflow-y-auto">
                        <h3 className="text-lg font-bold text-gray-900 mb-4 border-b border-gray-200 pb-2 sticky top-0 bg-gray-50 z-10 flex items-center justify-between">
                            <span>Matches Found ({searchResults.length})</span>
                            <span className="text-xs font-normal text-gray-400">Similarity &ge; {similarityParam || localSimilarity}%</span>
                        </h3>
                        <div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6 gap-3 sm:gap-4 pb-8">
                            {searchResults.map((item) => (
                                <MediaCard 
                                    key={`search-${item.id}`} 
                                    item={item}
                                    onClick={() => handleCardClick(item)}
                                    onDragStart={(e) => handleDragStart(item, e)}
                                />

                            ))}
                        </div>
                    </div>
                ) : (
                    <div className="h-full flex items-center justify-center text-gray-400">
                        {isSearching ? (
                            <LoadingIndicator 
                                variant="centered" 
                                label="Analyzing library..." 
                                size="lg" 
                                color="text-purple-600" 
                            />
                        ) : (
                            <div className="flex flex-col items-center gap-3">
                                <PhotoIcon className="w-12 h-12 text-gray-200" />
                                <span className="text-sm text-center max-w-xs">Select an image to find similar matches across your library.</span>
                            </div>
                        )}
                    </div>
                )}
            </div>

            {showPicker && (
                <LibraryPicker
                    onPick={handlePickItem}
                    onCancel={() => setShowPicker(false)}
                    refreshKey={refreshKey}
                    folders={folders}
                    onFoldersChanged={onFoldersChanged}
                    singleSelect={true}
                />
            )}

            {renderModal()}
        </div>
    );
}
