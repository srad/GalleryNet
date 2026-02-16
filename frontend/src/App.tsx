import { useState, useCallback, useEffect } from 'react';
import { useLocation, useNavigate, matchPath } from 'react-router-dom';
import type { MediaFilter, Folder } from './types';
import Sidebar from './components/Sidebar';
import GalleryView from './components/GalleryView';
import SearchView from './components/SearchView';
import LoginView from './components/LoginView';
import { apiFetch } from './auth';

type AuthState = 'loading' | 'authenticated' | 'unauthenticated';

export default function App() {
    const location = useLocation();
    const navigate = useNavigate();
    
    const [authState, setAuthState] = useState<AuthState>('loading');
    const [authRequired, setAuthRequired] = useState(false);
    const [folders, setFolders] = useState<Folder[]>([]);
    const [foldersLoaded, setFoldersLoaded] = useState(false);
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
            if (res.ok) {
                setFolders(await res.json());
            }
        } catch { /* ignore */ }
        finally {
            setFoldersLoaded(true);
        }
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
        navigate('/');
    }, [navigate]);

    const handleFilterChange = useCallback((filter: MediaFilter) => {
        setMediaFilter(filter);
        localStorage.setItem('galleryFilter', filter);
    }, []);

    const handleUploadComplete = useCallback(() => {
        setRefreshKey(k => k + 1);
    }, []);

    const handleFindSimilar = useCallback((mediaId: string) => {
        navigate(`/search?source=${mediaId}`);
    }, [navigate]);

    // Routing Logic
    const isGallery = location.pathname === '/';
    const isFavorites = location.pathname === '/favorites';
    const isSearch = location.pathname.startsWith('/search');
    const folderMatch = matchPath('/folders/:folderId', location.pathname);
    const isFolder = !!folderMatch;
    const activeFolderId = folderMatch?.params.folderId;
    const activeFolder = folders.find(f => f.id === activeFolderId) || null;

    // Loading state
    if (authState === 'loading' || (authState === 'authenticated' && !foldersLoaded)) {
        return (
            <div className="flex items-center justify-center min-h-screen bg-gray-50">
                <div className="flex flex-col items-center gap-3">
                    <svg className="w-8 h-8 animate-spin text-blue-600" viewBox="0 0 24 24" fill="none">
                        <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
                        <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
                    </svg>
                    <div className="text-gray-400 text-sm font-medium">Loading GalleryNet...</div>
                </div>
            </div>
        );
    }

    // Login screen
    if (authState === 'unauthenticated') {
        return <LoginView onLogin={handleLogin} />;
    }

    return (
        <div className="flex flex-col h-screen w-full bg-gray-50 font-sans text-gray-800 overflow-hidden">
            {/* Mobile header bar */}
            <div className="flex items-center gap-3 px-4 py-3 bg-white border-b border-gray-200 md:hidden flex-shrink-0">
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

            {/* Main content area */}
            <div className="flex flex-1 overflow-hidden">
                {/* Sidebar backdrop (mobile only) */}
                {sidebarOpen && (
                    <div
                        className="fixed inset-0 z-40 bg-black/40 backdrop-blur-sm md:hidden"
                        onClick={() => setSidebarOpen(false)}
                    />
                )}

                <Sidebar
                    refreshKey={refreshKey}
                    onLogout={authRequired ? handleLogout : undefined}
                    folders={folders}
                    onFoldersChanged={fetchFolders}
                    disabled={isBusy}
                    mobileOpen={sidebarOpen}
                    onMobileClose={() => setSidebarOpen(false)}
                />

                <main className="flex-1 h-full overflow-y-auto">
                    <div className={isGallery ? '' : 'hidden'}>
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

                    <div className={isFavorites ? '' : 'hidden'}>
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

                    <div className={isSearch ? 'p-4 md:p-8' : 'hidden'}>
                        <SearchView />
                    </div>

                    {isFolder && activeFolderId && (
                        <GalleryView
                            key={`folder-${activeFolderId}`}
                            filter={mediaFilter}
                            onFilterChange={handleFilterChange}
                            refreshKey={refreshKey}
                            folderId={activeFolderId}
                            folderName={activeFolder?.name}
                            onBackToGallery={() => navigate('/')}
                            folders={folders}
                            onFoldersChanged={fetchFolders}
                            onUploadComplete={handleUploadComplete}
                            onBusyChange={setIsBusy}
                            onFindSimilar={handleFindSimilar}
                        />
                    )}
                </main>
            </div>
        </div>
    );
}

