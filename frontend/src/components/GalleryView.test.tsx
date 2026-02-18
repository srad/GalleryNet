import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor, act } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import GalleryView from './GalleryView';
import { apiClient } from '../api';
import { fireMediaUpdate } from '../events';

// Mock API
vi.mock('../api', () => ({
    apiClient: {
        getMedia: vi.fn(),
        getGroups: vi.fn(),
        checkAuth: vi.fn(),
        getMediaItem: vi.fn(),
    }
}));

// Mock Icons to avoid SVG issues or noise
vi.mock('./Icons', () => ({
    PhotoIcon: () => <div data-testid="photo-icon" />,
    UploadIcon: () => <div data-testid="upload-icon" />,
    PlusIcon: () => <div data-testid="plus-icon" />,
    HeartIcon: ({ solid }: { solid: boolean }) => <div data-testid="heart-icon" data-solid={solid} />,
    TagIcon: () => <div data-testid="tag-icon" />,
    LogoutIcon: () => <div data-testid="logout-icon" />,
}));

// Mock MediaCard to simplify
vi.mock('./MediaCard', () => ({
    default: ({ item }: { item: any }) => (
        <div data-testid="media-card">
            <span data-testid="media-filename">{item.filename}</span>
            <span data-testid="media-phash">{item.phash}</span>
            <span data-testid="media-width">{item.width}</span>
            <span data-testid="media-height">{item.height}</span>
        </div>
    )
}));

// Mock other components
vi.mock('./TagFilter', () => ({ default: () => <div /> }));
vi.mock('./MediaModal', () => ({ default: () => <div /> }));
vi.mock('./ConfirmDialog', () => ({ default: () => <div /> }));
vi.mock('./AlertDialog', () => ({ default: () => <div /> }));
vi.mock('./LoadingIndicator', () => ({ default: () => <div /> }));

describe('GalleryView', () => {
    const mockMediaItems = [
        {
            id: 'id-1',
            filename: 'test1.jpg',
            original_filename: 'test1.jpg',
            media_type: 'image',
            phash: 'initial-phash',
            uploaded_at: '2023-01-01T00:00:00Z',
            original_date: '2023-01-01T00:00:00Z',
            width: 100,
            height: 100,
            size_bytes: 1024,
            is_favorite: false,
            tags: []
        },
        {
            id: 'id-2',
            filename: 'test2.jpg',
            original_filename: 'test2.jpg',
            media_type: 'image',
            phash: 'phash-2',
            uploaded_at: '2023-01-02T00:00:00Z',
            original_date: '2023-01-02T00:00:00Z',
            width: 200,
            height: 200,
            size_bytes: 2048,
            is_favorite: false,
            tags: []
        }
    ];

    beforeEach(() => {
        vi.clearAllMocks();
        (apiClient.getMedia as any).mockResolvedValue(mockMediaItems);
        (apiClient.getGroups as any).mockResolvedValue([]);
        
        // Mock IntersectionObserver
        const MockObserver = vi.fn(function() {
            return {
                observe: vi.fn(),
                unobserve: vi.fn(),
                disconnect: vi.fn(),
            };
        });
        window.IntersectionObserver = MockObserver as any;
    });

    it('updates item when thumbnail fix event is received', async () => {
        render(
            <MemoryRouter>
                <GalleryView
                    filter="all"
                    onFilterChange={vi.fn()}
                    refreshKey={0}
                    folders={[]}
                    onFoldersChanged={vi.fn()}
                />
            </MemoryRouter>
        );

        // Wait for initial load
        await waitFor(() => {
            expect(screen.getAllByTestId('media-card')).toHaveLength(2);
        });

        // Verify initial state
        const firstCard = screen.getAllByTestId('media-card')[0];
        expect(firstCard.textContent).toContain('initial-phash');
        expect(firstCard.textContent).toContain('100'); // width

        // Trigger update (simulate fix thumbnail result)
        // The fix updates phash, width, height, etc.
        const updatedItem = {
            ...mockMediaItems[0],
            phash: 'fixed-phash',
            width: 1920,
            height: 1080
        };

        // Fire the event that useWebSocket would fire
        act(() => {
            fireMediaUpdate('id-1', updatedItem);
        });

        // Verify update is reflected
        await waitFor(() => {
            const cards = screen.getAllByTestId('media-card');
            // Sorted by date desc: id-2 (Jan 2) then id-1 (Jan 1)
            const updatedCard = cards[1]; 
            expect(updatedCard.textContent).toContain('fixed-phash');
            expect(updatedCard.textContent).toContain('1920');
            expect(updatedCard.textContent).toContain('1080');
        });
    });
});
