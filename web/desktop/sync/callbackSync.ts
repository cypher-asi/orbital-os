/**
 * Callback Sync - Event-driven updates from supervisor.
 * 
 * Registers callbacks with the Rust supervisor to update stores
 * on discrete events (process spawn, window events, etc.).
 * 
 * This complements the render loop sync by handling events that
 * occur outside the animation frame cycle.
 */

import type { Supervisor, DesktopController } from '../hooks/useSupervisor';

/**
 * Register supervisor callbacks to update stores on discrete events.
 * Call this once during app initialization.
 * 
 * @param supervisor - The Rust supervisor instance
 * @param desktop - The Rust desktop controller instance
 * @returns Cleanup function to unregister callbacks
 */
export function registerStoreCallbacks(
  supervisor: Supervisor,
  desktop: DesktopController
): () => void {
  // =========================================================================
  // Process Spawn Callback
  // =========================================================================
  
  // Register callback for when new processes are spawned
  supervisor.set_spawn_callback((procType: string, name: string) => {
    console.log(`[callbackSync] Process spawned: ${procType}/${name}`);
    // Process list updates are handled via IPC or the render loop
    // Could add a processStore if needed for process management
  });
  
  // =========================================================================
  // TODO: Future Callbacks (as Rust exposes them)
  // =========================================================================
  
  // When Rust exposes these callbacks, we can add:
  // 
  // Window Events:
  // - supervisor.set_window_created_callback((windowId, appId) => {
  //     // Update window store with new window
  //   });
  // 
  // - supervisor.set_window_closed_callback((windowId) => {
  //     // Remove window from store
  //   });
  // 
  // Desktop Events:
  // - supervisor.set_desktop_switched_callback((fromIndex, toIndex) => {
  //     // Update desktop store
  //   });
  // 
  // Identity Events:
  // - supervisor.set_user_logged_in_callback((userId, sessionId) => {
  //     // Update identity store
  //   });
  // 
  // - supervisor.set_session_expired_callback((sessionId) => {
  //     // Clear session from identity store
  //   });

  // Return cleanup function
  return () => {
    // Reset callbacks if needed
    // Note: Currently supervisor doesn't have a way to unset callbacks
    // This is a no-op placeholder for future implementation
    console.log('[callbackSync] Cleaning up store callbacks');
  };
}

/**
 * Type for a callback cleanup function
 */
export type CallbackCleanup = () => void;
