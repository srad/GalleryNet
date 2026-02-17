interface AlertDialogProps {
    isOpen: boolean;
    title: string;
    message: string;
    onClose: () => void;
}

export default function AlertDialog({
    isOpen,
    title,
    message,
    onClose
}: AlertDialogProps) {
    if (!isOpen) return null;

    return (
        <div className="fixed inset-0 z-[60] flex items-center justify-center bg-black/50 backdrop-blur-sm p-4">
            <div className="bg-white dark:bg-gray-800 rounded-xl shadow-xl w-full max-w-sm overflow-hidden transform transition-all">
                <div className="p-6">
                    <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100 mb-2">{title}</h3>
                    <div className="text-sm text-gray-500 dark:text-gray-400 whitespace-pre-wrap">{message}</div>
                </div>
                <div className="bg-gray-50 dark:bg-gray-900 px-6 py-4 flex items-center justify-end border-t border-gray-100 dark:border-gray-700">
                    <button
                        onClick={onClose}
                        className="px-4 py-2 text-sm font-medium text-white bg-indigo-600 hover:bg-indigo-700 rounded-lg focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-indigo-500 transition-colors"
                    >
                        OK
                    </button>
                </div>
            </div>
        </div>
    );
}
