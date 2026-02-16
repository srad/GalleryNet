import {useState, useEffect, useCallback} from 'react';
import {ChevronDownIcon} from './Icons';
import {apiFetch} from '../auth';

interface Stats {
    version: string;
    total_files: number;
    total_images: number;
    total_videos: number;
    total_size_bytes: number;
    disk_free_bytes: number;
    disk_total_bytes: number;
}

function formatBytes(bytes: number): string {
    if (bytes === 0) return '0 B';
    const units = ['B', 'KB', 'MB', 'GB', 'TB'];
    const i = Math.floor(Math.log(bytes) / Math.log(1024));
    const val = bytes / Math.pow(1024, i);
    return `${val.toFixed(i > 0 ? 1 : 0)} ${units[i]}`;
}

export default function LibraryInfo({refreshKey}: { refreshKey: number }) {
    const [stats, setStats] = useState<Stats | null>(null);
    const [isStatsExpanded, setIsStatsExpanded] = useState(false);

    const fetchStats = useCallback(async () => {
        try {
            const res = await apiFetch('/api/stats');
            if (res.ok) setStats(await res.json());
        } catch { /* ignore */ }
    }, []);

    useEffect(() => {
        fetchStats();
    }, [fetchStats, refreshKey]);

    const diskUsedPercent = stats && stats.disk_total_bytes > 0
        ? Math.round(((stats.disk_total_bytes - stats.disk_free_bytes) / stats.disk_total_bytes) * 100)
        : 0;

    if (!stats) return null;

    return (
        <div className="mt-auto border-t border-gray-100 bg-white flex-shrink-0">
            <button
                onClick={() => setIsStatsExpanded(!isStatsExpanded)}
                className="w-full flex items-center gap-3 px-6 py-4 hover:bg-gray-50 transition-colors group"
                title={`Storage usage: ${diskUsedPercent}%`}
            >
                <svg className="w-4 h-4 text-gray-400 group-hover:text-gray-600 flex-shrink-0" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor">
                    <path strokeLinecap="round" strokeLinejoin="round" d="M3 13.125C3 12.504 3.504 12 4.125 12h2.25c.621 0 1.125.504 1.125 1.125v6.75C7.5 20.496 6.996 21 6.375 21h-2.25A1.125 1.125 0 013 19.875v-6.75zM9.75 8.625c0-.621.504-1.125 1.125-1.125h2.25c.621 0 1.125.504 1.125 1.125v11.25c0 .621-.504 1.125-1.125 1.125h-2.25a1.125 1.125 0 01-1.125-1.125V8.625zM16.5 4.125c0-.621.504-1.125 1.125-1.125h2.25c.621 0 1.125.504 1.125 1.125v15.75c0 .621-.504 1.125-1.125 1.125h-2.25a1.125 1.125 0 01-1.125-1.125V4.125z" />
                </svg>
                
                {isStatsExpanded ? (
                    <span className="flex-1 text-[11px] font-bold text-gray-400 uppercase tracking-widest text-left group-hover:text-gray-600 animate-in fade-in duration-300">
                        Library Info
                    </span>
                ) : (
                    <div className="flex-1 flex items-center gap-2 min-w-0">
                        <div className="flex-1 bg-gray-100 rounded-full h-1.5 overflow-hidden relative">
                            <div
                                className={`h-full rounded-full transition-all duration-500 ${
                                    diskUsedPercent > 90 ? 'bg-red-500' : diskUsedPercent > 70 ? 'bg-amber-500' : 'bg-blue-500'
                                }`}
                                style={{width: `${diskUsedPercent}%`}}
                            />
                        </div>
                        <div className="flex items-center gap-1.5 text-[10px] font-bold text-gray-400 group-hover:text-gray-600 tabular-nums whitespace-nowrap">
                            <span>{diskUsedPercent}%</span>
                            <span className="opacity-30">•</span>
                            <span>{stats.total_files}</span>
                        </div>
                    </div>
                )}
                
                <ChevronDownIcon className={`w-3.5 h-3.5 text-gray-400 group-hover:text-gray-600 transition-transform duration-300 ${isStatsExpanded ? 'rotate-180' : ''}`} />
            </button>

            <div className={`grid transition-all duration-300 ease-in-out ${isStatsExpanded ? 'grid-rows-[1fr] opacity-100' : 'grid-rows-[0fr] opacity-0'}`}>
                <div className="overflow-hidden">
                    <div className="px-4 pb-6">
                        <div className="bg-gray-50 rounded-xl p-3 space-y-2.5">
                            <div className="flex justify-between items-center">
                                <p className="text-[10px] text-gray-400">System Info</p>
                                <p className="text-[9px] text-gray-400">v{stats.version}</p>
                            </div>

                            <div className="grid grid-cols-3 gap-1 text-center">
                                <div>
                                    <p className="text-lg font-bold text-gray-900 leading-tight">{stats.total_files}</p>
                                    <p className="text-[10px] text-gray-400">Total</p>
                                </div>
                                <div>
                                    <p className="text-lg font-bold text-blue-600 leading-tight">{stats.total_images}</p>
                                    <p className="text-[10px] text-gray-400">Photos</p>
                                </div>
                                <div>
                                    <p className="text-lg font-bold text-purple-600 leading-tight">{stats.total_videos}</p>
                                    <p className="text-[10px] text-gray-400">Videos</p>
                                </div>
                            </div>

                            <div className="border-t border-gray-200 pt-2">
                                <div className="flex justify-between text-[11px] text-gray-500 mb-1">
                                    <span>Storage used</span>
                                    <span>{formatBytes(stats.total_size_bytes)}</span>
                                </div>
                            </div>

                            <div>
                                <div className="flex justify-between text-[11px] text-gray-500 mb-1">
                                    <span>Disk</span>
                                    <span>{formatBytes(stats.disk_free_bytes)} free</span>
                                </div>
                                <div className="w-full bg-gray-200 rounded-full h-1.5 overflow-hidden">
                                    <div
                                        className={`h-1.5 rounded-full transition-all ${
                                            diskUsedPercent > 90 ? 'bg-red-500' : diskUsedPercent > 70 ? 'bg-amber-500' : 'bg-green-500'
                                        }`}
                                        style={{width: `${diskUsedPercent}%`}}
                                    />
                                </div>
                            </div>
                        </div>
                    </div>
                </div>
            </div>
        </div>
    );
}
