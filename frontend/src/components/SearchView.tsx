import { useState, useEffect, useCallback } from 'react';
import { useSearchParams } from 'react-router-dom';
import type { MediaItem } from '../types';
import MediaCard from './MediaCard';
import MediaModal from './MediaModal';
import { apiFetch } from '../auth';

export default function SearchView() {
    const [searchParams, setSearchParams] = useSearchParams();
    const sourceId = searchParams.get('source');
    const selectedMediaId = searchParams.get('media');

    const [searchFile, setSearchFile] = useState<File | null>(null);
    const [similarity, setSimilarity] = useState<number>(70);
    const [searchResults, setSearchResults] = useState<MediaItem[]>([]);
    const [isSearching, setIsSearching] = useState(false);
    const [activeId, setActiveId] = useState<string | null>(null);

    // Effect to handle prop changes (new search initiated from Gallery via URL)
    useEffect(() => {
        if (sourceId && sourceId !== activeId) {
            setActiveId(sourceId);
            setSearchFile(null);
            handleSearchById(sourceId, similarity);
        } else if (!sourceId && !searchFile) {
            // Cleared via URL?
            if (activeId) {
                setActiveId(null);
                setSearchResults([]);
            }
        }
    }, [sourceId]);

    const handleSearchById = async (id: string, sim: number) => {
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
    };

    const handleSearch = async () => {
        if (activeId) {
            // If we have an active ID, update the URL to reflect the search if not already
            // (Though typically activeId comes FROM the URL)
            handleSearchById(activeId, similarity);
            return;
        }

        if (!searchFile) return alert('Please select a file to search with');
        setIsSearching(true);

        const formData = new FormData();
        formData.append('similarity', similarity.toString());
        formData.append('file', searchFile);

        try {
            const res = await apiFetch('/api/search', { method: 'POST', body: formData });
            const results = await res.json();
            setSearchResults(results);
        } catch (e) {
            alert(`Search error: ${e}`);
        } finally {
            setIsSearching(false);
        }
    };

    const clearSearch = () => {
        setActiveId(null);
        setSearchFile(null);
        setSearchResults([]);
        // Clear source param
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

    // Render modal logic
    const renderModal = () => {
        if (!selectedMediaId) return null;
        
        const idx = searchResults.findIndex(m => m.id === selectedMediaId);
        if (idx === -1) return null; // Item not in results? Maybe fetching full detail? 
        // If searching via ID, the source item might not be in results (it's the query).
        // But the user clicked a result.
        
        const item = searchResults[idx];
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
                // Search view doesn't support favorite/delete/tags context fully yet or passed props?
                // MediaModal handles specific item actions via API.
                // We just need to handle the list updates if an item is deleted.
                // For now, let's omit callback for delete to avoid complex state sync, 
                // or just remove from local results.
                onToggleFavorite={() => {
                    // Optimistic update locally
                    const newStatus = !item.is_favorite;
                    setSearchResults(prev => prev.map(m => m.id === item.id ? { ...m, is_favorite: newStatus } : m));
                    // API call is handled inside MediaModal? No, MediaModal expects parent to handle it?
                    // Wait, MediaModal renders buttons that call props. 
                    // But in GalleryView we had `handleToggleFavorite`.
                    // We need to duplicate that logic here or accept it as prop.
                    // For simplicity, let's implement basic favorite toggle here.
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
            <h2 className="text-2xl md:text-3xl font-bold text-gray-900 mb-4 md:mb-6 shrink-0">Visual Search</h2>

            <div className="bg-white border border-gray-200 p-4 sm:p-6 md:p-8 rounded-2xl shadow-sm mb-4 sm:mb-8 shrink-0">
                <div className="flex flex-col md:flex-row gap-4 sm:gap-6 md:gap-8 items-start md:items-center">
                    <div className="flex-1 w-full">
                        <label className="block text-sm font-medium text-gray-700 mb-2">Reference Image</label>
                        
                        {activeId ? (
                            <div className="flex items-center gap-4 p-2 border border-gray-200 rounded-lg bg-gray-50">
                                <img 
                                    src={getThumbnailUrl(activeId)} 
                                    alt="Reference" 
                                    className="w-16 h-16 object-cover rounded-md"
                                />
                                <div className="flex-1">
                                    <p className="text-sm font-medium text-gray-900">Searching by selected image</p>
                                    <button 
                                        onClick={clearSearch}
                                        className="text-xs text-red-600 hover:text-red-800 underline mt-1"
                                    >
                                        Clear / Upload New
                                    </button>
                                </div>
                            </div>
                        ) : (
                            <input
                                type="file"
                                accept="image/*"
                                onChange={(e) => setSearchFile(e.target.files?.[0] || null)}
                                className="block w-full text-sm text-gray-500 file:mr-4 file:py-2.5 file:px-4 file:rounded-lg file:border-0 file:text-sm file:font-semibold file:bg-purple-50 file:text-purple-700 hover:file:bg-purple-100 cursor-pointer border border-gray-200 rounded-lg"
                            />
                        )}
                    </div>

                    <div className="flex-1 w-full">
                        <label htmlFor="similarity" className="block text-sm font-medium text-gray-700 mb-2">
                            Min Similarity: <span className="text-purple-600 font-bold">{similarity}%</span>
                        </label>
                        <input
                            id="similarity"
                            type="range"
                            min="0"
                            max="100"
                            value={similarity}
                            onChange={(e) => setSimilarity(Number(e.target.value))}
                            className="w-full accent-purple-600 cursor-pointer h-2 bg-gray-200 rounded-lg appearance-none"
                        />
                    </div>

                    <button
                        onClick={handleSearch}
                        disabled={isSearching || (!searchFile && !activeId)}
                        className="mt-6 md:mt-0 whitespace-nowrap bg-purple-600 text-white px-8 py-3 rounded-lg font-medium hover:bg-purple-700 disabled:opacity-50 disabled:cursor-not-allowed transition-all shadow-sm"
                    >
                        {isSearching ? 'Searching...' : 'Find Matches'}
                    </button>
                </div>
            </div>

            {searchResults.length > 0 ? (
                <div className="flex-1 overflow-y-auto min-h-0">
                    <h3 className="text-lg font-semibold text-gray-900 mb-4 border-b pb-2 sticky top-0 bg-gray-50 z-10">
                        Matches Found ({searchResults.length})
                    </h3>
                    <div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 gap-2 sm:gap-4 md:gap-6 pb-8">
                        {searchResults.map((item) => (
                            <MediaCard 
                                key={`search-${item.id}`} 
                                item={item}
                                onClick={() => handleCardClick(item)}
                            />
                        ))}
                    </div>
                </div>
            ) : (
                <div className="flex-1 flex items-center justify-center text-gray-400">
                    {isSearching ? 'Analyzing...' : 'No matches found or no search initiated.'}
                </div>
            )}

            {renderModal()}
        </div>
    );
}
