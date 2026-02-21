import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import '@testing-library/jest-dom';
import MediaModal from './MediaModal';
import { apiClient } from '../api';
import { BrowserRouter } from 'react-router-dom';

// Mock dependencies
vi.mock('../api', () => ({
    apiClient: {
        getPeople: vi.fn(),
        updatePerson: vi.fn(),
        getMediaItem: vi.fn(),
        searchExternal: vi.fn(),
        updateMediaTags: vi.fn(),
        getTags: vi.fn(),
    }
}));

const mockNavigate = vi.fn();
vi.mock('react-router-dom', async () => {
    const actual = await vi.importActual('react-router-dom');
    return {
        ...actual,
        useNavigate: () => mockNavigate,
    };
});

const mockItem = {
    id: 'm1',
    filename: 'test.jpg',
    original_filename: 'test.jpg',
    media_type: 'image',
    phash: 'hash',
    uploaded_at: '2024-01-01',
    original_date: '2024-01-01',
    width: 1000,
    height: 1000,
    size_bytes: 1000,
    is_favorite: false,
    tags: [],
    faces: [
        { id: 'f1', media_id: 'm1', box_x1: 100, box_y1: 100, box_x2: 200, box_y2: 200, cluster_id: 1, person_id: 'p1' },
        { id: 'f2', media_id: 'm1', box_x1: 300, box_y1: 300, box_x2: 400, box_y2: 400, cluster_id: 2, person_id: 'p2' }
    ],
    faces_scanned: true
};

const mockPeople = [
    [{ id: 'p1', name: 'John Doe', is_hidden: false, face_count: 1 }, null, null]
];

describe('MediaModal', () => {
    beforeEach(() => {
        vi.resetAllMocks();
        (apiClient.getPeople as any).mockResolvedValue(mockPeople);
        (apiClient.getMediaItem as any).mockResolvedValue(mockItem);
        (apiClient.getTags as any).mockResolvedValue([]);
    });

    const renderModal = () => {
        render(
            <BrowserRouter>
                <MediaModal 
                    item={mockItem} 
                    onClose={() => {}} 
                    onPrev={null} 
                    onNext={null} 
                />
            </BrowserRouter>
        );
    };

    it('renders face overlays with names', async () => {
        renderModal();
        
        await waitFor(() => {
            expect(screen.getByText('John Doe')).toBeInTheDocument();
            expect(screen.getByText('Unnamed Person 2')).toBeInTheDocument();
        });
    });

    it('navigates to search when clicking a face name', async () => {
        renderModal();
        await waitFor(() => screen.getByText('John Doe'));

        fireEvent.click(screen.getByText('John Doe'));
        expect(mockNavigate).toHaveBeenCalledWith('/search?face=f1');
    });

    it('sets profile picture when clicking the icon', async () => {
        (apiClient.updatePerson as any).mockResolvedValue({});
        renderModal();
        await waitFor(() => screen.getByText('John Doe'));

        // Find the "Set as Profile Picture" button specifically for John Doe
        // Since the button is a sibling of the name, we can find the name and look nearby
        const nameButton = screen.getByText('John Doe');
        const container = nameButton.parentElement; // The div containing both name and profile button
        // Use non-null assertion since we know the structure
        const setProfileBtn = container!.querySelector('button[title="Set as Profile Picture"]');
        
        fireEvent.click(setProfileBtn!);

        expect(apiClient.updatePerson).toHaveBeenCalledWith('p1', { representative_face_id: 'f1' });
    });

    it('toggles face visibility', async () => {
        renderModal();
        await waitFor(() => screen.getByText('John Doe'));

        const toggleBtn = screen.getByText('Hide Identified People');
        fireEvent.click(toggleBtn);

        expect(screen.queryByText('John Doe')).not.toBeInTheDocument();
        expect(screen.getByText('Show Identified People')).toBeInTheDocument();
    });
});
