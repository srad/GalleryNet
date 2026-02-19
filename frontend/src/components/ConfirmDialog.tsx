import { useEffect, useRef } from 'react';

interface ConfirmDialogProps {
    isOpen: boolean;
    title: string;
    message: string;
    confirmLabel?: string;
    cancelLabel?: string;
    isDestructive?: boolean;
    onConfirm: () => void;
    onCancel: () => void;
}

export default function ConfirmDialog({
    isOpen,
    title,
    message,
    confirmLabel = "Confirm",
    cancelLabel = "Cancel",
    isDestructive = false,
    onConfirm,
    onCancel
}: ConfirmDialogProps) {
    const cancelRef = useRef<HTMLButtonElement>(null);
    const confirmRef = useRef<HTMLButtonElement>(null);

    // Focus management when opening
    useEffect(() => {
        if (isOpen) {
            // Small timeout to ensure render is complete and to play nice with focus transitions
            setTimeout(() => {
                cancelRef.current?.focus();
            }, 100);
        }
    }, [isOpen]);

    // Handle keyboard navigation
    useEffect(() => {
        if (!isOpen) return;

        const handleKeyDown = (e: KeyboardEvent) => {
            // Stop propagation to prevent background interactions (e.g. video seeking, gallery nav)
            e.stopPropagation();
            
            if (e.key === 'ArrowLeft') {
                e.preventDefault();
                cancelRef.current?.focus();
            } else if (e.key === 'ArrowRight') {
                e.preventDefault();
                confirmRef.current?.focus();
            } else if (e.key === 'Escape') {
                e.preventDefault();
                onCancel();
            }
            // Enter is handled natively by the button's onClick when focused
        };

        // Use capture phase to intercept events early
        window.addEventListener('keydown', handleKeyDown, true);
        return () => window.removeEventListener('keydown', handleKeyDown, true);
    }, [isOpen, onCancel]);

    if (!isOpen) return null;

    return (
        <div className="fixed inset-0 z-[60] flex items-center justify-center bg-black/50 backdrop-blur-sm p-4">
            <div className="bg-white dark:bg-gray-800 rounded-xl shadow-xl w-full max-w-sm overflow-hidden transform transition-all">
                <div className="p-6">
                    <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100 mb-2">{title}</h3>
                    <p className="text-sm text-gray-500 dark:text-gray-400">{message}</p>
                </div>
                <div className="bg-gray-50 dark:bg-gray-900 px-6 py-4 flex items-center justify-end gap-3 border-t border-gray-100 dark:border-gray-700">
                    <button
                        ref={cancelRef}
                        onClick={onCancel}
                        className="px-4 py-2 text-sm font-medium text-gray-700 dark:text-gray-300 bg-white dark:bg-gray-700 border border-gray-300 dark:border-gray-600 rounded-lg hover:bg-gray-50 dark:hover:bg-gray-600 focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-indigo-500 transition-colors"
                    >
                        {cancelLabel}
                    </button>
                    <button
                        ref={confirmRef}
                        onClick={onConfirm}
                        className={`px-4 py-2 text-sm font-medium text-white rounded-lg focus:outline-none focus:ring-2 focus:ring-offset-2 transition-colors ${
                            isDestructive
                                ? 'bg-red-600 hover:bg-red-700 focus:ring-red-500'
                                : 'bg-indigo-600 hover:bg-indigo-700 focus:ring-indigo-500'
                        }`}
                    >
                        {confirmLabel}
                    </button>
                </div>
            </div>
        </div>
    );
}
