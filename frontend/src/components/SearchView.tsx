import { useState, useEffect } from 'react';
import type { MediaItem } from '../types';
import MediaCard from './MediaCard';
import { apiFetch } from '../auth';

interface SearchViewProps {
    initialMediaId?: string | null;
}

export default function SearchView({ initialMediaId }: SearchViewProps) {
    const [searchFile, setSearchFile] = useState<File | null>(null);
    const [similarity, setSimilarity] = useState<number>(70);
    const [searchResults, setSearchResults] = useState<MediaItem[]>([]);
    const [isSearching, setIsSearching] = useState(false);
    const [activeId, setActiveId] = useState<string | null>(null);

    // Effect to handle prop changes (new search initiated from Gallery)
    useEffect(() => {
        if (initialMediaId) {
            setActiveId(initialMediaId);
            setSearchFile(null);
            handleSearchById(initialMediaId, similarity);
        }
    }, [initialMediaId]);

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
    };

    // Helper to get thumbnail URL (duplicated from MediaCard logic, simplified)
    const getThumbnailUrl = (uuid: string) => {
        const p1 = uuid.substring(0, 2);
        const p2 = uuid.substring(2, 4);
        return `/thumbnails/${p1}/${p2}/${uuid}.jpg`;
    };

    return (
        <div className="max-w-7xl mx-auto h-full flex flex-col">
            <h2 className="text-3xl font-bold text-gray-900 mb-6 shrink-0">Visual Search</h2>

            <div className="bg-white border border-gray-200 p-8 rounded-2xl shadow-sm mb-8 shrink-0">
                <div className="flex flex-col md:flex-row gap-8 items-start md:items-center">
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
                    <div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 gap-6 pb-8">
                        {searchResults.map((item) => (
                            <MediaCard key={`search-${item.id}`} item={item} />
                        ))}
                    </div>
                </div>
            ) : (
                <div className="flex-1 flex items-center justify-center text-gray-400">
                    {isSearching ? 'Analyzing...' : 'No matches found or no search initiated.'}
                </div>
            )}
        </div>
    );
}
