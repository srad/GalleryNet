/**
 * Dispatches a custom 'gallerynet-unauthorized' event on the window.
 * App.tsx listens for this to redirect to the login screen.
 */
export function fireUnauthorized() {
    window.dispatchEvent(new CustomEvent('gallerynet-unauthorized'));
}

/**
 * Wrapper around fetch() that automatically fires an unauthorized event on 401.
 * Use this for all authenticated API calls.
 */
export async function apiFetch(input: RequestInfo | URL, init?: RequestInit): Promise<Response> {
    const res = await fetch(input, init);
    if (res.status === 401) {
        fireUnauthorized();
    }
    return res;
}
