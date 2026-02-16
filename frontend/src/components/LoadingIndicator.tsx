interface LoadingIndicatorProps {
    label?: string;
    description?: string;
    variant?: 'inline' | 'centered' | 'overlay';
    size?: 'sm' | 'md' | 'lg';
    color?: string;
    className?: string;
}

export default function LoadingIndicator({ 
    label, 
    description,
    variant = 'inline', 
    size = 'md',
    color = 'text-blue-600',
    className = "" 
}: LoadingIndicatorProps) {
    const sizeClasses = {
        sm: 'w-4 h-4',
        md: 'w-6 h-6',
        lg: 'w-10 h-10'
    };

    const spinner = (
        <svg className={`${sizeClasses[size]} animate-spin ${color}`} viewBox="0 0 24 24" fill="none">
            <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
            <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
        </svg>
    );

    if (variant === 'overlay') {
        return (
            <div className={`absolute inset-0 z-20 flex items-start justify-center pt-20 bg-gray-50/40 backdrop-blur-[1px] ${className}`}>
                <div className="flex items-center gap-3 px-5 py-3 bg-white rounded-2xl shadow-lg border border-gray-200 animate-in fade-in zoom-in duration-200">
                    {spinner}
                    {label && <span className="text-sm font-medium text-gray-700">{label}</span>}
                </div>
            </div>
        );
    }

    if (variant === 'centered') {
        return (
            <div className={`flex flex-col items-center justify-center gap-3 ${className}`}>
                {spinner}
                {label && <div className="text-gray-900 font-medium">{label}</div>}
                {description && <div className="text-gray-400 text-sm">{description}</div>}
            </div>
        );
    }

    // Default: inline
    return (
        <div className={`flex items-center gap-2 ${className}`}>
            {spinner}
            {label && <span className="text-sm font-medium">{label}</span>}
        </div>
    );
}
