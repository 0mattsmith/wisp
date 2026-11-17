/** The Dubplate mark: a record with a download arrow on its label. */
export function Logo({ size = 28 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 64 64" aria-hidden="true">
      <defs>
        <linearGradient id="dp-disc" x1="0.1" y1="0.05" x2="0.9" y2="0.95">
          <stop offset="0" stopColor="#f056c8" />
          <stop offset="0.5" stopColor="#8b5cf6" />
          <stop offset="1" stopColor="#3b82f6" />
        </linearGradient>
      </defs>
      <circle cx="32" cy="32" r="30" fill="url(#dp-disc)" />
      <g fill="none" stroke="#0b0e1c" strokeOpacity="0.28" strokeWidth="1">
        <circle cx="32" cy="32" r="25.500" />
        <circle cx="32" cy="32" r="21" />
      </g>
      <circle cx="32" cy="32" r="15" fill="#0b0e1c" />
      <g fill="none" stroke="#fff" strokeWidth="3.400" strokeLinecap="round" strokeLinejoin="round">
        <path d="M32 23.500v13" />
        <path d="M26.200 31.600l5.800 5.800 5.800-5.800" />
        <path d="M25.500 41.500h13" />
      </g>
    </svg>
  );
}

export function Wordmark() {
  return <span className="wordmark">Dubplate</span>;
}
