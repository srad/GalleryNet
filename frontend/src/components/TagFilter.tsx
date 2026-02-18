import { useState, useEffect, useRef, useMemo } from 'react';
import { apiClient } from '../api';
import type { TagCount } from '../types';
import { TagIcon } from './Icons';

interface TagFilterProps {
    selectedTags: string[];
    onChange: (tags: string[]) => void;
    refreshKey?: number;
}

export default function TagFilter({ selectedTags, onChange, refreshKey }: TagFilterProps) {
    const [allTags, setAllTags] = useState<TagCount[]>([]);
    const [isOpen, setIsOpen] = useState(false);
    const [searchTerm, setSearchTerm] = useState('');
    const wrapperRef = useRef<HTMLDivElement>(null);

    useEffect(() => {
        apiClient.getTags()
            .then(setAllTags)
            .catch(e => console.error('Failed to load tags:', e));
    }, [refreshKey]);

    const filteredTags = useMemo(() => {
        if (!searchTerm) return allTags;
        const lower = searchTerm.toLowerCase();
        return allTags.filter(t => t.name.toLowerCase().includes(lower));
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
        <div ref={wrapperRef} className="relative z-20">
            <button
                onClick={() => setIsOpen(!isOpen)}
                className={`
                    flex items-center gap-2 px-3 py-1.5 text-sm font-medium rounded-lg border transition-all active:scale-95
                    ${selectedTags.length > 0 || isOpen
                        ? 'bg-blue-50 dark:bg-blue-950 text-blue-700 dark:text-blue-300 border-blue-200 dark:border-blue-800 hover:bg-blue-100 dark:hover:bg-blue-900'
                        : 'bg-white dark:bg-gray-800 text-gray-600 dark:text-gray-300 border-gray-200 dark:border-gray-600 hover:border-gray-300 hover:bg-gray-50 dark:hover:bg-gray-700'
                    }
                `}
                title="Filter by tags"
            >
                <TagIcon />
                <span className="hidden sm:inline">Tags</span>
                {selectedTags.length > 0 && (
                    <span className="flex items-center justify-center min-w-[1.25rem] h-5 px-1.5 text-[10px] font-bold text-white bg-blue-600 rounded-full">
                        {selectedTags.length}
                    </span>
                )}
            </button>

            {isOpen && (
                <div className="absolute top-full mt-2 left-0 z-50 w-72 bg-white/95 dark:bg-gray-800/95 backdrop-blur-xl border border-gray-200/60 dark:border-gray-700/60 rounded-xl shadow-2xl ring-1 ring-black/5 overflow-hidden flex flex-col max-h-[80vh] sm:max-h-96 animate-in fade-in zoom-in-95 duration-100 origin-top-left">
                    <div className="p-3 border-b border-gray-100 dark:border-gray-700 bg-gray-50/50 dark:bg-gray-900/50">
                        <div className="relative">
                            <svg className="absolute left-2.5 top-1/2 -translate-y-1/2 w-4 h-4 text-gray-400 dark:text-gray-500" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor">
                                <path strokeLinecap="round" strokeLinejoin="round" d="M21 21l-5.197-5.197m0 0A7.5 7.5 0 105.196 5.196a7.5 7.5 0 0010.607 10.607z" />
                            </svg>
                            <input
                                type="text"
                                value={searchTerm}
                                onChange={e => setSearchTerm(e.target.value)}
                                placeholder="Search tags..."
                                className="w-full pl-9 pr-3 py-2 text-sm bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-600 rounded-lg focus:outline-none focus:border-blue-500 focus:ring-2 focus:ring-blue-500/20 text-gray-900 dark:text-gray-100 placeholder-gray-400 dark:placeholder-gray-500 transition-all"
                                autoFocus
                            />
                        </div>
                    </div>

                    <div className="overflow-y-auto flex-1 p-1.5 scrollbar-thin scrollbar-thumb-gray-200 scrollbar-track-transparent">
                        {filteredTags.length === 0 ? (
                            <div className="px-4 py-8 text-center flex flex-col items-center justify-center text-gray-400 dark:text-gray-500">
                                <TagIcon />
                                <span className="mt-2 text-sm italic">{allTags.length === 0 ? "No tags found" : "No matching tags"}</span>
                            </div>
                        ) : (
                            <div className="grid grid-cols-1 gap-0.5">
                                {filteredTags.map(tagObj => {
                                    const tag = tagObj.name;
                                    const isSelected = selectedTags.includes(tag);
                                    return (
                                        <button
                                            key={tag}
                                            onClick={() => toggleTag(tag)}
                                            className={`
                                                group flex items-center w-full px-3 py-2 text-sm text-left rounded-lg transition-all
                                                ${isSelected
                                                    ? 'bg-blue-50 dark:bg-blue-950 text-blue-700 dark:text-blue-300'
                                                    : 'text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700'
                                                }
                                            `}
                                        >
                                            <div className={`
                                                w-4 h-4 mr-3 border rounded flex items-center justify-center transition-all flex-shrink-0
                                                ${isSelected
                                                    ? 'bg-blue-600 border-blue-600 shadow-sm'
                                                    : 'border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 group-hover:border-gray-400 dark:group-hover:border-gray-500'
                                                }
                                            `}>
                                                {isSelected && (
                                                    <svg className="w-3 h-3 text-white" fill="none" viewBox="0 0 24 24" strokeWidth={3} stroke="currentColor">
                                                        <path strokeLinecap="round" strokeLinejoin="round" d="M4.5 12.75l6 6 9-13.5" />
                                                    </svg>
                                                )}
                                            </div>
                                            <span className="truncate flex-1 font-medium">{tag}</span>
                                            <span className={`text-xs ml-2 px-1.5 py-0.5 rounded-full ${isSelected ? 'bg-blue-100 dark:bg-blue-900 text-blue-600 dark:text-blue-300' : 'bg-gray-100 dark:bg-gray-700 text-gray-500 dark:text-gray-400'}`}>
                                                {tagObj.count}
                                            </span>
                                        </button>
                                    );
                                })}
                            </div>
                        )}
                    </div>

                    {selectedTags.length > 0 && (
                        <div className="p-2 border-t border-gray-100 dark:border-gray-700 bg-gray-50/80 dark:bg-gray-900/80 backdrop-blur-sm flex justify-between items-center">
                            <span className="text-xs text-gray-500 dark:text-gray-400 ml-2">{selectedTags.length} selected</span>
                            <button
                                onClick={() => onChange([])}
                                className="text-xs font-semibold text-red-600 dark:text-red-400 hover:text-red-700 dark:hover:text-red-300 hover:bg-red-50 dark:hover:bg-red-950 px-3 py-1.5 rounded-md transition-colors"
                            >
                                Clear all
                            </button>
                        </div>
                    )}
                </div>
            )}
        </div>
    );

}
