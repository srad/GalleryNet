import { useState, useEffect, useCallback } from 'react';
import { useNavigate } from 'react-router-dom';
import type { PersonWithFace, Folder, FaceStats } from '../types';
import { apiClient } from '../api';
import LoadingIndicator from './LoadingIndicator';
import { LogoutIcon, UsersIcon } from './Icons';
import ConfirmDialog from './ConfirmDialog';
import FaceThumbnail from './FaceThumbnail';

interface PeopleViewProps {
    refreshKey: number;
    folders: Folder[];
    onFoldersChanged: () => void;
    onLogout?: () => void;
    isActive?: boolean;
}

export default function PeopleView({ refreshKey, isActive = true, onLogout }: PeopleViewProps) {
    const navigate = useNavigate();
    const [people, setPeople] = useState<PersonWithFace[]>([]);
    const [stats, setStats] = useState<FaceStats | null>(null);
    const [isLoading, setIsLoading] = useState(true);
    
    // Management State
    const [selectionMode, setSelectionMode] = useState(false);
    const [selected, setSelected] = useState<Set<string>>(new Set());
    const [editingId, setEditingId] = useState<string | null>(null);
    const [editName, setEditName] = useState("");
    const [showMergeDialog, setShowMergeDialog] = useState(false);
    const [mergeName, setMergeName] = useState("");
    const [isMerging, setIsMerging] = useState(false);

    const fetchPeople = useCallback(async () => {
        setIsLoading(true);
        try {
            const [peopleData, statsData] = await Promise.all([
                apiClient.getPeople(),
                apiClient.getFaceStats()
            ]);
            setPeople(peopleData);
            setStats(statsData);
        } catch (e) {
            console.error('Failed to fetch people or stats:', e);
        } finally {
            setIsLoading(false);
        }
    }, []);

    useEffect(() => {
        if (isActive) fetchPeople();
    }, [isActive, refreshKey, fetchPeople]);

    const handlePersonClick = (id: string) => {
        if (selectionMode) {
            setSelected(prev => {
                const next = new Set(prev);
                if (next.has(id)) next.delete(id);
                else next.add(id);
                return next;
            });
            return;
        }
        // Navigate to gallery filtered by this person
        navigate(`/?person=${id}`);
    };

    const startNaming = (e: React.MouseEvent, id: string, name: string) => {
        e.stopPropagation();
        setEditingId(id);
        setEditName(name || "");
    };

    const saveName = async () => {
        if (!editingId) return;
        try {
            await apiClient.updatePerson(editingId, { name: editName });
            setPeople(prev => prev.map(p => p[0].id === editingId ? [{ ...p[0], name: editName }, p[1], p[2]] : p));
            setEditingId(null);
        } catch (e) {
            console.error('Failed to update name', e);
        }
    };

    const handleMerge = async () => {
        if (selected.size < 2) return;
        setIsMerging(true);
        try {
            // Merge logic: in backend we take source and target.
            // For multi-merge, we can pick one as target and others as sources.
            const ids = Array.from(selected);
            const targetId = ids[0];
            const sourceIds = ids.slice(1);
            
            for (const sourceId of sourceIds) {
                await apiClient.mergePeople(sourceId, targetId);
            }
            
            if (mergeName) {
                await apiClient.updatePerson(targetId, { name: mergeName });
            }

            setSelectionMode(false);
            setSelected(new Set());
            setShowMergeDialog(false);
            setMergeName("");
            fetchPeople();
        } catch (e) {
            console.error('Merge failed', e);
        } finally {
            setIsMerging(false);
        }
    };

    const toggleHide = async (e: React.MouseEvent, id: string) => {
        e.stopPropagation();
        try {
            await apiClient.updatePerson(id, { is_hidden: true });
            setPeople(prev => prev.filter(p => p[0].id !== id));
        } catch (e) {
            console.error('Hide failed', e);
        }
    };

    if (!isActive) return null;

    return (
        <div className="h-full flex flex-col p-4 md:p-8">
            <div className="flex items-center justify-between mb-6 shrink-0">
                <div className="flex items-center gap-4">
                    <h2 className="text-2xl md:text-3xl font-bold text-gray-900 dark:text-gray-100">People</h2>
                    {people.length > 0 && (
                        <button
                            onClick={() => {
                                setSelectionMode(!selectionMode);
                                setSelected(new Set());
                            }}
                            className={`px-3 py-1 text-xs font-bold rounded-full border transition-all ${selectionMode ? 'bg-blue-600 border-blue-600 text-white' : 'bg-white dark:bg-gray-800 text-gray-600 dark:text-gray-400 border-gray-200 dark:border-gray-700 hover:border-gray-300'}`}
                        >
                            {selectionMode ? 'Cancel Selection' : 'Manage / Merge'}
                        </button>
                    )}
                </div>
                
                <div className="flex items-center gap-3">
                    {selectionMode && selected.size >= 2 && (
                        <button
                            onClick={() => setShowMergeDialog(true)}
                            className="flex items-center gap-2 px-4 py-2 bg-blue-600 text-white text-sm font-bold rounded-xl shadow-lg hover:bg-blue-700 transition-all animate-in fade-in zoom-in"
                        >
                            Merge {selected.size} People
                        </button>
                    )}
                    
                    {onLogout && (
                        <button
                            onClick={onLogout}
                            className="flex items-center gap-2 px-3 py-2 text-sm font-semibold text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-gray-100 hover:bg-white/50 dark:hover:bg-gray-800 rounded-xl transition-all active:scale-95 border border-transparent hover:border-gray-200 dark:hover:border-gray-700"
                        >
                            <LogoutIcon />
                        </button>
                    )}
                </div>
            </div>

            {stats && (
                <div className="grid grid-cols-2 md:grid-cols-6 gap-4 mb-8">
                    <div className="bg-white dark:bg-gray-800 p-4 rounded-2xl border border-gray-100 dark:border-gray-700 shadow-sm">
                        <div className="text-xs font-bold text-gray-500 uppercase tracking-wider mb-1">Total Faces</div>
                        <div className="text-2xl font-black text-gray-900 dark:text-gray-100">{stats.total_faces}</div>
                    </div>
                    <div className="bg-white dark:bg-gray-800 p-4 rounded-2xl border border-gray-100 dark:border-gray-700 shadow-sm">
                        <div className="text-xs font-bold text-gray-500 uppercase tracking-wider mb-1">Total People</div>
                        <div className="text-2xl font-black text-gray-900 dark:text-gray-100">{stats.total_people}</div>
                    </div>
                    <div className="bg-white dark:bg-gray-800 p-4 rounded-2xl border border-gray-100 dark:border-gray-700 shadow-sm">
                        <div className="text-xs font-bold text-gray-500 uppercase tracking-wider mb-1">Named</div>
                        <div className="text-2xl font-black text-blue-600 dark:text-blue-400">{stats.named_people}</div>
                    </div>
                    <div className="bg-white dark:bg-gray-800 p-4 rounded-2xl border border-gray-100 dark:border-gray-700 shadow-sm">
                        <div className="text-xs font-bold text-gray-500 uppercase tracking-wider mb-1">Ungrouped</div>
                        <div className="text-2xl font-black text-amber-600 dark:text-amber-400">{stats.ungrouped_faces}</div>
                    </div>
                    <div className="bg-white dark:bg-gray-800 p-4 rounded-2xl border border-gray-100 dark:border-gray-700 shadow-sm">
                        <div className="text-xs font-bold text-gray-500 uppercase tracking-wider mb-1">Unassigned</div>
                        <div className="text-2xl font-black text-rose-600 dark:text-rose-400">{stats.unassigned_faces}</div>
                    </div>
                    <div className="bg-white dark:bg-gray-800 p-4 rounded-2xl border border-gray-100 dark:border-gray-700 shadow-sm">
                        <div className="text-xs font-bold text-gray-500 uppercase tracking-wider mb-1">Hidden</div>
                        <div className="text-2xl font-black text-gray-400 dark:text-gray-600">{stats.hidden_people}</div>
                    </div>
                </div>
            )}

            {isLoading && people.length === 0 ? (
                <div className="flex-1 flex items-center justify-center">
                    <LoadingIndicator size="lg" label="Identifying people..." />
                </div>
            ) : people.length > 0 ? (
                <div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-6 xl:grid-cols-8 gap-6 overflow-y-auto pb-8">
                    {people.map(([person, face, media]) => {
                        const isSelected = selected.has(person.id);
                        
                        return (
                            <div key={person.id} className="relative group">
                                <div
                                    data-testid={`person-card-${person.id}`}
                                    onClick={() => handlePersonClick(person.id)}
                                    className={`w-full relative flex flex-col gap-3 text-left transition-all duration-300 cursor-pointer ${isSelected ? 'scale-95' : ''}`}
                                >
                                    <div className={`aspect-square relative rounded-3xl overflow-hidden bg-gray-200 dark:bg-gray-800 shadow-sm transition-all duration-500 ring-2 ${isSelected ? 'ring-blue-500 shadow-blue-500/20' : 'ring-transparent'}`}>
                                        {face && media ? (
                                            <FaceThumbnail 
                                                face={face} 
                                                media={media} 
                                                className="w-full h-full"
                                            />
                                        ) : (
                                            <div className="w-full h-full flex items-center justify-center text-gray-400">
                                                <UsersIcon className="w-1/2 h-1/2 opacity-20" />
                                            </div>
                                        )}
                                        
                                        <div className="absolute inset-0 bg-gradient-to-t from-black/60 via-transparent to-transparent opacity-60 group-hover:opacity-80 transition-opacity pointer-events-none" />
                                        
                                        {/* Selection Checkmark */}
                                        {selectionMode && (
                                            <div className={`absolute top-3 right-3 w-6 h-6 rounded-full border-2 flex items-center justify-center transition-all ${isSelected ? 'bg-blue-500 border-blue-500 text-white' : 'bg-black/20 border-white/50'}`}>
                                                {isSelected && (
                                                    <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={3}>
                                                        <path strokeLinecap="round" strokeLinejoin="round" d="M5 13l4 4L19 7" />
                                                    </svg>
                                                )}
                                            </div>
                                        )}

                                        {/* Actions Overlay */}
                                        {!selectionMode && (
                                            <button 
                                                onClick={(e) => toggleHide(e, person.id)}
                                                className="absolute top-3 right-3 p-1.5 rounded-full bg-black/40 text-white/70 opacity-0 group-hover:opacity-100 hover:bg-black/60 hover:text-white transition-all backdrop-blur-sm"
                                                title="Hide this person"
                                            >
                                                <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                                                    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M13.875 18.825A10.05 10.05 0 0112 19c-4.478 0-8.268-2.943-9.542-7a9.97 9.97 0 011.563-3.029m5.858.908a3 3 0 114.243 4.243M9.878 9.878l4.242 4.242M9.88 9.88l-3.29-3.29m7.532 7.532l3.29 3.29M3 3l18 18" />
                                                </svg>
                                            </button>
                                        )}
                                    </div>

                                    <div className="px-1">
                                        {editingId === person.id ? (
                                            <input
                                                autoFocus
                                                value={editName}
                                                onChange={(e) => setEditName(e.target.value)}
                                                onBlur={saveName}
                                                onKeyDown={(e) => e.key === 'Enter' && saveName()}
                                                onClick={(e) => e.stopPropagation()}
                                                className="w-full bg-white dark:bg-gray-800 border border-blue-500 rounded px-2 py-1 text-sm font-bold text-gray-900 dark:text-gray-100 outline-none shadow-sm"
                                            />
                                        ) : (
                                            <div className="flex items-center justify-between group/name">
                                                <div className="flex flex-col truncate">
                                                    <span 
                                                        className={`text-sm font-bold truncate transition-colors ${person.name ? 'text-gray-900 dark:text-gray-100' : 'text-gray-400 italic'}`}
                                                    >
                                                        {person.name || `Unnamed Person`}
                                                    </span>
                                                    <span className="text-[10px] text-gray-500 font-medium">{person.face_count} photos</span>
                                                </div>
                                                {!selectionMode && (
                                                    <button 
                                                        onClick={(e) => startNaming(e, person.id, person.name)}
                                                        className="opacity-0 group-hover/name:opacity-100 p-1 text-blue-500 hover:text-blue-600 transition-all"
                                                    >
                                                        <svg className="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                                                            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M15.232 5.232l3.536 3.536m-2.036-5.036a2.5 2.5 0 113.536 3.536L6.5 21.036H3v-3.572L16.732 3.732z" />
                                                        </svg>
                                                    </button>
                                                )}
                                            </div>
                                        )}
                                    </div>
                                </div>
                            </div>
                        );
                    })}
                </div>
            ) : (
                <div className="flex-1 flex flex-col items-center justify-center text-gray-400 dark:text-gray-500 gap-4">
                    <div className="w-20 h-20 rounded-full bg-gray-100 dark:bg-gray-800 flex items-center justify-center text-gray-300 dark:text-gray-700">
                        <UsersIcon className="w-10 h-10" />
                    </div>
                    <p className="text-center max-w-xs">No people identified yet. Face scanning happens automatically in the background.</p>
                </div>
            )}

            <ConfirmDialog
                isOpen={showMergeDialog}
                title="Merge People"
                message={`Merge ${selected.size} profiles into one? Enter a name for the new profile:`}
                confirmLabel={isMerging ? "Merging..." : "Merge Profiles"}
                onConfirm={handleMerge}
                onCancel={() => setShowMergeDialog(false)}
                isDestructive={false}
            >
                <input
                    autoFocus
                    placeholder="New profile name"
                    value={mergeName}
                    onChange={(e) => setMergeName(e.target.value)}
                    className="w-full mt-4 bg-gray-50 dark:bg-gray-900 border border-gray-200 dark:border-gray-700 rounded-xl px-4 py-3 text-sm focus:ring-2 focus:ring-blue-500 outline-none transition-all"
                />
            </ConfirmDialog>
        </div>
    );
}
