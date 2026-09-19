/**
 * Inline SVG icon set.
 *
 * Recreated from the design's toolbar/menu glyphs as line icons (1.6px stroke,
 * `currentColor` so callers control the tint via CSS). No emoji and no external
 * icon font — everything ships in the bundle. Each icon is a 12–13px viewBox to
 * match the mockup's toolbar button sizing.
 */

import type { SVGProps } from "react";

type IconProps = SVGProps<SVGSVGElement> & { size?: number };

/** Shared wrapper: sets size and inherits color. */
function Svg({
  size = 13,
  children,
  ...rest
}: IconProps & { children: React.ReactNode }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 12 12"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.6}
      {...rest}
    >
      {children}
    </svg>
  );
}

export const AddIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M6 1v10M1 6h10" />
  </Svg>
);

export const SearchIcon = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="5.2" cy="5.2" r="3.7" />
    <path d="M7.9 7.9 10.8 10.8" />
  </Svg>
);

export const GearIcon = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="6" cy="6" r="2.1" />
    <path d="M6 1.2v1.5M6 9.3v1.5M1.2 6h1.5M9.3 6h1.5M2.6 2.6l1.1 1.1M8.3 8.3l1.1 1.1M9.4 2.6 8.3 3.7M3.7 8.3 2.6 9.4" />
  </Svg>
);

export const StatsIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M2 10V6M6 10V2M10 10V4" />
  </Svg>
);

export const StopIcon = (p: IconProps) => (
  <Svg {...p} strokeWidth={0} fill="currentColor">
    <rect x="2" y="2" width="8" height="8" />
  </Svg>
);

export const MagnetIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M2.5 1v5a3.5 3.5 0 0 0 7 0V1" />
    <path d="M1 1.5h3M8 1.5h3" />
  </Svg>
);

export const RemoveIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M1 6h10" />
  </Svg>
);

export const PlayIcon = (p: IconProps) => (
  <Svg {...p} strokeWidth={0} fill="currentColor">
    <polygon points="3,1.5 10.5,6 3,10.5" />
  </Svg>
);

export const PauseIcon = (p: IconProps) => (
  <Svg {...p} strokeWidth={0} fill="currentColor">
    <rect x="2.5" y="1.5" width="2.6" height="9" />
    <rect x="7" y="1.5" width="2.6" height="9" />
  </Svg>
);

export const UpIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M6 10.5V2M2.8 5.2 6 2l3.2 3.2" />
  </Svg>
);

export const DownIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M6 1.5V10M2.8 6.8 6 10l3.2-3.2" />
  </Svg>
);

export const RateLimitIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M1.5 3h9M1.5 9h9" />
    <path d="m3.5 1.5-2 1.5 2 1.5M8.5 7.5l2 1.5-2 1.5" />
  </Svg>
);

export const RecheckIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M10 3.5A4.5 4.5 0 1 0 10.5 7" />
    <path d="M10.5 1.5V4H8" />
  </Svg>
);

export const LabelIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M1.5 1.5h5l4 4-5 5-4-4z" />
    <circle cx="3.6" cy="3.6" r="0.7" fill="currentColor" stroke="none" />
  </Svg>
);

export const FolderIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M1 3h3l1 1.2h6V10H1z" />
  </Svg>
);

export const LinkIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M4.5 7.5 7.5 4.5" />
    <path d="M5 2.5 6.5 1a2 2 0 0 1 2.8 2.8L7.8 5.3" />
    <path d="M7 9.5 5.5 11a2 2 0 0 1-2.8-2.8L4.2 6.7" />
  </Svg>
);

export const OpenIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M4.5 1.5H1.5v9h9v-3" />
    <path d="M7 1.5h3.5V5M10.5 1.5 5.5 6.5" />
  </Svg>
);

export const CloseIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M2 2l8 8M10 2l-8 8" />
  </Svg>
);

export const ChevronRight = (p: IconProps) => (
  <Svg {...p}>
    <path d="M4.5 2.5 8 6l-3.5 3.5" />
  </Svg>
);

export const CreateTorrentIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M2 1.5h5l3 3V10.5H2z" />
    <path d="M7 1.5v3h3" />
    <path d="M4 7h4M6 5v4" />
  </Svg>
);

export const SessionIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M2 4.5h8v6H2z" />
    <path d="M4 4.5V2.8h4v1.7" />
    <path d="M5 7.2 6 6l1 1.2M6 6v3" />
  </Svg>
);
