import type { SVGProps } from 'react';

export function AppleIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg viewBox="0 0 24 24" width="1em" height="1em" fill="currentColor" {...props}>
      <path d="M18.71 19.5c-.83 1.24-1.71 2.45-3.05 2.47-1.34.03-1.77-.79-3.29-.79-1.53 0-2 .77-3.27.82-1.31.05-2.3-1.32-3.14-2.53C4.25 17 2.94 12.45 4.7 9.39c.87-1.52 2.43-2.48 4.12-2.51 1.28-.02 2.5.87 3.29.87.78 0 2.26-1.07 3.81-.91.65.03 2.47.26 3.64 1.98-.09.06-2.17 1.28-2.15 3.81.03 3.02 2.65 4.03 2.68 4.04-.03.07-.42 1.44-1.38 2.83M15.97 7.05c.61-.75 1.04-1.8 0.92-2.85-.92.04-2.02.62-2.67 1.38-.58.67-1.09 1.74-.96 2.77 1.03.08 2.08-.55 2.71-1.3" />
    </svg>
  );
}

export function WindowsIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg viewBox="0 0 24 24" width="1em" height="1em" fill="currentColor" {...props}>
      <path d="M3 5.454 10.125 4.5v6.75H3V5.454Zm0 13.092 7.125.954V12.75H3v5.796ZM11.25 4.35 21 3v8.25h-9.75V4.35Zm0 15.3 9.75-1.35V12.75h-9.75v6.9Z" />
    </svg>
  );
}

export function LinuxIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg viewBox="0 0 24 24" width="1em" height="1em" fill="currentColor" {...props}>
      <path d="M12.01 2c-2.58 0-4.04 2.1-4.04 4.54 0 1.02.26 2.17.65 3.07-.63.46-1.5 1.26-1.99 2.15-.55.99-.7 2.08-.41 3.19.26.97.87 1.78 1.68 2.29.21.94.75 1.83 1.51 2.45 1.03.84 2.37 1.31 3.73 1.31 1.48 0 2.91-.54 3.97-1.5.68-.62 1.15-1.45 1.32-2.36.78-.51 1.37-1.28 1.63-2.2.3-1.06.18-2.14-.34-3.11-.47-.88-1.29-1.68-1.92-2.14.39-.89.65-2.02.65-3.09C20.45 4.1 14.59 2 12.01 2zm-1.6 5.25a.85.85 0 1 1 0 1.7.85.85 0 0 1 0-1.7zm3.2 0a.85.85 0 1 1 0 1.7.85.85 0 0 1 0-1.7zm-1.6 2.45c.87 0 1.4.37 1.4.75 0 .38-.53.75-1.4.75-.87 0-1.4-.37-1.4-.75 0-.38.53-.75 1.4-.75z" />
    </svg>
  );
}

export function MicrosoftStoreIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg viewBox="0 0 24 24" width="1em" height="1em" fill="currentColor" {...props}>
      <path d="M4 4h16a1 1 0 0 1 1 1v15a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1zm3 3v2h10V7H7zm2 4a3 3 0 0 0 6 0H9z" />
    </svg>
  );
}

export function PlatformIcon({ platform, className }: { platform: 'macos' | 'windows' | 'linux' | 'store'; className?: string }) {
  switch (platform) {
    case 'macos':
      return <AppleIcon className={className} />;
    case 'windows':
      return <WindowsIcon className={className} />;
    case 'linux':
      return <LinuxIcon className={className} />;
    case 'store':
      return <MicrosoftStoreIcon className={className} />;
  }
}
