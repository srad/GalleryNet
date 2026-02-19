import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor, act } from '@testing-library/react';
import { MemoryRouter, useLocation } from 'react-router-dom';
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
    default: ({ item, focused }: { item: any, focused?: boolean }) => (
        <div 
            data-testid={`media-card-${item.id}`} 
            data-filename={item.filename} 
            data-id={item.id} 
            className={focused ? 'focused' : ''}
        >
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
        },
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
        }
    ];

    beforeEach(() => {
        vi.clearAllMocks();
        (apiClient.getMedia as any).mockResolvedValue(mockMediaItems);
        (apiClient.getGroups as any).mockResolvedValue([]);
        
        // Mock DOM methods
        window.HTMLElement.prototype.scrollIntoView = vi.fn();
        window.HTMLElement.prototype.focus = vi.fn();
        window.HTMLElement.prototype.getBoundingClientRect = vi.fn().mockReturnValue({
            width: 100,
            height: 100,
            top: 0,
            left: 0,
            bottom: 100,
            right: 100,
        });

        // Mock IntersectionObserver
        const MockIntersectionObserver = vi.fn(function() {
            return {
                observe: vi.fn(),
                unobserve: vi.fn(),
                disconnect: vi.fn(),
            };
        });
        window.IntersectionObserver = MockIntersectionObserver as any;

        // Mock ResizeObserver
        const MockResizeObserver = vi.fn(function() {
            return {
                observe: vi.fn(),
                unobserve: vi.fn(),
                disconnect: vi.fn(),
            };
        });
        window.ResizeObserver = MockResizeObserver as any;
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
            expect(screen.getAllByTestId(/^media-card-id-/)).toHaveLength(2);
        });

        // Verify initial state
        const id1Card = screen.getByTestId('media-card-id-1');
        expect(id1Card.textContent).toContain('initial-phash');
        expect(id1Card.textContent).toContain('100'); // width

        // Trigger update (simulate fix thumbnail result)
        const updatedItem = {
            ...mockMediaItems[1], // id-1 is second in mockMediaItems now
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
            const updatedCard = screen.getByTestId('media-card-id-1');
            expect(updatedCard.textContent).toContain('fixed-phash');
            expect(updatedCard.textContent).toContain('1920');
            expect(updatedCard.textContent).toContain('1080');
        });
    });

    it('navigates with keyboard and opens correct item', async () => {
        let currentLocation: any;
        const LocationTracker = () => {
            const location = useLocation();
            currentLocation = location;
            return null;
        };

        render(
            <MemoryRouter initialEntries={['/']}>
                <LocationTracker />
                <GalleryView
                    filter="all"
                    onFilterChange={vi.fn()}
                    refreshKey={0}
                    folders={[]}
                    onFoldersChanged={vi.fn()}
                    isActive={true}
                />
            </MemoryRouter>
        );

        // Wait for initial load
        await waitFor(() => {
            expect(screen.getAllByTestId(/^media-card-id-/)).toHaveLength(2);
        });

        // 1. Initial state: nothing focused
        expect(document.querySelector('.focused')).toBeNull();

        // 2. Press ArrowRight to focus first item (id-2)
        act(() => {
            window.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight' }));
        });
        
        const cards = screen.getAllByTestId(/^media-card-id-/);
        expect(cards[0].className).toContain('focused');
        expect(cards[0].getAttribute('data-id')).toBe('id-2');

        // 3. Press Enter to open id-2
        act(() => {
            window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter' }));
        });

        // Verify URL contains media=id-2
        expect(currentLocation.search).toContain('media=id-2');

        // 4. "Close" the modal to continue grid navigation
        // In real app, modal onClose clears the search param
        act(() => {
            // We can't call internal GalleryView functions easily, 
            // but we can dispatch a navigation event if we had a way.
            // For this test, let's just render a version without selectedMediaId 
            // or assume we stay focused.
        });

        // Actually, let's just verify that it DOES NOT navigate while "modal" is open
        act(() => {
            window.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight' }));
        });
        // Still id-2 should be focused (ArrowRight was ignored)
        expect(cards[0].className).toContain('focused');
        expect(cards[1].className).not.toContain('focused');
    });

    it('ignores keyboard events when isActive is false', async () => {
        render(
            <MemoryRouter>
                <GalleryView
                    filter="all"
                    onFilterChange={vi.fn()}
                    refreshKey={0}
                    folders={[]}
                    onFoldersChanged={vi.fn()}
                    isActive={false}
                />
            </MemoryRouter>
        );

        await waitFor(() => {
            expect(screen.getAllByTestId(/^media-card-id-/)).toHaveLength(2);
        });

        // Press ArrowRight
        act(() => {
            window.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight' }));
        });

        // Nothing should be focused
        expect(document.querySelector('.focused')).toBeNull();
    });
});
