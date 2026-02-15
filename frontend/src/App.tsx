import { useState, useCallback, useEffect } from 'react';
import type { MediaFilter, Folder } from './types';
import Sidebar, { type Tab } from './components/Sidebar';
import GalleryView from './components/GalleryView';
import SearchView from './components/SearchView';
import LoginView from './components/LoginView';
import { apiFetch } from './auth';

type AuthState = 'loading' | 'authenticated' | 'unauthenticated';

export default function App() {
    const [authState, setAuthState] = useState<AuthState>('loading');
    const [authRequired, setAuthRequired] = useState(false);
    const [activeTab, setActiveTab] = useState<Tab>('gallery');
    const [activeFolder, setActiveFolder] = useState<Folder | null>(null);
    const [folders, setFolders] = useState<Folder[]>([]);
    const [sidebarOpen, setSidebarOpen] = useState(false);

    // Listen for 401 events from any API call
    useEffect(() => {
        const handler = () => setAuthState('unauthenticated');
        window.addEventListener('gallerynet-unauthorized', handler);
        return () => window.removeEventListener('gallerynet-unauthorized', handler);
    }, []);
    const [mediaFilter, setMediaFilter] = useState<MediaFilter>(() => {
        const saved = localStorage.getItem('galleryFilter');
        return saved === 'image' || saved === 'video' ? saved : 'all';
    });
    const [refreshKey, setRefreshKey] = useState(0);
    const [activeSearchMediaId, setActiveSearchMediaId] = useState<string | null>(null);
    const [isBusy, setIsBusy] = useState(false);

    // Check authentication status on mount
    useEffect(() => {
        fetch('/api/auth-check')
            .then(res => {
                if (res.ok) {
                    return res.json().then(data => {
                        setAuthRequired(data.required ?? false);
                        if (data.authenticated) {
                            setAuthState('authenticated');
                        } else {
                            setAuthState('unauthenticated');
                        }
                    });
                } else {
                    setAuthRequired(true);
                    setAuthState('unauthenticated');
                }
            })
            .catch(() => {
                setAuthState('unauthenticated');
            });
    }, []);

    // Fetch folders when authenticated
    const fetchFolders = useCallback(async () => {
        try {
            const res = await apiFetch('/api/folders');
            if (res.ok) setFolders(await res.json());
        } catch { /* ignore */ }
    }, []);

    useEffect(() => {
        if (authState === 'authenticated') fetchFolders();
    }, [authState, fetchFolders]);

    // Re-fetch folders when refreshKey changes (uploads may affect counts)
    useEffect(() => {
        if (authState === 'authenticated') fetchFolders();
    }, [refreshKey, authState, fetchFolders]);

    const handleLogin = useCallback(() => {
        setAuthState('authenticated');
    }, []);

    const handleLogout = useCallback(async () => {
        try {
            await fetch('/api/logout', { method: 'POST' });
        } catch { /* ignore */ }
        setAuthState('unauthenticated');
    }, []);

    const handleFilterChange = useCallback((filter: MediaFilter) => {
        setMediaFilter(filter);
        localStorage.setItem('galleryFilter', filter);
    }, []);

    const handleUploadComplete = useCallback(() => {
        setRefreshKey(k => k + 1);
    }, []);

    const handleSelectFolder = useCallback((folder: Folder) => {
        setActiveFolder(folder);
        setActiveTab('folder');
    }, []);

    const handleBackToGallery = useCallback(() => {
        setActiveFolder(null);
        setActiveTab('gallery');
    }, []);

    const handleTabChange = useCallback((tab: Tab) => {
        if (tab !== 'folder') {
            setActiveFolder(null);
        }
        if (tab !== 'search') {
            setActiveSearchMediaId(null);
        }
        setActiveTab(tab);
        setSidebarOpen(false); // Close sidebar on mobile after navigation
    }, []);

    const handleSelectFolderWrapped = useCallback((folder: Folder) => {
        handleSelectFolder(folder);
        setSidebarOpen(false); // Close sidebar on mobile after navigation
    }, [handleSelectFolder]);

    const handleFindSimilar = useCallback((mediaId: string) => {
        setActiveSearchMediaId(mediaId);
        setActiveTab('search');
    }, []);

    // Warn before leaving/reloading while uploads are active
    // Note: This is now handled locally in GalleryView if needed, or we can add a global context later if strictly required.
    // For now, removing the global effect since upload state is local to GalleryView.

    // Loading state
    if (authState === 'loading') {
        return (
            <div className="flex items-center justify-center min-h-screen bg-gray-50">
                <div className="text-gray-400 text-sm">Loading...</div>
            </div>
        );
    }

    // Login screen
    if (authState === 'unauthenticated') {
        return <LoginView onLogin={handleLogin} />;
    }

    return (
        <div className="flex h-screen w-full bg-gray-50 font-sans text-gray-800 overflow-hidden">
            {/* Mobile header bar */}
            <div className="fixed top-0 left-0 right-0 z-40 flex items-center gap-3 px-4 py-3 bg-white border-b border-gray-200 md:hidden">
                <button
                    onClick={() => setSidebarOpen(true)}
                    className="p-1.5 rounded-lg hover:bg-gray-100 transition-colors"
                    aria-label="Open menu"
                >
                    <svg className="w-6 h-6 text-gray-700" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor">
                        <path strokeLinecap="round" strokeLinejoin="round" d="M3.75 6.75h16.5M3.75 12h16.5m-16.5 5.25h16.5" />
                    </svg>
                </button>
                <h1 className="text-lg font-bold text-transparent bg-clip-text bg-gradient-to-r from-blue-600 to-purple-600">
                    GalleryNet
                </h1>
            </div>

            {/* Sidebar backdrop (mobile only) */}
            {sidebarOpen && (
                <div
                    className="fixed inset-0 z-40 bg-black/40 backdrop-blur-sm md:hidden"
                    onClick={() => setSidebarOpen(false)}
                />
            )}

            <Sidebar
                activeTab={activeTab}
                onTabChange={handleTabChange}
                refreshKey={refreshKey}
                onLogout={authRequired ? handleLogout : undefined}
                folders={folders}
                activeFolder={activeFolder}
                onSelectFolder={handleSelectFolderWrapped}
                onFoldersChanged={fetchFolders}
                disabled={isBusy}
                mobileOpen={sidebarOpen}
                onMobileClose={() => setSidebarOpen(false)}
            />

            <main className="flex-1 h-full overflow-y-auto p-4 pt-16 md:p-8 md:pt-8">
                <div className={activeTab === 'gallery' ? '' : 'hidden'}>
                    <GalleryView
                        filter={mediaFilter}
                        onFilterChange={handleFilterChange}
                        refreshKey={refreshKey}
                        folders={folders}
                        onFoldersChanged={fetchFolders}
                        onUploadComplete={handleUploadComplete}
                        onBusyChange={setIsBusy}
                        onFindSimilar={handleFindSimilar}
                    />
                </div>

                <div className={activeTab === 'favorites' ? '' : 'hidden'}>
                    <GalleryView
                        filter={mediaFilter}
                        onFilterChange={handleFilterChange}
                        refreshKey={refreshKey}
                        folders={folders}
                        onFoldersChanged={fetchFolders}
                        onUploadComplete={handleUploadComplete}
                        onBusyChange={setIsBusy}
                        onFindSimilar={handleFindSimilar}
                        favoritesOnly={true}
                    />
                </div>

                <div className={activeTab === 'search' ? '' : 'hidden'}>
                    <SearchView initialMediaId={activeSearchMediaId} />
                </div>

                {activeTab === 'folder' && activeFolder && (
                    <GalleryView
                        key={`folder-${activeFolder.id}`}
                        filter={mediaFilter}
                        onFilterChange={handleFilterChange}
                        refreshKey={refreshKey}
                        folderId={activeFolder.id}
                        folderName={activeFolder.name}
                        onBackToGallery={handleBackToGallery}
                        folders={folders}
                        onFoldersChanged={fetchFolders}
                        onUploadComplete={handleUploadComplete}
                        onBusyChange={setIsBusy}
                        onFindSimilar={handleFindSimilar}
                    />
                )}
            </main>
        </div>
    );
}
