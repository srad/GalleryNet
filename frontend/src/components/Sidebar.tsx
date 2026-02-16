import {useState, useCallback, useRef} from 'react';
import {Link, useLocation} from 'react-router-dom';
import {PhotoIcon, SearchIcon, HeartIcon} from './Icons';
import type {Folder} from '../types';
import {apiFetch} from '../auth';
import LibraryInfo from './LibraryInfo';
import ConfirmDialog from './ConfirmDialog';


interface SidebarProps {
    refreshKey: number;
    folders: Folder[];
    onFoldersChanged: () => void;
    /** When true, all navigation is disabled (e.g. during group computation) */
    disabled?: boolean;
    /** Whether the mobile sidebar drawer is open */
    mobileOpen?: boolean;
    /** Called to close the mobile sidebar drawer */
    onMobileClose?: () => void;
}

export default function Sidebar({refreshKey, folders, onFoldersChanged, disabled, mobileOpen, onMobileClose}: SidebarProps) {
    const location = useLocation();
    const [newFolderName, setNewFolderName] = useState('');
    const [isCreating, setIsCreating] = useState(false);

    // --- Rename state ---
    const [renamingId, setRenamingId] = useState<string | null>(null);
    const [renameValue, setRenameValue] = useState('');
    const renameInputRef = useRef<HTMLInputElement>(null);

    // --- Delete confirmation state ---
    const [folderToDelete, setFolderToDelete] = useState<Folder | null>(null);

    // --- Drag-to-reorder state ---
    const [dragIndex, setDragIndex] = useState<number | null>(null);
    const [dropIndex, setDropIndex] = useState<number | null>(null);
    const [mediaDropTargetId, setMediaDropTargetId] = useState<string | null>(null);
    const [dropSuccessId, setDropSuccessId] = useState<string | null>(null);
    const dragCounterRef = useRef(0);



    const handleCreateFolder = useCallback(async () => {
        const name = newFolderName.trim();
        if (!name) return;
        setIsCreating(true);
        try {
            const res = await apiFetch('/api/folders', {
                method: 'POST',
                headers: {'Content-Type': 'application/json'},
                body: JSON.stringify({name}),
            });
            if (res.ok) {
                setNewFolderName('');
                onFoldersChanged();
            }
        } catch { /* ignore */ }
        setIsCreating(false);
    }, [newFolderName, onFoldersChanged]);

    const handleDeleteFolder = useCallback(async (e: React.MouseEvent, folder: Folder) => {
        e.preventDefault(); // Prevent navigation
        e.stopPropagation();
        setFolderToDelete(folder);
    }, []);

    const confirmDeleteFolder = useCallback(async () => {
        if (!folderToDelete) return;
        try {
            const res = await apiFetch(`/api/folders/${folderToDelete.id}`, {method: 'DELETE'});
            if (res.ok) onFoldersChanged();
        } catch { /* ignore */ }
        setFolderToDelete(null);
    }, [folderToDelete, onFoldersChanged]);


    // --- Rename handlers ---
    const startRename = useCallback((e: React.MouseEvent, folder: Folder) => {
        e.preventDefault(); // Prevent navigation
        e.stopPropagation();
        setRenamingId(folder.id);
        setRenameValue(folder.name);
        // Focus the input after render
        setTimeout(() => renameInputRef.current?.focus(), 0);
    }, []);

    const commitRename = useCallback(async () => {
        if (!renamingId) return;
        const name = renameValue.trim();
        if (!name) {
            setRenamingId(null);
            return;
        }
        try {
            const res = await apiFetch(`/api/folders/${renamingId}`, {
                method: 'PUT',
                headers: {'Content-Type': 'application/json'},
                body: JSON.stringify({name}),
            });
            if (res.ok) onFoldersChanged();
        } catch { /* ignore */ }
        setRenamingId(null);
    }, [renamingId, renameValue, onFoldersChanged]);

    const cancelRename = useCallback(() => {
        setRenamingId(null);
    }, []);

    // --- Drag-to-reorder handlers ---
    const handleDragStart = useCallback((e: React.DragEvent, index: number) => {
        setDragIndex(index);
        e.dataTransfer.effectAllowed = 'move';
        e.dataTransfer.setData('text/plain', String(index));
        // Make the drag image slightly transparent
        if (e.currentTarget instanceof HTMLElement) {
            e.currentTarget.style.opacity = '0.5';
        }
    }, []);

    const handleDragEnd = useCallback((e: React.DragEvent) => {
        if (e.currentTarget instanceof HTMLElement) {
            e.currentTarget.style.opacity = '1';
        }
        setDragIndex(null);
        setDropIndex(null);
        dragCounterRef.current = 0;
    }, []);

    const handleDragEnter = useCallback((e: React.DragEvent, index: number, folderId: string) => {
        e.preventDefault();
        dragCounterRef.current++;
        if (e.dataTransfer.types.includes('application/x-gallerynet-media')) {
            setMediaDropTargetId(folderId);
        } else if (dragIndex !== null && dragIndex !== index) {
            setDropIndex(index);
        }
    }, [dragIndex]);

    const handleDragOver = useCallback((e: React.DragEvent) => {
        e.preventDefault();
        if (e.dataTransfer.types.includes('application/x-gallerynet-media')) {
            e.dataTransfer.dropEffect = 'copy';
        } else {
            e.dataTransfer.dropEffect = 'move';
        }
    }, []);

    const handleDragLeave = useCallback((_e: React.DragEvent, _index: number) => {
        dragCounterRef.current--;
        if (dragCounterRef.current === 0) {
            setDropIndex(null);
            setMediaDropTargetId(null);
        }
    }, []);

    const handleDrop = useCallback(async (e: React.DragEvent, toIndex: number, folderId: string) => {
        e.preventDefault();
        dragCounterRef.current = 0;
        const fromIndex = dragIndex;
        const mediaData = e.dataTransfer.getData('application/x-gallerynet-media');
        
        setDragIndex(null);
        setDropIndex(null);
        setMediaDropTargetId(null);

        if (mediaData) {
            // Media drop
            try {
                const ids = JSON.parse(mediaData);
                if (Array.isArray(ids) && ids.length > 0) {
                    const res = await apiFetch(`/api/folders/${folderId}/media`, {
                        method: 'POST',
                        headers: {'Content-Type': 'application/json'},
                        body: JSON.stringify(ids),
                    });
                    if (res.ok) {
                        onFoldersChanged();
                        setDropSuccessId(folderId);
                        setTimeout(() => setDropSuccessId(null), 2000);
                    }
                }
            } catch (err) {
                console.error('Failed to drop media:', err);
            }
            return;
        }

        if (fromIndex === null || fromIndex === toIndex) return;

        // Compute new order locally
        const reordered = [...folders];
        const [moved] = reordered.splice(fromIndex, 1);
        reordered.splice(toIndex, 0, moved);

        // Send new order to backend
        const folderIds = reordered.map(f => f.id);
        try {
            await apiFetch('/api/folders/reorder', {
                method: 'PUT',
                headers: {'Content-Type': 'application/json'},
                body: JSON.stringify(folderIds),
            });
            onFoldersChanged();
        } catch { /* ignore */ }
    }, [dragIndex, folders, onFoldersChanged]);


    const isActive = (path: string) => {
        if (path === '/') return location.pathname === '/';
        return location.pathname.startsWith(path);
    };

    const isFolderActive = (id: string) => location.pathname === `/folders/${id}`;

    return (
        <aside className={`
            fixed inset-y-0 left-0 z-50 w-72 bg-white border-r border-gray-200 flex flex-col flex-shrink-0
            transition-transform duration-200 ease-in-out
            md:relative md:z-auto md:translate-x-0
            ${mobileOpen ? 'translate-x-0' : '-translate-x-full'}
        `}>
            <div className="py-4 px-6 border-b border-gray-100 flex items-center justify-between">
                <div>
                    <h1 className="text-2xl font-bold text-transparent bg-clip-text bg-gradient-to-r from-blue-600 to-purple-600">
                        GalleryNet
                    </h1>
                </div>
                {/* Close button (mobile only) */}
                <button
                    onClick={onMobileClose}
                    className="p-1.5 rounded-lg hover:bg-gray-100 transition-colors md:hidden"
                    aria-label="Close menu"
                >
                    <svg className="w-5 h-5 text-gray-500" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor">
                        <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
                    </svg>
                </button>
            </div>

            <nav className={`flex-1 p-4 space-y-2 overflow-y-auto ${disabled ? 'pointer-events-none' : ''}`}>
                <Link
                    to="/"
                    onClick={onMobileClose}
                    className={`w-full flex items-center gap-3 px-4 py-3 rounded-lg text-sm font-medium transition-colors disabled:opacity-50 ${isActive('/') ? 'bg-blue-50 text-blue-700' : 'text-gray-600 hover:bg-gray-100'}`}
                >
                    <PhotoIcon/> Gallery
                </Link>
                <Link
                    to="/favorites"
                    onClick={onMobileClose}
                    className={`w-full flex items-center gap-3 px-4 py-3 rounded-lg text-sm font-medium transition-colors disabled:opacity-50 ${isActive('/favorites') ? 'bg-red-50 text-red-700' : 'text-gray-600 hover:bg-gray-100'}`}
                >
                    <HeartIcon/> Favorites
                </Link>
                <Link
                    to="/search"
                    onClick={onMobileClose}
                    className={`w-full flex items-center gap-3 px-4 py-3 rounded-lg text-sm font-medium transition-colors disabled:opacity-50 ${isActive('/search') ? 'bg-purple-50 text-purple-700' : 'text-gray-600 hover:bg-gray-100'}`}
                >
                    <SearchIcon/> Visual Search
                </Link>

                {/* Folders section */}
                <div className="pt-3 mt-3 border-t border-gray-100">
                    <p className="text-[11px] font-semibold text-gray-400 uppercase tracking-wider px-4 mb-2">Folders</p>
                    {folders.map((folder, index) => (
                        <div
                            key={folder.id}
                            draggable={renamingId !== folder.id}
                            onDragStart={(e) => handleDragStart(e, index)}
                            onDragEnd={handleDragEnd}
                            onDragEnter={(e) => handleDragEnter(e, index, folder.id)}
                            onDragOver={handleDragOver}
                            onDragLeave={(e) => handleDragLeave(e, index)}
                            onDrop={(e) => handleDrop(e, index, folder.id)}
                            className={`relative transition-all duration-200 ${
                                mediaDropTargetId === folder.id 
                                    ? 'bg-blue-50 scale-[1.02] ring-2 ring-blue-400 rounded-lg z-10' 
                                    : ''
                            } ${
                                dropIndex === index && dragIndex !== null && dragIndex !== index
                                    ? dragIndex < index
                                        ? 'after:absolute after:bottom-0 after:left-2 after:right-2 after:h-0.5 after:bg-blue-500 after:rounded-full'
                                        : 'before:absolute before:top-0 before:left-2 before:right-2 before:h-0.5 before:bg-blue-500 before:rounded-full'
                                    : ''
                            }`}
                        >

                            {renamingId === folder.id ? (
                                /* Inline rename input */
                                <div className="flex items-center gap-2 px-4 py-2">
                                    <svg className="w-4 h-4 flex-shrink-0 text-gray-400" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor">
                                        <path strokeLinecap="round" strokeLinejoin="round" d="M2.25 12.75V12A2.25 2.25 0 014.5 9.75h15A2.25 2.25 0 0121.75 12v.75m-8.69-6.44l-2.12-2.12a1.5 1.5 0 00-1.061-.44H4.5A2.25 2.25 0 002.25 6v12a2.25 2.25 0 002.25 2.25h15A2.25 2.25 0 0021.75 18V9a2.25 2.25 0 00-2.25-2.25h-5.379a1.5 1.5 0 01-1.06-.44z" />
                                    </svg>
                                    <input
                                        ref={renameInputRef}
                                        type="text"
                                        value={renameValue}
                                        onChange={(e) => setRenameValue(e.target.value)}
                                        onKeyDown={(e) => {
                                            if (e.key === 'Enter') commitRename();
                                            if (e.key === 'Escape') cancelRename();
                                        }}
                                        onBlur={commitRename}
                                        className="flex-1 min-w-0 text-sm px-1.5 py-0.5 rounded border border-blue-400 focus:outline-none focus:ring-1 focus:ring-blue-400"
                                    />
                                </div>
                            ) : (
                                 /* Normal folder row - now a Link */
                                <Link
                                    to={`/folders/${folder.id}`}
                                    onClick={onMobileClose}
                                    className={`flex items-center gap-2 px-4 py-2 rounded-lg text-sm transition-colors group cursor-pointer ${
                                        mediaDropTargetId === folder.id
                                            ? 'text-blue-700 bg-blue-50/50'
                                            : isFolderActive(folder.id)
                                                ? 'bg-amber-50 text-amber-700'
                                                : 'text-gray-600 hover:bg-gray-100'
                                    } ${dragIndex === index ? 'opacity-50' : ''}`}
                                    onDoubleClick={(e) => startRename(e, folder)}
                                >

                                    {/* Drag handle */}
                                    <svg 
                                        className="w-3 h-3 flex-shrink-0 text-gray-300 cursor-grab active:cursor-grabbing hover:text-gray-500" 
                                        viewBox="0 0 6 10" 
                                        fill="currentColor"
                                        onMouseDown={e => e.stopPropagation()} // Allow dragging only from handle? Or whole row?
                                        // The draggable is on the parent div, so we don't need to do anything special here unless we want to limit drag handle.
                                        // But the parent div has draggable.
                                    >
                                        <circle cx="1" cy="1" r="1"/><circle cx="5" cy="1" r="1"/>
                                        <circle cx="1" cy="5" r="1"/><circle cx="5" cy="5" r="1"/>
                                        <circle cx="1" cy="9" r="1"/><circle cx="5" cy="9" r="1"/>
                                    </svg>
                                    <svg className="w-4 h-4 flex-shrink-0" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor">
                                        <path strokeLinecap="round" strokeLinejoin="round" d="M2.25 12.75V12A2.25 2.25 0 014.5 9.75h15A2.25 2.25 0 0121.75 12v.75m-8.69-6.44l-2.12-2.12a1.5 1.5 0 00-1.061-.44H4.5A2.25 2.25 0 002.25 6v12a2.25 2.25 0 002.25 2.25h15A2.25 2.25 0 0021.75 18V9a2.25 2.25 0 00-2.25-2.25h-5.379a1.5 1.5 0 01-1.06-.44z" />
                                    </svg>
                                    <span className="truncate flex-1 text-left">{folder.name}</span>
                                    {dropSuccessId === folder.id ? (
                                        <svg className="w-4 h-4 text-green-500 animate-in zoom-in duration-300" fill="none" viewBox="0 0 24 24" strokeWidth={3} stroke="currentColor">
                                            <path strokeLinecap="round" strokeLinejoin="round" d="M4.5 12.75l6 6 9-13.5" />
                                        </svg>
                                    ) : (
                                        <span className="text-[10px] text-gray-400 flex-shrink-0">{folder.item_count}</span>
                                    )}
                                    <button

                                        onClick={(e) => handleDeleteFolder(e, folder)}
                                        className="opacity-0 group-hover:opacity-100 p-0.5 rounded hover:bg-red-100 hover:text-red-600 transition-all flex-shrink-0"
                                        title="Delete folder"
                                    >

                                        <svg className="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor">
                                            <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
                                        </svg>
                                    </button>
                                </Link>

                            )}
                        </div>
                    ))}

                    {/* New folder input */}
                    <div className="flex items-center gap-1 px-4 mt-1">
                        <input
                            type="text"
                            value={newFolderName}
                            onChange={(e) => setNewFolderName(e.target.value)}
                            onKeyDown={(e) => { if (e.key === 'Enter') handleCreateFolder(); }}
                            placeholder="New folder..."
                            className="flex-1 min-w-0 text-sm px-2 py-1.5 rounded-md border border-gray-200 focus:border-blue-400 focus:outline-none placeholder:text-gray-300"
                            disabled={isCreating}
                        />
                        <button
                            onClick={handleCreateFolder}
                            disabled={isCreating || !newFolderName.trim()}
                            className="p-1.5 rounded-md text-gray-400 hover:text-blue-600 hover:bg-blue-50 disabled:opacity-30 transition-colors flex-shrink-0"
                            title="Create folder"
                        >
                            <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor">
                                <path strokeLinecap="round" strokeLinejoin="round" d="M12 4.5v15m7.5-7.5h-15" />
                            </svg>
                        </button>
                    </div>
                </div>
            </nav>

            <LibraryInfo refreshKey={refreshKey} />

            <ConfirmDialog
                isOpen={!!folderToDelete}
                title="Delete Folder?"
                message={`Are you sure you want to delete the folder "${folderToDelete?.name}"? The media items inside will not be deleted.`}
                confirmLabel="Delete"
                cancelLabel="Cancel"
                isDestructive={true}
                onConfirm={confirmDeleteFolder}
                onCancel={() => setFolderToDelete(null)}
            />
        </aside>

    );
}
