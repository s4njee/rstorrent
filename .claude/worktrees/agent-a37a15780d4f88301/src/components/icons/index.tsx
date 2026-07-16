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
