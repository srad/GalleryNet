import { useState, useEffect, useCallback, useRef } from 'react';
import { useSearchParams } from 'react-router-dom';
import type { MediaItem, Folder } from '../types';
import MediaCard from './MediaCard';
import MediaModal from './MediaModal';
import { apiClient } from '../api';

import { LibraryPicker } from './GalleryView';
import { PhotoIcon, LogoutIcon } from './Icons';
import LoadingIndicator from './LoadingIndicator';

interface SearchViewProps {
    folders: Folder[];
    refreshKey: number;
    onFoldersChanged: () => void;
    onLogout?: () => void;
    isActive?: boolean;
}

export default function SearchView({ folders, refreshKey, onFoldersChanged, onLogout, isActive = true }: SearchViewProps) {
    const [searchParams, setSearchParams] = useSearchParams();
    const sourceId = searchParams.get('source');
    const faceId = searchParams.get('face');
    const selectedMediaId = searchParams.get('media');

    const similarityParam = searchParams.get('similarity');

    const [searchFile, setSearchFile] = useState<File | null>(null);
    const [localSimilarity, setLocalSimilarity] = useState<number>(() =>
        similarityParam ? parseInt(similarityParam, 10) : 45
    );

    const [searchResults, setSearchResults] = useState<MediaItem[]>([]);
    const [isSearching, setIsSearching] = useState(false);
    const [hasSearched, setHasSearched] = useState(false);
    const [activeId, setActiveId] = useState<string | null>(null);
    const [showPicker, setShowPicker] = useState(false);

    const handleSearchByFace = useCallback(async (id: string, sim: number) => {
        setIsSearching(true);
        setHasSearched(true);
        try {
            const results = await apiClient.searchFaces(id, sim);
            setSearchResults(results);
        } catch (e) {
            console.error(e);
            alert('Face search failed');
        } finally {
            setIsSearching(false);
        }
    }, []);


    const handleSearchById = useCallback(async (id: string, sim: number) => {
        setIsSearching(true);
        setHasSearched(true);
        try {
            const results = await apiClient.searchSimilarById(id, sim);
            setSearchResults(results);
        } catch (e) {
            console.error(e);
            alert('Search failed');
        } finally {
            setIsSearching(false);
        }
    }, []);

    const handleSearchByFile = useCallback(async (file: File, sim: number) => {
        setIsSearching(true);
        setHasSearched(true);
        try {
            const results = await apiClient.searchSimilarByFile(file, sim);
            setSearchResults(results);
        } catch (e) {
            alert(`Search error: ${e}`);
        } finally {
            setIsSearching(false);
        }
    }, []);

    // Track the last search criteria to prevent redundant searches

    const lastSearchRef = useRef<{ source: string | null; face: string | null; file: File | null; sim: number } | null>(null);


    // Sync local slider state when URL param changes (e.g. back/forward navigation)

    useEffect(() => {
        if (similarityParam) {
            const val = parseInt(similarityParam, 10);
            if (!isNaN(val)) {
                setLocalSimilarity(prev => (prev !== val ? val : prev));
            }
        }
    }, [similarityParam]);

    // Main search trigger effect
    useEffect(() => {
        // Use the similarity from URL if present, otherwise use the local state (only on initial load or source change)
        const currentSim = similarityParam ? parseInt(similarityParam, 10) : localSimilarity;

        const currentSource = sourceId;
        const currentFace = faceId;
        const currentFile = searchFile;

        // Check if anything meaningful changed
        const last = lastSearchRef.current;
        const changed = !last ||
                        last.source !== currentSource ||
                        last.face !== currentFace ||
                        last.file !== currentFile ||
                        last.sim !== currentSim;

        if (!changed) return;

        // Reset search results if source is cleared
        if (!currentSource && !currentFile && !currentFace) {
            setSearchResults([]);
            setHasSearched(false);
            setActiveId(null);
            lastSearchRef.current = null;
            return;
        }

        // Run the appropriate search
        if (currentSource) {
            setActiveId(currentSource);
            setSearchFile(null);
            handleSearchById(currentSource, currentSim);
        } else if (currentFace) {
            setActiveId(null);
            setSearchFile(null);
            handleSearchByFace(currentFace, currentSim);
        } else if (currentFile) {
            setActiveId(null);
            handleSearchByFile(currentFile, currentSim);
        }

        lastSearchRef.current = { source: currentSource, face: currentFace, file: currentFile, sim: currentSim };
    }, [sourceId, faceId, similarityParam, searchFile, handleSearchById, handleSearchByFile, handleSearchByFace]);


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
        setHasSearched(false);
        setSearchParams(prev => {
            const next = new URLSearchParams(prev);
            next.delete('source');
            next.delete('face');
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
        if (!isActive || !selectedMediaId) return null;

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
                onToggleFavorite={async () => {
                    if (!item.id) return;
                    const newStatus = !item.is_favorite;
                    setSearchResults(prev => prev.map(m => m.id === item.id ? { ...m, is_favorite: newStatus } : m));
                    try {
                        await apiClient.toggleFavorite(item.id, newStatus);
                    } catch (e) {
                        console.error(e);
                        // Revert on error
                        setSearchResults(prev => prev.map(m => m.id === item.id ? { ...m, is_favorite: !newStatus } : m));
                    }
                }}

            />
        );
    };

    return (
        <div className="h-full flex flex-col">
            <div className="flex items-center justify-between mb-4 md:mb-6 shrink-0">
                <h2 className="text-2xl md:text-3xl font-bold text-gray-900 dark:text-gray-100 truncate">Visual Search</h2>
                {onLogout && (
                    <button
                        onClick={onLogout}
                        className="flex items-center gap-2 px-3 py-2 text-sm font-semibold text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-gray-100 hover:bg-white/50 dark:hover:bg-gray-800 rounded-xl transition-all active:scale-95 border border-transparent hover:border-gray-200 dark:hover:border-gray-700"
                        title="Log out"
                    >
                        <LogoutIcon />
                    </button>
                )}
            </div>

            <div className="bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700 p-4 sm:p-6 md:p-8 rounded-2xl shadow-sm mb-4 sm:mb-8 shrink-0">
                <div className="flex flex-col lg:flex-row gap-6 sm:gap-8 items-start lg:items-center">
                    <div className="flex-1 w-full">
                        <label className="block text-sm font-medium text-gray-700 dark:text-gray-200 mb-2">Reference Image</label>

                        {activeId ? (
                            <div className="flex items-center gap-4 p-3 border border-gray-200 dark:border-gray-700 rounded-xl bg-gray-50 dark:bg-gray-900">
                                <img
                                    src={getThumbnailUrl(activeId)}
                                    alt="Reference"
                                    className="w-16 h-16 object-cover rounded-lg shadow-sm"
                                />
                                <div className="flex-1">
                                    <p className="text-sm font-medium text-gray-900 dark:text-gray-100">Searching by selected image</p>
                                    <button
                                        onClick={clearSearch}
                                        className="text-xs text-red-600 dark:text-red-400 hover:text-red-800 dark:hover:text-red-300 font-semibold mt-1"
                                    >
                                        Clear / Pick New
                                    </button>
                                </div>
                            </div>
                        ) : faceId ? (
                            <div className="flex items-center gap-4 p-3 border border-gray-200 dark:border-gray-700 rounded-xl bg-gray-50 dark:bg-gray-900">
                                <div className="w-16 h-16 bg-amber-100 dark:bg-amber-900 rounded-lg flex items-center justify-center">
                                    <svg className="w-8 h-8 text-amber-600 dark:text-amber-400" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                                        <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.5} d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
                                        <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.5} d="M2.458 12C3.732 7.943 7.523 5 12 5c4.478 0 8.268 2.943 9.542 7-1.274 4.057-5.064 7-9.542 7-4.477 0-8.268-2.943-9.542-7z" />
                                    </svg>
                                </div>
                                <div className="flex-1">
                                    <p className="text-sm font-medium text-gray-900 dark:text-gray-100">Searching by identified person</p>
                                    <button
                                        onClick={clearSearch}
                                        className="text-xs text-red-600 dark:text-red-400 hover:text-red-800 dark:hover:text-red-300 font-semibold mt-1"
                                    >
                                        Clear / Pick New
                                    </button>
                                </div>
                            </div>
                        ) : searchFile ? (

                            <div className="flex items-center gap-4 p-3 border border-gray-200 dark:border-gray-700 rounded-xl bg-gray-50 dark:bg-gray-900">
                                <div className="w-16 h-16 bg-purple-100 dark:bg-purple-900 rounded-lg flex items-center justify-center">
                                    <PhotoIcon className="w-8 h-8 text-purple-600 dark:text-purple-400" />
                                </div>
                                <div className="flex-1">
                                    <p className="text-sm font-medium text-gray-900 dark:text-gray-100">{searchFile.name}</p>
                                    <button
                                        onClick={clearSearch}
                                        className="text-xs text-red-600 dark:text-red-400 hover:text-red-800 dark:hover:text-red-300 font-semibold mt-1"
                                    >
                                        Clear / Pick New
                                    </button>
                                </div>
                            </div>
                        ) : (
                            <div className="flex flex-col sm:flex-row gap-3">
                                <button
                                    onClick={() => setShowPicker(true)}
                                    className="flex-1 flex items-center justify-center gap-2 px-4 py-2.5 bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-600 hover:border-blue-400 hover:bg-blue-50 dark:hover:bg-blue-950 text-gray-700 dark:text-gray-200 rounded-xl transition-all font-medium text-sm shadow-sm group"
                                >
                                    <PhotoIcon className="w-5 h-5 text-gray-400 dark:text-gray-500 group-hover:text-blue-500" />
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
                                    <div className="flex items-center justify-center gap-2 px-4 py-2.5 bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-600 hover:border-purple-400 hover:bg-purple-50 dark:hover:bg-purple-950 text-gray-700 dark:text-gray-200 rounded-xl transition-all font-medium text-sm shadow-sm">
                                        <svg className="w-5 h-5 text-gray-400 dark:text-gray-500" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor">
                                            <path strokeLinecap="round" strokeLinejoin="round" d="M3 16.5v2.25A2.25 2.25 0 005.25 21h13.5A2.25 2.25 0 0021 18.75V16.5m-13.5-9L12 3m0 0l4.5 4.5M12 3v13.5" />
                                        </svg>
                                        Upload File
                                    </div>
                                </div>
                            </div>
                        )}
                    </div>

                    <div className="w-full lg:w-72">
                        <label htmlFor="similarity" className="block text-sm font-medium text-gray-700 dark:text-gray-200 mb-2">
                            Min Similarity: <span className="text-purple-600 dark:text-purple-400 font-bold">{localSimilarity}%</span>
                        </label>
                        <input
                            id="similarity"
                            type="range"
                            min="30"
                            max="100"
                            value={localSimilarity}

                            onChange={(e) => setLocalSimilarity(Number(e.target.value))}
                            onPointerUp={handleSimilarityCommit}
                            onKeyUp={(e) => { if (e.key === 'ArrowLeft' || e.key === 'ArrowRight') handleSimilarityCommit(); }}
                            className="w-full accent-purple-600 cursor-pointer h-2 bg-gray-200 dark:bg-gray-700 rounded-lg appearance-none"
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
                        <h3 className="text-lg font-bold text-gray-900 dark:text-gray-100 mb-4 border-b border-gray-200 dark:border-gray-700 pb-2 sticky top-0 bg-gray-50 dark:bg-gray-900 z-10 flex items-center justify-between">
                            <span>Matches Found ({searchResults.length})</span>
                            <span className="text-xs font-normal text-gray-400 dark:text-gray-500">Similarity &ge; {similarityParam || localSimilarity}%</span>
                        </h3>
                        <div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6 2xl:grid-cols-8 3xl:grid-cols-10 gap-3 sm:gap-4 pb-8">
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
                    <div className="h-full flex items-center justify-center text-gray-400 dark:text-gray-500">
                        {isSearching ? (
                            <LoadingIndicator
                                variant="centered"
                                label="Analyzing library..."
                                size="lg"
                                color="text-purple-600"
                            />
                        ) : hasSearched ? (
                            <div className="flex flex-col items-center gap-3">
                                <svg className="w-12 h-12 text-gray-200 dark:text-gray-700" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                                    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.5} d="M9.172 9.172a4 4 0 015.656 0M9 10h.01M15 10h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
                                </svg>
                                <span className="text-sm text-center max-w-xs">No matches found. Try lowering the similarity threshold.</span>
                            </div>
                        ) : (
                            <div className="flex flex-col items-center gap-3">
                                <PhotoIcon className="w-12 h-12 text-gray-200 dark:text-gray-700" />
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
