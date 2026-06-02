import { Icon as IconifyIcon } from "@iconify/react";

interface Props {
  icon: string;
  className?: string;
  style?: React.CSSProperties;
  size?: number;
}

export default function Icon({ icon, className, style, size = 16 }: Props) {
  return (
    <IconifyIcon
      icon={icon}
      className={className}
      style={{ ...style, width: size, height: size }}
      width={size}
      height={size}
    />
  );
}
