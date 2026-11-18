/** The Wisp mark: a soft cloud whose outline sweeps into a download arrow. */
export function Logo({ size = 24 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="232 274 600 600" aria-hidden="true" className="logo">
      <defs>
        <linearGradient id="wisp-ink" x1="0.1" y1="0.1" x2="0.9" y2="0.9">
          <stop offset="0" stopColor="#9B86FF" />
          <stop offset="0.5" stopColor="#7C8CFF" />
          <stop offset="1" stopColor="#4CC4FF" />
        </linearGradient>
      </defs>
      <g fill="none" stroke="url(#wisp-ink)" strokeWidth="62" strokeLinecap="round" strokeLinejoin="round">
        <path d="M 364 610 C 270 610 252 492 338 470 C 340 378 456 346 506 406 C 552 318 700 332 712 454 C 796 464 806 604 712 612 C 650 618 590 600 556 618 C 524 636 518 676 518 732" />
        <path d="M 456 670 L 518 734 L 580 670" />
      </g>
    </svg>
  );
}

export function Wordmark() {
  return <span className="wordmark">Wisp</span>;
}
