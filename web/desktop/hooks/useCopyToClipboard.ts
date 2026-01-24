import { useState, useCallback, useRef, useEffect } from 'react';

/**
 * Return type for useCopyToClipboard hook
 */
export interface UseCopyToClipboardReturn {
  /**
   * Copy text to clipboard with an optional key for tracking.
   * @param text - The text to copy
   * @param key - Optional key to identify this copy action (defaults to 'default')
   * @returns Promise that resolves to true if successful, false otherwise
   */
  copy: (text: string, key?: string) => Promise<boolean>;
  /**
   * Check if a specific key was recently copied.
   * @param key - The key to check (defaults to 'default')
   * @returns true if this key was the most recent copy within the timeout period
   */
  isCopied: (key?: string) => boolean;
  /**
   * The currently copied key, or null if nothing was recently copied.
   */
  copiedKey: string | null;
}

/**
 * Hook for copying text to clipboard with feedback state.
 * 
 * Provides a consistent UX pattern for copy buttons:
 * - Shows "Copied" feedback for a configurable duration
 * - Supports multiple independent copy actions via keys
 * - Handles errors gracefully
 * 
 * @param timeout - Duration in ms to show "copied" feedback (default: 2000)
 * 
 * @example
 * // Simple usage with single copy button
 * const { copy, isCopied } = useCopyToClipboard();
 * <Button onClick={() => copy(text)}>
 *   {isCopied() ? 'Copied!' : 'Copy'}
 * </Button>
 * 
 * @example
 * // Multiple copy buttons with keys
 * const { copy, isCopied } = useCopyToClipboard();
 * {items.map((item, i) => (
 *   <Button onClick={() => copy(item.text, `item-${i}`)}>
 *     {isCopied(`item-${i}`) ? 'Copied!' : 'Copy'}
 *   </Button>
 * ))}
 */
export function useCopyToClipboard(timeout = 2000): UseCopyToClipboardReturn {
  const [copiedKey, setCopiedKey] = useState<string | null>(null);
  const timeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Cleanup timeout on unmount
  useEffect(() => {
    return () => {
      if (timeoutRef.current) {
        clearTimeout(timeoutRef.current);
      }
    };
  }, []);

  const copy = useCallback(async (text: string, key = 'default'): Promise<boolean> => {
    try {
      await navigator.clipboard.writeText(text);
      
      // Clear any existing timeout
      if (timeoutRef.current) {
        clearTimeout(timeoutRef.current);
      }
      
      setCopiedKey(key);
      timeoutRef.current = setTimeout(() => {
        setCopiedKey(null);
        timeoutRef.current = null;
      }, timeout);
      
      return true;
    } catch (err) {
      console.error('Failed to copy to clipboard:', err);
      return false;
    }
  }, [timeout]);

  const isCopied = useCallback((key = 'default'): boolean => {
    return copiedKey === key;
  }, [copiedKey]);

  return { copy, isCopied, copiedKey };
}
