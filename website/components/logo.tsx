export function Logo({ size = 26 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 32 32" aria-hidden="true">
      <circle cx="16" cy="16" r="15.5" fill="none" stroke="#cecac8" />
      <g transform="translate(16 17)" fill="#242424">
        {[0, 60, 120, 180, 240, 300].map((r) => (
          <path
            key={r}
            d="M0 0 A4 4 0 0 1 0 -6 A4 4 0 0 1 0 0 Z"
            transform={`rotate(${r})`}
          />
        ))}
      </g>
    </svg>
  );
}
