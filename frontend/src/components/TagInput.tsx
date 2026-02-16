import { useState, useEffect, useRef, useMemo } from 'react';
import { apiFetch } from '../auth';
import type { TagDetail } from '../types';

interface TagInputProps {
    value: (string | TagDetail)[];
    onChange: (tags: string[]) => void;
    placeholder?: string;
    readOnly?: boolean;
    autoFocus?: boolean;
}

export default function TagInput({ value, onChange, placeholder = "Add tags...", readOnly = false, autoFocus = false }: TagInputProps) {
    const [inputValue, setInputValue] = useState('');
    const [allTags, setAllTags] = useState<string[]>([]);
    const [showSuggestions, setShowSuggestions] = useState(false);
    const wrapperRef = useRef<HTMLDivElement>(null);

    // Normalize value to TagDetail[]
    const tagDetails = useMemo(() => value.map(t => 
        typeof t === 'string' ? { name: t, is_auto: false } as TagDetail : t
    ), [value]);

    const tagNames = useMemo(() => tagDetails.map(t => t.name), [tagDetails]);

    useEffect(() => {
        apiFetch('/api/tags')
            .then(res => res.ok ? res.json() : [])
            .then((data: any[]) => setAllTags(data.map(d => d.name)))
            .catch(() => {});
    }, []);

    const filteredSuggestions = useMemo(() => {
        if (!inputValue.trim()) return [];
        const lower = inputValue.toLowerCase();
        return allTags.filter(t => 
            t.toLowerCase().includes(lower) && !tagNames.includes(t)
        );
    }, [allTags, inputValue, tagNames]);

    const addTag = (tag: string) => {
        const trimmed = tag.trim();
        if (trimmed && !tagNames.includes(trimmed)) {
            onChange([...tagNames, trimmed]);
        }
        setInputValue('');
        setShowSuggestions(false);
    };

    const removeTag = (tagName: string) => {
        onChange(tagNames.filter(t => t !== tagName));
    };

    const handleKeyDown = (e: React.KeyboardEvent) => {
        if (e.key === 'Enter') {
            e.preventDefault();
            if (filteredSuggestions.length > 0 && showSuggestions) {
                addTag(filteredSuggestions[0]);
            } else {
                addTag(inputValue);
            }
        } else if (e.key === 'Backspace' && !inputValue && value.length > 0) {
            const lastTag = value[value.length - 1];
            const name = typeof lastTag === 'string' ? lastTag : lastTag.name;
            removeTag(name);
        } else if (e.key === 'Escape') {
            setShowSuggestions(false);
        }
    };

    // Close suggestions on click outside
    useEffect(() => {
        const handler = (e: MouseEvent) => {
            if (wrapperRef.current && !wrapperRef.current.contains(e.target as Node)) {
                setShowSuggestions(false);
            }
        };
        document.addEventListener('mousedown', handler);
        return () => document.removeEventListener('mousedown', handler);
    }, []);

    return (
        <div ref={wrapperRef} className="relative w-full">
            <div className={`flex flex-wrap items-center gap-1.5 p-2 bg-white border border-gray-300 rounded-lg focus-within:ring-2 focus-within:ring-blue-500/20 focus-within:border-blue-500 ${readOnly ? 'bg-gray-50' : ''}`}>
                {tagDetails.map(tag => (
                    <span 
                        key={tag.name} 
                        className={`flex items-center gap-1 px-2 py-0.5 text-xs font-medium rounded-md border ${
                            tag.is_auto 
                            ? 'text-indigo-800 bg-indigo-50 border-indigo-50' 
                            : 'text-blue-700 bg-blue-50 border-blue-100'
                        }`}
                        title={tag.is_auto ? `Automatically assigned (confidence: ${Math.round((tag.confidence || 0) * 100)}%)` : undefined}
                    >
                        {tag.is_auto && (
                            <svg className="w-3 h-3 text-indigo-800" fill="none" viewBox="0 0 24 24" strokeWidth={2.5} stroke="currentColor">
                                <path strokeLinecap="round" strokeLinejoin="round" d="M9.813 15.904L9 18.75l-.813-2.846a4.5 4.5 0 00-3.09-3.09L2.25 12l2.846-.813a4.5 4.5 0 003.09-3.09L9 5.25l.813 2.846a4.5 4.5 0 003.09 3.09L15.75 12l-2.846.813a4.5 4.5 0 00-3.09 3.09z" />
                            </svg>
                        )}
                        {tag.name}
                        {!readOnly && (
                            <button
                                onClick={() => removeTag(tag.name)}
                                className="hover:opacity-60 focus:outline-none ml-0.5"
                            >
                                &times;
                            </button>
                        )}
                    </span>
                ))}
                {!readOnly && (
                    <input
                        type="text"
                        value={inputValue}
                        onChange={e => {
                            setInputValue(e.target.value);
                            setShowSuggestions(true);
                        }}
                        onKeyDown={handleKeyDown}
                        onFocus={() => setShowSuggestions(true)}
                        autoFocus={autoFocus}
                        placeholder={value.length === 0 ? placeholder : ''}
                        className="flex-1 min-w-[80px] text-sm outline-none bg-transparent text-gray-900 placeholder:text-gray-400"
                    />
                )}
            </div>

            {showSuggestions && inputValue && (
                <div className="absolute z-50 w-full mt-1 bg-white border border-gray-200 rounded-lg shadow-lg max-h-48 overflow-y-auto overflow-x-hidden">
                    {filteredSuggestions.length > 0 ? (
                        filteredSuggestions.map(tag => (
                            <button
                                key={tag}
                                onClick={() => addTag(tag)}
                                className="w-full text-left px-3 py-2 text-sm hover:bg-gray-50 text-gray-900 truncate transition-colors"
                            >
                                {tag}
                            </button>
                        ))
                    ) : (
                        <button
                            onClick={() => addTag(inputValue)}
                            className="w-full text-left px-3 py-2 text-sm hover:bg-gray-50 text-gray-900 italic transition-colors"
                        >
                            Create tag "{inputValue}"
                        </button>
                    )}
                </div>
            )}
        </div>
    );
}
