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
    http.post('http://localhost:3000/api/search', async ({ request }) => {
        const formData = await request.formData();
        const file = formData.get('file');
        const sim = formData.get('similarity');
        if (!file || !sim) return new HttpResponse(null, { status: 400 });
        return HttpResponse.json([
            { id: 'similar-2', filename: 'upload-similar.jpg', original_date: '2024-01-01', media_type: 'image', is_favorite: false, tags: [] }
        ]);
    })
);

beforeEach(() => {
    vi.stubGlobal('location', { origin: 'http://localhost:3000' });
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
});
