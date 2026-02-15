import { useState, useEffect, useRef, useMemo } from 'react';
import { apiFetch } from '../auth';

interface TagInputProps {
    value: string[];
    onChange: (tags: string[]) => void;
    placeholder?: string;
    readOnly?: boolean;
}

export default function TagInput({ value, onChange, placeholder = "Add tags...", readOnly = false }: TagInputProps) {
    const [inputValue, setInputValue] = useState('');
    const [allTags, setAllTags] = useState<string[]>([]);
    const [showSuggestions, setShowSuggestions] = useState(false);
    const wrapperRef = useRef<HTMLDivElement>(null);

    useEffect(() => {
        apiFetch('/api/tags')
            .then(res => res.ok ? res.json() : [])
            .then(setAllTags)
            .catch(() => {});
    }, []);

    const filteredSuggestions = useMemo(() => {
        if (!inputValue.trim()) return [];
        const lower = inputValue.toLowerCase();
        return allTags.filter(t => 
            t.toLowerCase().includes(lower) && !value.includes(t)
        );
    }, [allTags, inputValue, value]);

    const addTag = (tag: string) => {
        const trimmed = tag.trim();
        if (trimmed && !value.includes(trimmed)) {
            onChange([...value, trimmed]);
        }
        setInputValue('');
        setShowSuggestions(false);
    };

    const removeTag = (tag: string) => {
        onChange(value.filter(t => t !== tag));
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
            removeTag(value[value.length - 1]);
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
                {value.map(tag => (
                    <span key={tag} className="flex items-center gap-1 px-2 py-0.5 text-xs font-medium text-blue-700 bg-blue-50 rounded-md border border-blue-100">
                        {tag}
                        {!readOnly && (
                            <button
                                onClick={() => removeTag(tag)}
                                className="hover:text-blue-900 focus:outline-none"
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
