import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook } from '@testing-library/react';
import { useWebSocket } from './useWebSocket';
import * as events from './events';

// Mock the events module
vi.mock('./events', () => ({
    fireMediaUpdate: vi.fn(),
}));

describe('useWebSocket', () => {
    let mockWebSocket: any;
    const onFoldersChanged = vi.fn();
    const onUploadComplete = vi.fn();

    beforeEach(() => {
        vi.useFakeTimers();
        
        // WebSocket mock class
        class MockWebSocket {
            send = vi.fn();
            close = vi.fn();
            onopen: any = null;
            onmessage: any = null;
            onclose: any = null;
            onerror: any = null;
            url: string;

            constructor(url: string) {
                this.url = url;
                mockWebSocket = this;
            }
        }

        const WebSocketSpy = vi.fn(function(this: any, url: string) {
            return new MockWebSocket(url);
        });

        // Mock global WebSocket
        vi.stubGlobal('WebSocket', WebSocketSpy);
        
        // Mock location
        vi.stubGlobal('location', {
            protocol: 'http:',
            host: 'localhost:3000',
            origin: 'http://localhost:3000',
        });
    });

    afterEach(() => {
        vi.restoreAllMocks();
        vi.useRealTimers();
        vi.unstubAllGlobals();
    });

    it('connects to the correct URL', () => {
        renderHook(() => useWebSocket(onFoldersChanged, onUploadComplete, true));
        expect(WebSocket).toHaveBeenCalledWith('ws://localhost:3000/api/ws');
    });


    it('handles MediaCreated message', () => {
        renderHook(() => useWebSocket(onFoldersChanged, onUploadComplete, true));
        
        const mediaItem = { id: 'media-1', filename: 'test.jpg' };
        const event = {
            data: JSON.stringify({
                type: 'MediaCreated',
                data: { item: mediaItem }
            })
        };

        mockWebSocket.onmessage(event);

        expect(events.fireMediaUpdate).toHaveBeenCalledWith('media-1', mediaItem, 'create');
    });

    it('handles MediaUpdated message', () => {
        renderHook(() => useWebSocket(onFoldersChanged, onUploadComplete, true));
        
        const mediaItem = { is_favorite: true };
        const event = {
            data: JSON.stringify({
                type: 'MediaUpdated',
                data: { id: 'media-1', item: mediaItem }
            })
        };

        mockWebSocket.onmessage(event);

        expect(events.fireMediaUpdate).toHaveBeenCalledWith('media-1', mediaItem);
    });

    it('handles MediaDeleted message', () => {
        renderHook(() => useWebSocket(onFoldersChanged, onUploadComplete, true));
        
        const event = {
            data: JSON.stringify({
                type: 'MediaDeleted',
                data: { id: 'media-1' }
            })
        };

        mockWebSocket.onmessage(event);

        expect(events.fireMediaUpdate).toHaveBeenCalledWith('media-1', {}, 'delete');
        expect(onFoldersChanged).toHaveBeenCalled();
    });

    it('handles MediaBatchDeleted message', () => {
        renderHook(() => useWebSocket(onFoldersChanged, onUploadComplete, true));
        
        const event = {
            data: JSON.stringify({
                type: 'MediaBatchDeleted',
                data: { ids: ['media-1', 'media-2'] }
            })
        };

        mockWebSocket.onmessage(event);

        expect(events.fireMediaUpdate).toHaveBeenCalledWith('media-1', {}, 'delete');
        expect(events.fireMediaUpdate).toHaveBeenCalledWith('media-2', {}, 'delete');
        expect(onFoldersChanged).toHaveBeenCalled();
    });

    it('handles UploadComplete message with debouncing', () => {
        renderHook(() => useWebSocket(onFoldersChanged, onUploadComplete, true));
        
        const event = {
            data: JSON.stringify({
                type: 'UploadComplete',
                data: null
            })
        };

        mockWebSocket.onmessage(event);
        mockWebSocket.onmessage(event); // Second one

        expect(onUploadComplete).not.toHaveBeenCalled();

        // Fast forward time
        vi.advanceTimersByTime(500);

        expect(onUploadComplete).toHaveBeenCalledTimes(1);
    });

    it('reconnects on close', () => {
        renderHook(() => useWebSocket(onFoldersChanged, onUploadComplete, true));
        
        expect(WebSocket).toHaveBeenCalledTimes(1);

        mockWebSocket.onclose();

        // Fast forward time for reconnection
        vi.advanceTimersByTime(5000);

        expect(WebSocket).toHaveBeenCalledTimes(2);
    });

    it('does not connect when disabled', () => {
        renderHook(() => useWebSocket(onFoldersChanged, onUploadComplete, false));
        expect(WebSocket).not.toHaveBeenCalled();
    });

});
