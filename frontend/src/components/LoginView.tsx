import { useState, useCallback, type FormEvent } from 'react';

interface LoginViewProps {
    onLogin: () => void;
}

export default function LoginView({ onLogin }: LoginViewProps) {
    const [password, setPassword] = useState('');
    const [error, setError] = useState('');
    const [loading, setLoading] = useState(false);

    const handleSubmit = useCallback(async (e: FormEvent) => {
        e.preventDefault();
        setError('');
        setLoading(true);

        try {
            const res = await fetch('/api/login', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ password }),
            });

            if (res.ok) {
                onLogin();
            } else {
                const data = await res.json().catch(() => ({}));
                setError(data.error || 'Invalid password');
            }
        } catch {
            setError('Connection failed');
        } finally {
            setLoading(false);
        }
    }, [password, onLogin]);

    return (
        <div className="flex items-center justify-center min-h-screen bg-gray-50 dark:bg-gray-900">
            <div className="w-full max-w-sm">
                <div className="bg-white dark:bg-gray-800 rounded-2xl shadow-lg border border-gray-200 dark:border-gray-700 p-8">
                    <div className="text-center mb-8">
                        <h1 className="text-3xl font-bold text-transparent bg-clip-text bg-gradient-to-r from-blue-600 to-purple-600">
                            GalleryNet
                        </h1>
                        <p className="text-sm text-gray-500 dark:text-gray-400 mt-2">Enter your password to continue</p>
                    </div>

                    <form onSubmit={handleSubmit} className="space-y-4">
                        <div>
                            <input
                                type="password"
                                value={password}
                                onChange={e => setPassword(e.target.value)}
                                placeholder="Password"
                                autoFocus
                                required
                                className="w-full px-4 py-3 rounded-lg border border-gray-300 dark:border-gray-600 focus:border-blue-500 focus:ring-2 focus:ring-blue-200 dark:focus:ring-blue-800 outline-none transition-all text-sm dark:bg-gray-700 dark:text-gray-100 dark:placeholder:text-gray-400"
                            />
                        </div>

                        {error && (
                            <p className="text-sm text-red-600 dark:text-red-400 text-center">{error}</p>
                        )}

                        <button
                            type="submit"
                            disabled={loading || !password}
                            className="w-full py-3 rounded-lg bg-gradient-to-r from-blue-600 to-purple-600 text-white font-medium text-sm hover:from-blue-700 hover:to-purple-700 disabled:opacity-50 disabled:cursor-not-allowed transition-all"
                        >
                            {loading ? 'Signing in...' : 'Sign In'}
                        </button>
                    </form>
                </div>
            </div>
        </div>
    );
}
