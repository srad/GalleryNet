import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor, act, fireEvent } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import GalleryView from './GalleryView';
import { apiClient } from '../api';

// --- Mocks ---

// Mock API
vi.mock('../api', () => ({
    apiClient: {
        getMedia: vi.fn(),
        getGroups: vi.fn(),
        checkAuth: vi.fn(),
        getMediaItem: vi.fn(),
        deleteMediaBatch: vi.fn(),
        removeMediaFromFolder: vi.fn(),
    }
}));

// Mock ConfirmDialog to be interactive
vi.mock('./ConfirmDialog', () => ({
    default: ({ isOpen, title, onConfirm, onCancel }: any) => {
        if (!isOpen) return null;
        return (
            <div data-testid="confirm-dialog">
                <h1>{title}</h1>
                <button onClick={onConfirm}>Confirm</button>
                <button onClick={onCancel}>Cancel</button>
            </div>
        );
    }
}));

// Mock MediaCard
vi.mock('./MediaCard', () => ({
    default: ({ item, focused }: { item: any, focused?: boolean }) => (
        <div 
            data-testid={`media-card-${item.id}`} 
            data-id={item.id} 
            className={focused ? 'focused' : ''}
            // Mock getBoundingClientRect for navigation logic
            style={{ width: 100, height: 100 }}
        >
            {item.filename}
        </div>
    )
}));

// Mock other UI components to reduce noise
vi.mock('./Icons', () => ({
    PhotoIcon: () => null,
    UploadIcon: () => null,
    PlusIcon: () => null,
    HeartIcon: () => null,
    TagIcon: () => null,
    LogoutIcon: () => null,
}));
vi.mock('./TagFilter', () => ({ default: () => null }));
vi.mock('./MediaModal', () => ({ default: () => null }));
vi.mock('./LoadingIndicator', () => ({ default: () => null }));

describe('GalleryView Interactions', () => {
    const mockMedia = [
        { id: '1', filename: 'img1.jpg', original_date: '2023-01-01', size_bytes: 100 },
        { id: '2', filename: 'img2.jpg', original_date: '2023-01-02', size_bytes: 200 },
        { id: '3', filename: 'img3.jpg', original_date: '2023-01-03', size_bytes: 300 },
    ];

    beforeEach(() => {
        vi.clearAllMocks();
        (apiClient.getMedia as any).mockResolvedValue(mockMedia);
        (apiClient.getGroups as any).mockResolvedValue([]);
        
        // Mock scrollIntoView
        window.HTMLElement.prototype.scrollIntoView = vi.fn();
        window.HTMLElement.prototype.focus = vi.fn();
        
        // Mock getBoundingClientRect for basic grid layout simulation
        // Layout: 1 2
        //         3
        window.HTMLElement.prototype.getBoundingClientRect = function() {
            const id = this.getAttribute('data-id');
            if (id === '1') return { top: 0, left: 0, width: 100, height: 100, bottom: 100, right: 100 } as DOMRect;
            if (id === '2') return { top: 0, left: 100, width: 100, height: 100, bottom: 100, right: 200 } as DOMRect;
            if (id === '3') return { top: 100, left: 0, width: 100, height: 100, bottom: 200, right: 100 } as DOMRect;
            return { top: 0, left: 0, width: 0, height: 0, bottom: 0, right: 0 } as DOMRect;
        };

        // Mock Observer
        window.IntersectionObserver = class {
            observe = vi.fn();
            unobserve = vi.fn();
            disconnect = vi.fn();
        } as any;
    });

    it('supports grid navigation (Right/Left/Down)', async () => {
        render(
            <MemoryRouter>
                <GalleryView filter="all" onFilterChange={vi.fn()} refreshKey={0} folders={[]} onFoldersChanged={vi.fn()} />
            </MemoryRouter>
        );

        await waitFor(() => expect(screen.getAllByTestId(/^media-card-/)).toHaveLength(3));

        // 1. Initial: None focused
        expect(document.querySelector('.focused')).toBeNull();

        // 2. ArrowRight -> Focus '1'
        act(() => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight' })));
        expect(screen.getByTestId('media-card-1').className).toContain('focused');

        // 3. ArrowRight -> Focus '2'
        act(() => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight' })));
        expect(screen.getByTestId('media-card-2').className).toContain('focused');

        // 4. ArrowDown -> Focus '3' (from '2', '3' is visually below '1' but it's the next row)
        // With our layout logic: 
        // 1 (0,0)  2 (100,0)
        // 3 (0,100)
        // From 2, visual candidates below are [3].
        // Next row top is 100. '3' is at 100.
        // It should match.
        act(() => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown' })));
        expect(screen.getByTestId('media-card-3').className).toContain('focused');

        // 5. ArrowUp -> Focus '1' (visually above '3')
        act(() => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowUp' })));
        expect(screen.getByTestId('media-card-1').className).toContain('focused');
    });

    it('triggers delete confirmation with Delete key', async () => {
        render(
            <MemoryRouter>
                <GalleryView filter="all" onFilterChange={vi.fn()} refreshKey={0} folders={[]} onFoldersChanged={vi.fn()} />
            </MemoryRouter>
        );

        await waitFor(() => expect(screen.getAllByTestId(/^media-card-/)).toHaveLength(3));

        // Focus item '2'
        act(() => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight' }))); // 1
        act(() => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight' }))); // 2
        expect(screen.getByTestId('media-card-2').className).toContain('focused');

        // Press Delete
        act(() => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Delete' })));

        // Confirm Dialog should appear
        expect(screen.getByTestId('confirm-dialog')).not.toBeNull();
        expect(screen.getByText('Delete Media?')).not.toBeNull();

        // Confirm Delete
        await act(async () => {
            fireEvent.click(screen.getByText('Confirm'));
        });

        // API should be called with ['2']
        expect(apiClient.deleteMediaBatch).toHaveBeenCalledWith(['2']);
    });

    it('cancels delete confirmation without deleting', async () => {
        render(
            <MemoryRouter>
                <GalleryView filter="all" onFilterChange={vi.fn()} refreshKey={0} folders={[]} onFoldersChanged={vi.fn()} />
            </MemoryRouter>
        );

        await waitFor(() => expect(screen.getAllByTestId(/^media-card-/)).toHaveLength(3));

        // Focus item '1'
        act(() => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight' })));
        
        // Press Backspace
        act(() => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Backspace' })));
        expect(screen.getByTestId('confirm-dialog')).not.toBeNull();

        // Click Cancel
        act(() => {
            fireEvent.click(screen.getByText('Cancel'));
        });

        // Dialog gone, API not called
        expect(screen.queryByTestId('confirm-dialog')).toBeNull();
        expect(apiClient.deleteMediaBatch).not.toHaveBeenCalled();
    });

    it('handles Remove from Folder when in folder view', async () => {
        render(
            <MemoryRouter>
                <GalleryView 
                    filter="all" 
                    onFilterChange={vi.fn()} 
                    refreshKey={0} 
                    folders={[{ id: 'f1', name: 'MyFolder', item_count: 3, created_at: '', sort_order: 0 }]} 
                    folderId="f1" // Active folder
                    folderName="MyFolder"
                    onFoldersChanged={vi.fn()} 
                />
            </MemoryRouter>
        );

        await waitFor(() => expect(screen.getAllByTestId(/^media-card-/)).toHaveLength(3));

        // Focus item '3'
        // Fire events sequentially to allow state updates (re-binding of listener)
        act(() => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight' }))); // Focus 1
        await waitFor(() => expect(screen.getByTestId('media-card-1').className).toContain('focused'));
        
        act(() => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight' }))); // Focus 2
        await waitFor(() => expect(screen.getByTestId('media-card-2').className).toContain('focused'));

        act(() => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight' }))); // Focus 3
        await waitFor(() => expect(screen.getByTestId('media-card-3').className).toContain('focused'));
        
        expect(screen.getByTestId('media-card-3').className).toContain('focused');

        // Press Delete
        act(() => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Delete' })));

        // Dialog should be "Remove from Folder?"
        expect(screen.getByTestId('confirm-dialog')).not.toBeNull();
        expect(screen.getByText('Remove from Folder?')).not.toBeNull();

        // Confirm
        await act(async () => {
            fireEvent.click(screen.getByText('Confirm'));
        });

        // removeMediaFromFolder API called
        expect(apiClient.removeMediaFromFolder).toHaveBeenCalledWith('f1', ['3']);
        expect(apiClient.deleteMediaBatch).not.toHaveBeenCalled();
    });
});
