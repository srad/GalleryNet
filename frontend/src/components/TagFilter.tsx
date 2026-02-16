import { useState, useEffect, useRef, useMemo } from 'react';
import { apiFetch } from '../auth';
import { TagIcon } from './Icons';

interface TagFilterProps {
    selectedTags: string[];
    onChange: (tags: string[]) => void;
    refreshKey?: number;
}

export default function TagFilter({ selectedTags, onChange, refreshKey }: TagFilterProps) {
    const [allTags, setAllTags] = useState<string[]>([]);
    const [isOpen, setIsOpen] = useState(false);
    const [searchTerm, setSearchTerm] = useState('');
    const wrapperRef = useRef<HTMLDivElement>(null);

    useEffect(() => {
        apiFetch('/api/tags')
            .then(res => res.ok ? res.json() : [])
            .then(setAllTags)
            .catch(() => {});
    }, [refreshKey]);

    const filteredTags = useMemo(() => {
        if (!searchTerm) return allTags;
        const lower = searchTerm.toLowerCase();
        return allTags.filter(t => t.toLowerCase().includes(lower));
    }, [allTags, searchTerm]);

    const toggleTag = (tag: string) => {
        if (selectedTags.includes(tag)) {
            onChange(selectedTags.filter(t => t !== tag));
        } else {
            onChange([...selectedTags, tag]);
        }
    };

    // Close on click outside
    useEffect(() => {
        const handler = (e: MouseEvent) => {
            if (wrapperRef.current && !wrapperRef.current.contains(e.target as Node)) {
                setIsOpen(false);
            }
        };
        if (isOpen) {
            document.addEventListener('mousedown', handler);
        }
        return () => document.removeEventListener('mousedown', handler);
    }, [isOpen]);

    return (
        <div ref={wrapperRef} className="relative">
            <div
                onClick={() => setIsOpen(!isOpen)}
                className={`flex items-center gap-1.5 px-2.5 sm:px-3 py-1.5 text-xs sm:text-sm font-medium rounded-lg shadow-sm transition-colors border cursor-pointer ${
                    selectedTags.length > 0 || isOpen
                        ? 'bg-blue-50 text-blue-600 border-blue-200 hover:bg-blue-100'
                        : 'text-gray-600 bg-white border-gray-300 hover:bg-gray-50'
                }`}
                title="Filter by tags"
            >
                <TagIcon />
                <span className="hidden sm:inline">Tags</span>
                {selectedTags.length > 0 && (
                    <span className="flex items-center justify-center min-w-[1.25rem] h-5 px-1 text-[10px] font-bold text-white bg-blue-600 rounded-full ml-0.5">
                        {selectedTags.length}
                    </span>
                )}
            </div>

            {isOpen && (
                <div className="absolute right-0 z-50 mt-2 w-64 bg-white border border-gray-200 rounded-lg shadow-xl overflow-hidden flex flex-col max-h-[80vh] sm:max-h-96">
                    <div className="p-2 border-b border-gray-100 bg-gray-50">
                        <input
                            type="text"
                            value={searchTerm}
                            onChange={e => setSearchTerm(e.target.value)}
                            placeholder="Search tags..."
                            className="w-full px-2 py-1.5 text-sm bg-white border border-gray-300 rounded focus:outline-none focus:border-blue-500 focus:ring-1 focus:ring-blue-500/20 text-gray-900"
                            autoFocus
                        />
                    </div>
                    
                    <div className="overflow-y-auto flex-1 p-1">
                        {filteredTags.length === 0 ? (
                            <div className="px-3 py-4 text-center text-sm text-gray-500 italic">
                                {allTags.length === 0 ? "No tags found" : "No matching tags"}
                            </div>
                        ) : (
                            filteredTags.map(tag => {
                                const isSelected = selectedTags.includes(tag);
                                return (
                                    <button
                                        key={tag}
                                        onClick={() => toggleTag(tag)}
                                        className={`flex items-center w-full px-3 py-2 text-sm text-left rounded-md transition-colors ${
                                            isSelected 
                                                ? 'bg-blue-50 text-blue-700 font-medium' 
                                                : 'text-gray-700 hover:bg-gray-100'
                                        }`}
                                    >
                                        <div className={`w-4 h-4 mr-2 border rounded flex items-center justify-center transition-colors ${
                                            isSelected ? 'bg-blue-600 border-blue-600' : 'border-gray-400 bg-white'
                                        }`}>
                                            {isSelected && (
                                                <svg className="w-3 h-3 text-white" fill="none" viewBox="0 0 24 24" strokeWidth={3} stroke="currentColor">
                                                    <path strokeLinecap="round" strokeLinejoin="round" d="M4.5 12.75l6 6 9-13.5" />
                                                </svg>
                                            )}
                                        </div>
                                        <span className="truncate">{tag}</span>
                                    </button>
                                );
                            })
                        )}
                    </div>

                    {selectedTags.length > 0 && (
                        <div className="p-2 border-t border-gray-100 bg-gray-50 flex justify-end">
                            <button
                                onClick={() => onChange([])}
                                className="text-xs font-medium text-red-600 hover:text-red-800 px-2 py-1"
                            >
                                Clear filters
                            </button>
                        </div>
                    )}
                </div>
            )}
        </div>
    );
}
