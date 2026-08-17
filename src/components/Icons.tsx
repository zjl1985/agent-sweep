import type { JSX } from "preact";

interface IconProps extends JSX.SVGAttributes<SVGSVGElement> {
  size?: number;
}

function Icon({ size = 16, children, ...props }: IconProps) {
  return (
    <svg
      aria-hidden="true"
      fill="none"
      height={size}
      viewBox="0 0 24 24"
      width={size}
      {...props}
    >
      {children}
    </svg>
  );
}

export function RefreshIcon({ size = 16, ...props }: IconProps) {
  return (
    <Icon size={size} {...props}>
      <path
        d="M21 12a9 9 0 1 1-2.64-6.36M21 3v6h-6"
        stroke="currentColor"
        stroke-width="1.8"
        stroke-linecap="round"
        stroke-linejoin="round"
      />
    </Icon>
  );
}

export function SearchIcon({ size = 16, ...props }: IconProps) {
  return (
    <Icon size={size} {...props}>
      <circle cx="11" cy="11" r="7" stroke="currentColor" stroke-width="1.8" />
      <path
        d="m20 20-3.8-3.8"
        stroke="currentColor"
        stroke-linecap="round"
        stroke-width="1.8"
      />
    </Icon>
  );
}

export function TrashIcon({ size = 16, ...props }: IconProps) {
  return (
    <Icon size={size} {...props}>
      <path
        d="M4 7h16M9 7V5a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2m3 0-1 13a1 1 0 0 1-1 1H8a1 1 0 0 1-1-1L6 7M10 11v6M14 11v6"
        stroke="currentColor"
        stroke-linecap="round"
        stroke-linejoin="round"
        stroke-width="1.8"
      />
    </Icon>
  );
}

export function ArchiveIcon({ size = 16, ...props }: IconProps) {
  return (
    <Icon size={size} {...props}>
      <path
        d="M3 8h18v3H3zM5 11v8a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-8M10 15h4"
        stroke="currentColor"
        stroke-linecap="round"
        stroke-linejoin="round"
        stroke-width="1.8"
      />
    </Icon>
  );
}

export function SettingsIcon({ size = 16, ...props }: IconProps) {
  return (
    <Icon size={size} {...props}>
      <circle cx="12" cy="12" r="3" stroke="currentColor" stroke-width="1.8" />
      <path
        d="M12 3v2.2M12 18.8V21M4.2 7.5l1.9 1.1M17.9 15.4l1.9 1.1M4.2 16.5l1.9-1.1M17.9 8.6l1.9-1.1"
        stroke="currentColor"
        stroke-linecap="round"
        stroke-width="1.8"
      />
    </Icon>
  );
}

export function FolderIcon({ size = 16, ...props }: IconProps) {
  return (
    <Icon size={size} {...props}>
      <path
        d="M3 7a1 1 0 0 1 1-1h5l2 2h9a1 1 0 0 1 1 1v9a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1z"
        stroke="currentColor"
        stroke-linejoin="round"
        stroke-width="1.8"
      />
    </Icon>
  );
}

export function SunIcon({ size = 16, ...props }: IconProps) {
  return (
    <Icon size={size} {...props}>
      <circle cx="12" cy="12" r="4" stroke="currentColor" stroke-width="1.8" />
      <path
        d="M12 2.5v2M12 19.5v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2.5 12h2M19.5 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"
        stroke="currentColor"
        stroke-linecap="round"
        stroke-width="1.8"
      />
    </Icon>
  );
}

export function MoonIcon({ size = 16, ...props }: IconProps) {
  return (
    <Icon size={size} {...props}>
      <path
        d="M20.5 14.5A8.5 8.5 0 0 1 9.5 3.5a8.5 8.5 0 1 0 11 11z"
        stroke="currentColor"
        stroke-linecap="round"
        stroke-linejoin="round"
        stroke-width="1.8"
      />
    </Icon>
  );
}

export function DatabaseIcon({ size = 16, ...props }: IconProps) {
  return (
    <Icon size={size} {...props}>
      <ellipse cx="12" cy="5.5" rx="7" ry="2.5" stroke="currentColor" stroke-width="1.8" />
      <path
        d="M5 5.5v13c0 1.4 3.1 2.5 7 2.5s7-1.1 7-2.5v-13M5 12c0 1.4 3.1 2.5 7 2.5s7-1.1 7-2.5"
        stroke="currentColor"
        stroke-width="1.8"
      />
    </Icon>
  );
}
