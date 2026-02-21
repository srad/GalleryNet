import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook } from '@testing-library/react';
import { useWebSocket } from './useWebSocket';
import * as events from './events';

// Mock the events module
vi.mock('./events', () => ({
    fireMediaUpdate: vi.fn(),
}));

class MockWebSocket {
    send = vi.fn();
    close = vi.fn();
    onopen: (() => void) | null = null;
    onmessage: ((event: { data: string }) => void) | null = null;
    onclose: (() => void) | null = null;
    onerror: ((error: Error) => void) | null = null;
    url: string;

    constructor(url: string) {
        this.url = url;
    }
}

describe('useWebSocket', () => {
    let mockWebSocket: MockWebSocket;
    const onFoldersChanged = vi.fn();
    const onUploadComplete = vi.fn();
    const onThumbnailFixStatusChange = vi.fn();
    const onPeopleChanged = vi.fn();

    beforeEach(() => {
        vi.useFakeTimers();
        onUploadComplete.mockClear();
        onFoldersChanged.mockClear();
        onThumbnailFixStatusChange.mockClear();
        onPeopleChanged.mockClear();
        
        // Setup WebSocket mock

        // We use a regular function so it can be called with 'new'
        const WebSocketSpy = vi.fn(function(url: string) {
            mockWebSocket = new MockWebSocket(url);
            return mockWebSocket;
        });
        
        vi.stubGlobal('WebSocket', WebSocketSpy);
        
        // Mock location
        vi.stubGlobal('location', {
            protocol: 'http:',
            host: 'localhost:3000',
            origin: 'http://localhost:3000',
        } as Location);
    });

    afterEach(() => {
        vi.restoreAllMocks();
        vi.useRealTimers();
        vi.unstubAllGlobals();
    });

    it('connects to the correct URL', () => {
        renderHook(() => useWebSocket(onFoldersChanged, onUploadComplete, onThumbnailFixStatusChange, onPeopleChanged, true));
        expect(WebSocket).toHaveBeenCalledWith('ws://localhost:3000/api/ws');
    });

    it('handles MediaCreated message', () => {
        renderHook(() => useWebSocket(onFoldersChanged, onUploadComplete, onThumbnailFixStatusChange, onPeopleChanged, true));
        
        const mediaItem = { id: 'media-1', filename: 'test.jpg' };
        const event = {
            data: JSON.stringify({
                type: 'MediaCreated',
                data: { item: mediaItem }
            })
        };

        // Simulate receiving message
        mockWebSocket.onmessage?.(event);

        expect(events.fireMediaUpdate).toHaveBeenCalledWith('media-1', mediaItem, 'create');
    });

    it('handles MediaUpdated message', () => {
        renderHook(() => useWebSocket(onFoldersChanged, onUploadComplete, onThumbnailFixStatusChange, onPeopleChanged, true));
        
        const mediaItem = { is_favorite: true };
        const event = {
            data: JSON.stringify({
                type: 'MediaUpdated',
                data: { id: 'media-1', item: mediaItem }
            })
        };

        mockWebSocket.onmessage?.(event);

        expect(events.fireMediaUpdate).toHaveBeenCalledWith('media-1', mediaItem);
    });

    it('handles MediaDeleted message', () => {
        renderHook(() => useWebSocket(onFoldersChanged, onUploadComplete, onThumbnailFixStatusChange, onPeopleChanged, true));
        
        const event = {
            data: JSON.stringify({
                type: 'MediaDeleted',
                data: { id: 'media-1' }
            })
        };

        mockWebSocket.onmessage?.(event);

        expect(events.fireMediaUpdate).toHaveBeenCalledWith('media-1', {}, 'delete');
        expect(onFoldersChanged).toHaveBeenCalled();
    });

    it('handles UploadComplete message with debouncing', () => {
        renderHook(() => useWebSocket(onFoldersChanged, onUploadComplete, onThumbnailFixStatusChange, onPeopleChanged, true));
        
        const event = {
            data: JSON.stringify({
                type: 'UploadComplete',
                data: null
            })
        };

        mockWebSocket.onmessage?.(event);
        mockWebSocket.onmessage?.(event); // Second one

        expect(onUploadComplete).not.toHaveBeenCalled();

        // Fast forward time
        vi.advanceTimersByTime(500);

        expect(onUploadComplete).toHaveBeenCalledTimes(1);
    });

    it('handles ThumbnailFixStarted message', () => {
        renderHook(() => useWebSocket(onFoldersChanged, onUploadComplete, onThumbnailFixStatusChange, onPeopleChanged, true));

        const event = {
            data: JSON.stringify({
                type: 'ThumbnailFixStarted',
                data: null
            })
        };

        mockWebSocket.onmessage?.(event);

        expect(onThumbnailFixStatusChange).toHaveBeenCalledWith(true);
    });

    it('handles ThumbnailFixCompleted message', () => {
        renderHook(() => useWebSocket(onFoldersChanged, onUploadComplete, onThumbnailFixStatusChange, onPeopleChanged, true));

        const event = {
            data: JSON.stringify({
                type: 'ThumbnailFixCompleted',
                data: { count: 5 }
            })
        };

        mockWebSocket.onmessage?.(event);

        expect(onThumbnailFixStatusChange).toHaveBeenCalledWith(false);
        // Should NOT trigger upload complete (full refresh)
        expect(onUploadComplete).not.toHaveBeenCalled();
    });

    it('reconnects on close', () => {
        renderHook(() => useWebSocket(onFoldersChanged, onUploadComplete, onThumbnailFixStatusChange, onPeopleChanged, true));
        
        expect(WebSocket).toHaveBeenCalledTimes(1);

        mockWebSocket.onclose?.();

        // Fast forward time for reconnection
        vi.advanceTimersByTime(5000);

        expect(WebSocket).toHaveBeenCalledTimes(2);
    });

    it('does not connect when disabled', () => {
        renderHook(() => useWebSocket(onFoldersChanged, onUploadComplete, onThumbnailFixStatusChange, onPeopleChanged, false));
        expect(WebSocket).not.toHaveBeenCalled();
    });

});
