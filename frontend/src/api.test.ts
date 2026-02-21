import { describe, it, expect, beforeEach, afterEach, afterAll, vi } from 'vitest';
import { setupServer } from 'msw/node';
import { http, HttpResponse } from 'msw';
import { apiClient } from './api';

const server = setupServer(
    http.post('http://localhost:3000/api/media/download/plan', async () => {
        return HttpResponse.json({
            plan_id: 'plan-1',
            parts: [{ id: 'part-1', filename: 'test.zip', size_estimate: 100 }]
        });
    }),
    http.get('http://localhost:3000/api/media/download/stream/part-1', () => {
        const stream = new ReadableStream({
            start(controller) {
                controller.enqueue(new TextEncoder().encode('chunk1'));
                controller.enqueue(new TextEncoder().encode('chunk2'));
                controller.close();
            },
        });
        return new HttpResponse(stream, {
            headers: {
                'Content-Type': 'application/zip',
                'Content-Disposition': 'attachment; filename="test.zip"',
                'Content-Length': '12',
            },
        });
    }),
    http.get('http://localhost:3000/api/media/stream-1/similar', () => {
        return HttpResponse.json([
            { id: 'similar-1', filename: 'similar.jpg', original_date: '2024-01-01', media_type: 'image', is_favorite: false, tags: [] }
        ]);
    }),
    http.post('http://localhost:3000/api/search', async () => {
        // Skip parsing formData to avoid jsdom/msw hang
        return HttpResponse.json([
            { id: 'similar-2', filename: 'upload-similar.jpg', original_date: '2024-01-01', media_type: 'image', is_favorite: false, tags: [] }
        ]);
    }),
    http.get('http://localhost:3000/api/people', () => {
        return HttpResponse.json([
            [{ id: 'p1', name: 'John Doe', is_hidden: false, face_count: 1 }, { id: 'f1', box_x1: 0, box_y1: 0, box_x2: 10, box_y2: 10 }, { id: 'm1' }],
            [{ id: 'p2', name: null, is_hidden: false, face_count: 1 }, { id: 'f2', box_x1: 0, box_y1: 0, box_x2: 10, box_y2: 10 }, { id: 'm2' }]
        ]);
    }),
    http.put('http://localhost:3000/api/people/p1', async ({ request }) => {
        const body = await request.json() as any;
        if (body.name === 'Jane Doe') {
            return new HttpResponse(null, { status: 200 });
        }
        return new HttpResponse(null, { status: 400 });
    }),
    http.post('http://localhost:3000/api/people/p1/merge', async ({ request }) => {
        const body = await request.json() as any;
        if (body.target_id === 'p2') {
            return new HttpResponse(null, { status: 200 });
        }
        return new HttpResponse(null, { status: 400 });
    })

);


beforeEach(() => {
    vi.stubGlobal('location', {
        href: 'http://localhost:3000/',
        origin: 'http://localhost:3000',
        protocol: 'http:',
        host: 'localhost:3000',
        hostname: 'localhost',
        port: '3000',
        pathname: '/',
        search: '',
        hash: '',
    });
    server.listen();
});

afterEach(() => {
    server.resetHandlers();
    vi.unstubAllGlobals();
});
afterAll(() => server.close());

describe('ApiClient', () => {
    it('gets a download plan', async () => {
        const plan = await apiClient.getDownloadPlan(['id1', 'id2']);
        expect(plan.plan_id).toBe('plan-1');
        expect(plan.parts).toHaveLength(1);
        expect(plan.parts[0].id).toBe('part-1');
    });

    it('downloads a stream part with progress', async () => {
        const onProgress = vi.fn();
        const part = { id: 'part-1', filename: 'test.zip', size_estimate: 100 };
        
        const { blob } = await apiClient.downloadStreamPart(part, 1, 1, onProgress);
        
        expect(blob.size).toBe(12);
        expect(onProgress).toHaveBeenCalled();
        const lastCall = onProgress.mock.calls[onProgress.mock.calls.length - 1][0];
        expect(lastCall.received).toBe(12);
        expect(lastCall.total).toBe(12);
    });

    it('searches similar by ID', async () => {
        const results = await apiClient.searchSimilarById('stream-1', 70);
        expect(results).toHaveLength(1);
        expect(results[0].id).toBe('similar-1');
    });

    it('searches similar by file', async () => {
        const file = new File(['fake-image'], 'test.jpg', { type: 'image/jpeg' });
        const results = await apiClient.searchSimilarByFile(file, 70);
        expect(results).toHaveLength(1);
        expect(results[0].id).toBe('similar-2');
    });

    it('fetches people list', async () => {
        const people = await apiClient.getPeople();
        expect(people).toHaveLength(2);
        expect(people[0][0].name).toBe('John Doe');
        expect(people[1][0].name).toBeNull();
    });

    it('updates a person', async () => {
        await expect(apiClient.updatePerson('p1', { name: 'Jane Doe' })).resolves.not.toThrow();
    });

    it('merges people', async () => {
        await expect(apiClient.mergePeople('p1', 'p2')).resolves.not.toThrow();
    });
});

