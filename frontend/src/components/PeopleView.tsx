import { useState, useEffect, useCallback } from 'react';
import { useNavigate } from 'react-router-dom';
import type { PersonSummary, Folder } from '../types';
import { apiClient } from '../api';
import LoadingIndicator from './LoadingIndicator';
import { LogoutIcon } from './Icons';

interface PeopleViewProps {
    refreshKey: number;
    folders: Folder[];
    onFoldersChanged: () => void;
    onLogout?: () => void;
    isActive?: boolean;
}

export default function PeopleView({ refreshKey, isActive = true, onLogout }: PeopleViewProps) {
    const navigate = useNavigate();
    const [people, setPeople] = useState<PersonSummary[]>([]);
    const [isLoading, setIsLoading] = useState(true);

    const fetchPeople = useCallback(async () => {
        setIsLoading(true);
        try {
            const data = await apiClient.getPeople();
            setPeople(data);
        } catch (e) {
            console.error('Failed to fetch people:', e);
        } finally {
            setIsLoading(false);
        }
    }, []);

    useEffect(() => {
        if (isActive) fetchPeople();
    }, [isActive, refreshKey, fetchPeople]);

    const getThumbnailUrl = (uuid: string) => {
        const p1 = uuid.substring(0, 2);
        const p2 = uuid.substring(2, 4);
        return `/thumbnails/${p1}/${p2}/${uuid}.jpg`;
    };

    const handlePersonClick = (person: PersonSummary) => {
        navigate(`/search?face=${person.representative_face.id}`);
    };

    if (!isActive) return null;

    return (
        <div className="h-full flex flex-col p-4 md:p-8">
            <div className="flex items-center justify-between mb-6 shrink-0">
                <h2 className="text-2xl md:text-3xl font-bold text-gray-900 dark:text-gray-100">People</h2>
                {onLogout && (
                    <button
                        onClick={onLogout}
                        className="flex items-center gap-2 px-3 py-2 text-sm font-semibold text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-gray-100 hover:bg-white/50 dark:hover:bg-gray-800 rounded-xl transition-all active:scale-95 border border-transparent hover:border-gray-200 dark:hover:border-gray-700"
                    >
                        <LogoutIcon />
                    </button>
                )}
            </div>

            {isLoading && people.length === 0 ? (
                <div className="flex-1 flex items-center justify-center">
                    <LoadingIndicator size="lg" label="Identifying people..." />
                </div>
            ) : people.length > 0 ? (
                <div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-6 xl:grid-cols-8 gap-4 overflow-y-auto pb-8">
                    {people.map((person) => {
                        const m = person.representative_media;
                        const f = person.representative_face;
                        
                        // Calculate crop style based on bounding box
                        // The thumbnail is 224x224, but media width/height might be different.
                        // However, thumbnails are generated via resize_to_fill(224, 224).
                        // The bounding boxes are in original image coordinates.
                        
                        const imgWidth = m.width || 1;
                        const imgHeight = m.height || 1;
                        
                        const fw = f.box_x2 - f.box_x1;
                        const fh = f.box_y2 - f.box_y1;
                        
                        // Percentage based positioning for the crop
                        const top = (f.box_y1 / imgHeight) * 100;
                        const left = (f.box_x1 / imgWidth) * 100;
                        const width = (fw / imgWidth) * 100;
                        const height = (fh / imgHeight) * 100;

                        return (
                            <button
                                key={person.cluster_id}
                                onClick={() => handlePersonClick(person)}
                                className="group relative flex flex-col gap-2 text-left animate-in fade-in zoom-in duration-300"
                            >
                                <div className="aspect-square relative overflow-hidden rounded-2xl bg-gray-200 dark:bg-gray-800 shadow-sm group-hover:shadow-md transition-all group-hover:scale-[1.02] ring-1 ring-black/5 dark:ring-white/10">
                                    {/* We show the full thumbnail but focus on the face using object-position if possible, 
                                        but a better way is to use a div with background-image and background-position/size */}
                                    <img
                                        src={getThumbnailUrl(m.id!)}
                                        alt="Person"
                                        className="w-full h-full object-cover grayscale-[0.2] group-hover:grayscale-0 transition-all duration-500"
                                        style={{
                                            objectPosition: `${left + width/2}% ${top + height/2}%`,
                                            transform: 'scale(1.5)' // Zoom in a bit on the face area
                                        }}
                                    />
                                    <div className="absolute inset-0 bg-gradient-to-t from-black/40 via-transparent to-transparent opacity-0 group-hover:opacity-100 transition-opacity" />
                                </div>
                                <span className="text-xs font-semibold text-gray-500 dark:text-gray-400 group-hover:text-blue-600 dark:group-hover:text-blue-400 transition-colors px-1">
                                    Person {person.cluster_id}
                                </span>
                            </button>
                        );
                    })}
                </div>
            ) : (
                <div className="flex-1 flex flex-col items-center justify-center text-gray-400 dark:text-gray-500 gap-4">
                    <div className="w-20 h-20 rounded-full bg-gray-100 dark:bg-gray-800 flex items-center justify-center">
                        <svg className="w-10 h-10" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.5} d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
                            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.5} d="M2.458 12C3.732 7.943 7.523 5 12 5c4.478 0 8.268 2.943 9.542 7-1.274 4.057-5.064 7-9.542 7-4.477 0-8.268-2.943-9.542-7z" />
                        </svg>
                    </div>
                    <p className="text-center max-w-xs">No people identified yet. Face scanning happens automatically in the background.</p>
                </div>
            )}
        </div>
    );
}
