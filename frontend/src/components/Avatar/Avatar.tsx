import React from 'react';
import Image from 'next/image';

// Size to dimension class mapping
const sizeClasses = {
  xs: 'w-6 h-6',
  sm: 'w-8 h-8',
  md: 'w-10 h-10',
  lg: 'w-12 h-12',
  xl: 'w-16 h-16',
} as const;

// Badge/indicator size mapping
const badgeSizeClasses = {
  xs: 'w-3 h-3',
  sm: 'w-3.5 h-3.5',
  md: 'w-4 h-4',
  lg: 'w-4.5 h-4.5',
  xl: 'w-6 h-6',
} as const;

// Fallback text size mapping
const textSizeClasses = {
  xs: 'text-xs',
  sm: 'text-sm',
  md: 'text-base',
  lg: 'text-lg',
  xl: 'text-2xl',
} as const;

export interface AvatarProps {
  src?: string;
  alt: string;
  size?: 'xs' | 'sm' | 'md' | 'lg' | 'xl';
  fallback?: string;
  verified?: boolean;
  online?: boolean;
}

const Avatar: React.FC<AvatarProps> = ({
  src,
  alt,
  size = 'md',
  fallback,
  verified = false,
  online = false,
}) => {
  const sizeClass = sizeClasses[size];
  const textSizeClass = textSizeClasses[size];
  const badgeSizeClass = badgeSizeClasses[size];
  
  return (
    <div className={`relative rounded-full overflow-hidden border border-border-default ${sizeClass}`}>
      {src && (
        <Image
          src={src}
          alt={alt}
          fill
          className="object-cover"
        />
      )}
      {!src && fallback && (
        <div
          className={`bg-elevated text-text-secondary flex items-center justify-center font-medium rounded-full border border-border-default ${textSizeClass}`}
          role="img"
          aria-label={alt}
        >
          {fallback}
        </div>
      )}
      {verified && (
        <div
          className={`absolute -bottom-0.5 -right-0.5 rounded-full bg-bg-primary border-2 border-emerald flex items-center justify-center ${badgeSizeClass}`}
          aria-label="Verified"
        >
          <svg
            viewBox="0 0 16 16"
            fill="none"
            xmlns="http://www.w3.org/2000/svg"
            className="w-full h-full p-0.5"
          >
            <path
              d="M13.3334 4L6.00002 11.3333L2.66669 8"
              stroke="rgb(16 185 129)"
              strokeWidth="2"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          </svg>
        </div>
      )}
      {online && !verified && (
        <div
          className={`absolute -bottom-0.5 -right-0.5 rounded-full bg-emerald border-2 border-bg-primary ${badgeSizeClass}`}
          aria-label="Online"
        />
      )}
    </div>
  );
};

export default Avatar;
