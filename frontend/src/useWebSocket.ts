import { useEffect, useRef, useCallback } from 'react';
import type { MediaItem, Folder, PersonWithFace } from './types';
import { fireMediaUpdate } from './events';

type WsMessage = {
    type: 'MediaCreated',
    data: { item: MediaItem }
} | {
    type: 'MediaUpdated',
    data: { id: string, item: Partial<MediaItem> }
} | {
    type: 'MediaDeleted',
    data: { id: string }
} | {
    type: 'MediaBatchDeleted',
    data: { ids: string[] }
} | {
    type: 'MediaTagsUpdated',
    data: { ids: string[], tags: string[] }
} | {
    type: 'FolderCreated',
    data: { folder: Folder }
} | {
    type: 'FolderDeleted',
    data: { id: string }
} | {
    type: 'FolderRenamed',
    data: { id: string, name: string }
} | {
    type: 'FoldersReordered',
    data: { ids: string[] }
} | {
    type: 'MediaAddedToFolder',
    data: { folder_id: string, media_ids: string[] }
} | {
    type: 'MediaRemovedFromFolder',
    data: { folder_id: string, media_ids: string[] }
} | {
    type: 'TagLearningComplete',
    data: { tag_name: string }
} | {
    type: 'FullRefresh',
    data: null
} | {
    type: 'UploadComplete',
    data: null
} | {
    type: 'ThumbnailFixStarted',
    data: null
} | {
    type: 'ThumbnailFixCompleted',
    data: { count: number }
} | {
    type: 'PersonUpdated',
    data: { id: string, person: PersonWithFace }
} | {
    type: 'PeopleMerged',
    data: { source_id: string, target_id: string }
};

export function useWebSocket(
    onFoldersChanged: () => void, 
    onUploadComplete: () => void, 
    onThumbnailFixStatusChange: (isFixing: boolean) => void,
    onPeopleChanged: () => void,
    enabled: boolean = true
) {

    const socketRef = useRef<WebSocket | null>(null);
    const debounceTimeoutRef = useRef<number | null>(null);

    const debouncedUploadComplete = useCallback(() => {
        if (debounceTimeoutRef.current) {
            window.clearTimeout(debounceTimeoutRef.current);
        }
        debounceTimeoutRef.current = window.setTimeout(() => {
            onUploadComplete();
            debounceTimeoutRef.current = null;
        }, 500); // Wait for 500ms of quiet before refreshing
    }, [onUploadComplete]);

    useEffect(() => {
        if (!enabled) return;

        let socket: WebSocket | null = null;
        let reconnectTimeout: number | null = null;

        const connect = () => {
            const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
            const host = window.location.host;
            const wsUrl = `${protocol}//${host}/api/ws`;

            socket = new WebSocket(wsUrl);
            socketRef.current = socket;

            socket.onopen = () => {
                console.log('WebSocket connected');
                if (reconnectTimeout) {
                    window.clearTimeout(reconnectTimeout);
                    reconnectTimeout = null;
                }
            };

            socket.onmessage = (event) => {
                try {
                    const msg: WsMessage = JSON.parse(event.data);

                    switch (msg.type) {
                        case 'MediaCreated':
                            fireMediaUpdate(msg.data.item.id!, msg.data.item, 'create');
                            break;
                        case 'MediaUpdated':
                            fireMediaUpdate(msg.data.id, msg.data.item);
                            break;
                        case 'MediaDeleted':
                            fireMediaUpdate(msg.data.id, {}, 'delete');
                            onFoldersChanged();
                            break;
                        case 'MediaBatchDeleted':
                            msg.data.ids.forEach(id => fireMediaUpdate(id, {}, 'delete'));
                            onFoldersChanged();
                            break;
                        case 'MediaTagsUpdated':
                            msg.data.ids.forEach(id => fireMediaUpdate(id, { tags: msg.data.tags.map(name => ({ name, is_auto: false })) }));
                            break;
                        case 'FolderCreated':
                        case 'FolderDeleted':
                        case 'FolderRenamed':
                        case 'FoldersReordered':
                        case 'MediaAddedToFolder':
                        case 'MediaRemovedFromFolder':
                            onFoldersChanged();
                            break;
                        case 'TagLearningComplete':
                        case 'FullRefresh':
                        case 'UploadComplete':
                            debouncedUploadComplete();
                            break;
                        case 'ThumbnailFixStarted':
                            onThumbnailFixStatusChange(true);
                            break;
                        case 'ThumbnailFixCompleted':
                            onThumbnailFixStatusChange(false);
                            break;
                        case 'PersonUpdated':
                        case 'PeopleMerged':
                            onPeopleChanged();
                            break;
                    }

                } catch (err) {
                    console.error('Failed to parse WS message', err);
                }
            };

            socket.onclose = () => {
                console.log('WebSocket disconnected, reconnecting...');
                socket = null;
                socketRef.current = null;
                // Add jitter to avoid thundering herd on server restart
                const jitter = Math.random() * 2000;
                reconnectTimeout = window.setTimeout(connect, 3000 + jitter);
            };

            socket.onerror = (err) => {
                console.error('WebSocket error:', err);
                socket?.close();
            };
        };

        connect();

        return () => {
            if (socket) socket.close();
            if (reconnectTimeout) window.clearTimeout(reconnectTimeout);
            socketRef.current = null;
        };
    }, [enabled, onFoldersChanged, debouncedUploadComplete, onThumbnailFixStatusChange]);
}
