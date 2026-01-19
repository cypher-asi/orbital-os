// Desktop Environment Types
// Stage 1.8: Shared types for the desktop environment

// =============================================================================
// Math Types
// =============================================================================

export interface Vec2 {
  x: number;
  y: number;
}

export interface Size {
  width: number;
  height: number;
}

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export function vec2(x: number, y: number): Vec2 {
  return { x, y };
}

export function size(width: number, height: number): Size {
  return { width, height };
}

export function rect(x: number, y: number, width: number, height: number): Rect {
  return { x, y, width, height };
}

export function vec2Add(a: Vec2, b: Vec2): Vec2 {
  return { x: a.x + b.x, y: a.y + b.y };
}

export function vec2Sub(a: Vec2, b: Vec2): Vec2 {
  return { x: a.x - b.x, y: a.y - b.y };
}

export function vec2Scale(v: Vec2, s: number): Vec2 {
  return { x: v.x * s, y: v.y * s };
}

export function rectContains(r: Rect, p: Vec2): boolean {
  return p.x >= r.x && p.x < r.x + r.width && p.y >= r.y && p.y < r.y + r.height;
}

export function rectIntersects(a: Rect, b: Rect): boolean {
  return a.x < b.x + b.width && a.x + a.width > b.x &&
         a.y < b.y + b.height && a.y + a.height > b.y;
}

// =============================================================================
// Window Types
// =============================================================================

export type WindowId = number;
export type ProcessId = number;

export type WindowState = 'normal' | 'minimized' | 'maximized' | 'fullscreen';

export interface Window {
  id: WindowId;
  title: string;
  position: Vec2;
  size: Size;
  minSize: Size;
  maxSize: Size | null;
  state: WindowState;
  processId: ProcessId;
  zOrder: number;
  focused: boolean;
  // Content canvas for React rendering
  canvas: HTMLCanvasElement | null;
}

export interface WindowConfig {
  title: string;
  position?: Vec2;
  size: Size;
  minSize?: Size;
  maxSize?: Size;
  processId: ProcessId;
}

// Window regions for hit testing
export type WindowRegion =
  | 'title-bar'
  | 'content'
  | 'close-button'
  | 'minimize-button'
  | 'maximize-button'
  | 'resize-n'
  | 'resize-s'
  | 'resize-e'
  | 'resize-w'
  | 'resize-ne'
  | 'resize-nw'
  | 'resize-se'
  | 'resize-sw';

// =============================================================================
// Input Types
// =============================================================================

export type PointerButton = 'primary' | 'secondary' | 'middle';

export interface Modifiers {
  ctrl: boolean;
  alt: boolean;
  shift: boolean;
  meta: boolean;
}

export interface PointerEvent {
  type: 'down' | 'up' | 'move' | 'enter' | 'leave';
  position: Vec2;
  button: PointerButton | null;
  modifiers: Modifiers;
}

export interface KeyboardEvent {
  type: 'down' | 'up';
  key: string;
  code: string;
  modifiers: Modifiers;
}

export interface ScrollEvent {
  delta: Vec2;
  position: Vec2;
  modifiers: Modifiers;
}

export interface GestureEvent {
  type: 'pan' | 'pinch' | 'rotate';
  center: Vec2;
  delta: Vec2;
  scale: number;
  rotation: number;
}

export type InputEvent =
  | { kind: 'pointer'; event: PointerEvent }
  | { kind: 'keyboard'; event: KeyboardEvent }
  | { kind: 'scroll'; event: ScrollEvent }
  | { kind: 'gesture'; event: GestureEvent };

export type InputResult =
  | { type: 'handled' }
  | { type: 'unhandled' }
  | { type: 'forward'; windowId: WindowId; event: InputEvent };

// =============================================================================
// Camera and Viewport Types
// =============================================================================

/**
 * Camera state for a layer (desktop or void).
 * Matches Rust `Camera` struct in types.rs.
 */
export interface Camera {
  center: Vec2;
  zoom: number;
}

/**
 * Create a default camera (centered at origin, zoom 1.0)
 */
export function defaultCamera(): Camera {
  return { center: { x: 0, y: 0 }, zoom: 1.0 };
}

/**
 * Linear interpolation between two cameras
 */
export function lerpCamera(from: Camera, to: Camera, t: number): Camera {
  return {
    center: {
      x: from.center.x + (to.center.x - from.center.x) * t,
      y: from.center.y + (to.center.y - from.center.y) * t,
    },
    zoom: from.zoom + (to.zoom - from.zoom) * t,
  };
}

export interface Viewport {
  center: Vec2;
  zoom: number;
  screenSize: Size;
}

// =============================================================================
// View Mode Types
// =============================================================================

/**
 * Current viewing mode of the desktop.
 * 
 * The desktop can be in one of two states:
 * - "desktop": Viewing a single desktop with infinite zoom/pan
 * - "void": Zoomed out to see all desktops (the meta-layer)
 * - "transitioning": Animation in progress between modes
 * 
 * Transitions use opacity crossfade between layers - both layers render
 * simultaneously during transitions.
 */
export type ViewMode = 'desktop' | 'void' | 'transitioning';

/** @deprecated Use 'desktop' instead of 'workspace' */
export type LegacyViewMode = 'workspace' | 'void' | 'transitioning';

/**
 * Layer opacity values during crossfade transitions.
 * Both layers render simultaneously with complementary opacities.
 */
export interface LayerOpacities {
  /** Desktop layer opacity (0.0 = hidden, 1.0 = fully visible) */
  desktop: number;
  /** Void layer opacity (0.0 = hidden, 1.0 = fully visible) */
  void: number;
}

/**
 * Crossfade direction for transitions
 */
export type CrossfadeDirection = 'toVoid' | 'toDesktop';

// =============================================================================
// Theme Types
// =============================================================================

export type Theme = 'light' | 'dark';

export interface ThemeColors {
  background: string;
  windowBg: string;
  windowBorder: string;
  titleBar: string;
  titleBarActive: string;
  titleText: string;
  shadow: string;
  accent: string;
  textPrimary: string;
  textSecondary: string;
}

export const DARK_THEME: ThemeColors = {
  background: '#1a1a2e',
  windowBg: '#0f0f1a',
  windowBorder: '#333',
  titleBar: '#1a1a2e',
  titleBarActive: '#2a2a4e',
  titleText: '#e0e0e0',
  shadow: 'rgba(0,0,0,0.5)',
  accent: '#01f4cb',
  textPrimary: '#e0e0e0',
  textSecondary: '#888',
};

export const LIGHT_THEME: ThemeColors = {
  background: '#f0f0f5',
  windowBg: '#ffffff',
  windowBorder: '#d0d0d0',
  titleBar: '#e8e8e8',
  titleBarActive: '#d0d0d0',
  titleText: '#333',
  shadow: 'rgba(0,0,0,0.2)',
  accent: '#0066cc',
  textPrimary: '#1a1a1a',
  textSecondary: '#666',
};

// =============================================================================
// Style Constants
// =============================================================================

export const FRAME_STYLE = {
  titleBarHeight: 32,
  borderRadius: 8,
  borderWidth: 1,
  shadowBlur: 20,
  shadowOffsetY: 4,
  resizeHandleSize: 8,
  buttonSize: 12,
  buttonSpacing: 8,
  buttonMargin: 10,
};
