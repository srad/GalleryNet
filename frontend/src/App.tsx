import { useState, useCallback, useEffect } from 'react';
import { useLocation, useNavigate, matchPath } from 'react-router-dom';
import type { MediaFilter, Folder } from './types';
import Sidebar from './components/Sidebar';
import GalleryView from './components/GalleryView';
import SearchView from './components/SearchView';
import LoginView from './components/LoginView';
import LoadingIndicator from './components/LoadingIndicator';
import { apiClient } from './api';
import { useWebSocket } from './useWebSocket';


type AuthState = 'loading' | 'authenticated' | 'unauthenticated';

export default function App() {
    const location = useLocation();
    const navigate = useNavigate();

    const [authState, setAuthState] = useState<AuthState>('loading');
    const [authRequired, setAuthRequired] = useState(false);
    const [folders, setFolders] = useState<Folder[]>([]);
    const [appReady, setAppReady] = useState(false);
    const [isInitialLoading, setIsInitialLoading] = useState(true);
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
    const [isFixingThumbnails, setIsFixingThumbnails] = useState(false);

    // Initial load: Check auth and fetch folders
    useEffect(() => {
        const init = async () => {
            setIsInitialLoading(true);
            try {
                const data = await apiClient.checkAuth();
                setAuthRequired(data.required ?? false);
                if (data.authenticated) {
                    setAuthState('authenticated');
                    // Fetch folders immediately
                    const foldersData = await apiClient.getFolders();
                    setFolders(foldersData);
                } else {
                    setAuthState('unauthenticated');
                }
            } catch (e) {
                console.error('Init failed', e);
                // If auth check fails, assume we need to authenticate (or backend is down)
                setAuthRequired(true);
                setAuthState('unauthenticated');
            } finally {
                setIsInitialLoading(false);
                setAppReady(true);
            }
        };
        init();
    }, []);

    // Re-fetch folders when refreshKey changes (uploads may affect counts)
    const fetchFolders = useCallback(async () => {
        try {
            const foldersData = await apiClient.getFolders();
            setFolders(foldersData);
        } catch (e) {
            console.error('Failed to fetch folders:', e);
        }
    }, []);

    useEffect(() => {
        if (authState === 'authenticated' && refreshKey > 0) fetchFolders();
    }, [refreshKey, authState, fetchFolders]);

    const handleLogin = useCallback(async () => {
        setIsInitialLoading(true);
        setAuthState('authenticated');
        try {
            const foldersData = await apiClient.getFolders();
            setFolders(foldersData);
        } catch (e) {
            console.error('Failed to fetch folders on login:', e);
        }
        setIsInitialLoading(false);
    }, []);

    const handleLogout = useCallback(async () => {
        try {
            await apiClient.logout();
        } catch (e) {
            console.error('Logout failed:', e);
        }
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

    // Initialize WebSocket for real-time updates
    useWebSocket(
        useCallback(() => {
            if (authState === 'authenticated') fetchFolders();
        }, [authState, fetchFolders]),
        handleUploadComplete,
        useCallback((fixing: boolean) => {
            setIsFixingThumbnails(fixing);
        }, []),
        authState === 'authenticated'
    );

    // Routing Logic

    const isGallery = location.pathname === '/';
    const isFavorites = location.pathname === '/favorites';
    const isSearch = location.pathname.startsWith('/search');
    const folderMatch = matchPath('/folders/:folderId', location.pathname);
    const isFolder = !!folderMatch;
    const activeFolderId = folderMatch?.params.folderId;
    const activeFolder = folders.find(f => f.id === activeFolderId) || null;

    // Loading state: Hide everything until we know the auth status AND have folders if authenticated
    if (isInitialLoading || !appReady) {
        return (
            <div className="flex items-center justify-center min-h-screen bg-gray-50 dark:bg-gray-900">
                <LoadingIndicator
                    label="Loading GalleryNet..."
                    variant="centered"
                    size="lg"
                />
            </div>
        );
    }

    // Login screen - only show after initial load finishes and we are definitely unauthenticated
    if (authState === 'unauthenticated') {
        return <LoginView onLogin={handleLogin} />;
    }

    return (
        <div className="flex flex-col h-screen w-full bg-gray-50 dark:bg-gray-900 font-sans text-gray-800 dark:text-gray-100 overflow-hidden">
            {/* Mobile header bar */}
            <div className="flex items-center gap-3 px-4 py-3 bg-white dark:bg-gray-900 border-b border-gray-200 dark:border-gray-700 md:hidden flex-shrink-0">
                <button
                    onClick={() => setSidebarOpen(true)}
                    className="p-1.5 rounded-lg hover:bg-gray-100 dark:hover:bg-gray-800 transition-colors"
                    aria-label="Open menu"
                >
                    <svg className="w-6 h-6 text-gray-700 dark:text-gray-200" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor">
                        <path strokeLinecap="round" strokeLinejoin="round" d="M3.75 6.75h16.5M3.75 12h16.5m-16.5 5.25h16.5" />
                    </svg>
                </button>
                <h1 className="text-lg font-bold text-transparent bg-clip-text bg-gradient-to-r from-blue-600 to-purple-600">
                    GalleryNet
                </h1>
            </div>

            {/* Main content area */}
            <div className="flex flex-1 overflow-hidden relative">
                {/* Sidebar backdrop (mobile only) */}
                {sidebarOpen && (
                    <div
                        className="fixed inset-0 z-40 bg-black/40 backdrop-blur-sm md:hidden"
                        onClick={() => setSidebarOpen(false)}
                    />
                )}

                <Sidebar
                    refreshKey={refreshKey}
                    folders={folders}
                    onFoldersChanged={fetchFolders}
                    disabled={isBusy}
                    mobileOpen={sidebarOpen}
                    onMobileClose={() => setSidebarOpen(false)}
                />

                <main className="flex-1 h-full overflow-y-auto">
                    <div className={isGallery ? '' : 'hidden'}>
                        <GalleryView
                            isActive={isGallery}
                            filter={mediaFilter}
                            onFilterChange={handleFilterChange}
                            refreshKey={refreshKey}
                            folders={folders}
                            onFoldersChanged={fetchFolders}
                            onUploadComplete={handleUploadComplete}
                            onBusyChange={setIsBusy}
                            onFindSimilar={handleFindSimilar}
                            onLogout={authRequired ? handleLogout : undefined}
                        />
                    </div>

                    <div className={isFavorites ? '' : 'hidden'}>
                        <GalleryView
                            isActive={isFavorites}
                            filter={mediaFilter}
                            onFilterChange={handleFilterChange}
                            refreshKey={refreshKey}
                            folders={folders}
                            onFoldersChanged={fetchFolders}
                            onUploadComplete={handleUploadComplete}
                            onBusyChange={setIsBusy}
                            onFindSimilar={handleFindSimilar}
                            favoritesOnly={true}
                            onLogout={authRequired ? handleLogout : undefined}
                        />
                    </div>

                    <div className={isSearch ? 'p-4 md:p-8' : 'hidden'}>
                        <SearchView
                            isActive={isSearch}
                            folders={folders}
                            refreshKey={refreshKey}
                            onFoldersChanged={fetchFolders}
                            onLogout={authRequired ? handleLogout : undefined}
                        />
                    </div>

                    {isFolder && activeFolderId && (
                        <GalleryView
                            isActive={true}
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
                            onLogout={authRequired ? handleLogout : undefined}
                        />
                    )}
                </main>

                {isFixingThumbnails && (
                    <div className="absolute bottom-6 right-6 bg-white dark:bg-gray-800 px-4 py-3 rounded-xl shadow-xl border border-gray-200 dark:border-gray-700 flex items-center gap-3 z-50 animate-in fade-in slide-in-from-bottom-2 duration-300">
                        <LoadingIndicator size="sm" />
                        <span className="text-sm font-medium text-gray-700 dark:text-gray-200">
                            Fixing thumbnails...
                        </span>
                    </div>
                )}
            </div>
        </div>
    );
}
