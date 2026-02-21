import React from 'react';
import type { Face, MediaSummary } from '../types';

interface FaceThumbnailProps {
    face: Face;
    media: MediaSummary;
    className?: string;
    size?: 'sm' | 'md' | 'lg' | 'xl';
}

export default function FaceThumbnail({ face, media, className = "" }: FaceThumbnailProps) {
    const getThumbnailUrl = (uuid: string) => {
        const p1 = uuid.substring(0, 2);
        const p2 = uuid.substring(2, 4);
        return `/thumbnails/${p1}/${p2}/${uuid}.jpg`;
    };

    // Original image dimensions
    const imgWidth = media.width || 1;
    const imgHeight = media.height || 1;

    // Face bounding box coordinates (absolute pixels)
    const { box_x1, box_y1, box_x2, box_y2 } = face;
    
    // Calculate face center as percentage of original image
    const faceCenterX = box_x1 + (box_x2 - box_x1) / 2;
    const faceCenterY = box_y1 + (box_y2 - box_y1) / 2;
    
    const centerXPercent = (faceCenterX / imgWidth) * 100;
    const centerYPercent = (faceCenterY / imgHeight) * 100;

    // Zoom level
    const zoom = 2.5;

    // Determine orientation for fitting strategy
    // We want the image to fully cover the container in at least one dimension
    // but ideally match the aspect ratio so coordinates work.
    // Actually, we just need the image rendered size to match its intrinsic aspect ratio.
    // If we use min-width/min-height 100%? No.
    // We conditionally set width/height based on aspect ratio compared to container (square).
    
    const isLandscape = imgWidth > imgHeight;
    const baseStyle: React.CSSProperties = isLandscape 
        ? { height: '100%', width: 'auto', maxWidth: 'none' } 
        : { width: '100%', height: 'auto', maxWidth: 'none' };

    return (
        <div className={`aspect-square relative overflow-hidden bg-gray-200 dark:bg-gray-800 shadow-sm transition-all duration-500 ${className}`}>
            <img
                src={getThumbnailUrl(media.id)}
                alt="Face"
                className="block transition-all duration-700"
                style={{
                    ...baseStyle,
                    position: 'absolute',
                    left: '50%',
                    top: '50%',
                    transformOrigin: `${centerXPercent}% ${centerYPercent}%`,
                    transform: `translate(-${centerXPercent}%, -${centerYPercent}%) scale(${zoom})`
                }}
                loading="lazy"
                decoding="async"
            />
        </div>
    );
}
