import type { ReactNode, CSSProperties } from 'react';
import { forwardRef } from 'react';
import styles from './Panel.module.css';

export type PanelVariant = 'solid' | 'transparent' | 'glass';
export type BorderType = 'none' | 'solid' | 'future';
export type BorderRadius = 'none' | 'sm' | 'md' | 'lg' | number;

export interface PanelProps {
  /** Background variant (matches Menu component) */
  variant?: PanelVariant;
  /** Border style variant */
  border?: BorderType;
  /** Border radius - preset or custom number */
  borderRadius?: BorderRadius;
  /** Whether the panel is in a focused state */
  focused?: boolean;
  /** Additional CSS class names */
  className?: string;
  /** Inline styles */
  style?: CSSProperties;
  /** Panel content */
  children?: ReactNode;
  /** Data attributes for identification */
  'data-window-id'?: number;
  /** Pointer down handler */
  onPointerDown?: (e: React.PointerEvent) => void;
}

// CSS module classes for variants
const variantClassMap: Record<PanelVariant, string> = {
  solid: styles.solid,
  transparent: styles.transparent,
  glass: styles.glass,
};

// CSS module classes for border styles (except 'future' which uses global class)
const borderClassMap: Record<BorderType, string> = {
  none: styles.borderNone,
  solid: styles.borderSolid,
  future: 'border-future', // Global class from @cypher-asi/zui/styles/borders.css
};

const radiusClassMap: Record<Exclude<BorderRadius, number>, string> = {
  none: styles.radiusNone,
  sm: styles.radiusSm,
  md: styles.radiusMd,
  lg: styles.radiusLg,
};

/**
 * Panel - A fundamental container component for the ZUI design system.
 * 
 * Provides a glass-morphism styled container with configurable borders
 * and border radius. Serves as the base building block for windows,
 * dialogs, menus, and other floating UI elements.
 * 
 * Default: glass background + future border (matches Menu component)
 * 
 * Variants:
 * - 'solid': Opaque dark background
 * - 'transparent': No background
 * - 'glass': Semi-transparent with backdrop blur (default)
 * 
 * Border types:
 * - 'none': No border
 * - 'solid': Simple dark border
 * - 'future': Corner accent style (default, uses global .border-future class)
 */
export const Panel = forwardRef<HTMLDivElement, PanelProps>(function Panel(
  {
    variant = 'glass',
    border = 'future',
    borderRadius = 'none',
    focused = false,
    className = '',
    style,
    children,
    'data-window-id': windowId,
    onPointerDown,
  },
  ref
) {
  const variantClass = variantClassMap[variant];
  const borderClass = borderClassMap[border];
  const radiusClass = typeof borderRadius === 'number' 
    ? '' 
    : radiusClassMap[borderRadius];
  
  const combinedStyle: CSSProperties = {
    ...style,
    ...(typeof borderRadius === 'number' ? { borderRadius } : {}),
  };

  const classNames = [
    styles.panel,
    variantClass,
    borderClass,
    radiusClass,
    focused ? styles.focused : '',
    className,
  ].filter(Boolean).join(' ');

  return (
    <div
      ref={ref}
      className={classNames}
      style={combinedStyle}
      data-window-id={windowId}
      onPointerDown={onPointerDown}
    >
      {children}
    </div>
  );
});

export default Panel;
