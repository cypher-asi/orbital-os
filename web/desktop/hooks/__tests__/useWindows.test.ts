import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook, act, waitFor } from '@testing-library/react';
import { createElement } from 'react';
import { useWindows, useFocusedWindow, useWindowActions } from '../useWindows';
import { DesktopControllerProvider, SupervisorProvider } from '../useSupervisor';
import { createMockDesktopController, createMockDesktopControllerWithWindows, createMockSupervisor } from '../../../test/mocks';

describe('useWindows', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('returns empty array when no desktop controller', () => {
    const { result } = renderHook(() => useWindows());
    expect(result.current).toEqual([]);
  });

  it('returns empty array initially', () => {
    const mockDesktop = createMockDesktopController();

    const wrapper = ({ children }: { children: React.ReactNode }) =>
      createElement(DesktopControllerProvider, { value: mockDesktop }, children);

    const { result } = renderHook(() => useWindows(), { wrapper });

    expect(result.current).toEqual([]);
  });

  it('returns windows from desktop controller', async () => {
    const mockDesktop = createMockDesktopControllerWithWindows([
      { id: 1, title: 'Window 1', appId: 'app1' },
      { id: 2, title: 'Window 2', appId: 'app2' },
    ]);

    const wrapper = ({ children }: { children: React.ReactNode }) =>
      createElement(DesktopControllerProvider, { value: mockDesktop }, children);

    const { result } = renderHook(() => useWindows(), { wrapper });

    // Advance timer to trigger update
    act(() => {
      vi.advanceTimersByTime(100);
    });

    await waitFor(() => {
      expect(result.current.length).toBe(2);
    });

    expect(result.current[0].title).toBe('Window 1');
    expect(result.current[1].title).toBe('Window 2');
  });

  it('handles JSON parse errors gracefully', () => {
    const mockDesktop = createMockDesktopController();
    mockDesktop.get_windows_json = vi.fn(() => 'invalid json');

    const wrapper = ({ children }: { children: React.ReactNode }) =>
      createElement(DesktopControllerProvider, { value: mockDesktop }, children);

    const consoleSpy = vi.spyOn(console, 'error').mockImplementation(() => {});

    const { result } = renderHook(() => useWindows(), { wrapper });

    act(() => {
      vi.advanceTimersByTime(100);
    });

    // Should not throw, should return previous state (empty array)
    expect(result.current).toEqual([]);

    consoleSpy.mockRestore();
  });
});

describe('useFocusedWindow', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('returns null when no desktop controller', () => {
    const { result } = renderHook(() => useFocusedWindow());
    expect(result.current).toBeNull();
  });

  it('returns null when no window is focused', () => {
    const mockDesktop = createMockDesktopController();
    mockDesktop.get_focused_window = vi.fn(() => undefined);

    const wrapper = ({ children }: { children: React.ReactNode }) =>
      createElement(DesktopControllerProvider, { value: mockDesktop }, children);

    const { result } = renderHook(() => useFocusedWindow(), { wrapper });

    act(() => {
      vi.advanceTimersByTime(100);
    });

    expect(result.current).toBeNull();
  });

  it('returns focused window ID', async () => {
    const mockDesktop = createMockDesktopControllerWithWindows([
      { id: 1, title: 'Window 1', focused: false },
      { id: 2, title: 'Window 2', focused: true },
    ]);

    const wrapper = ({ children }: { children: React.ReactNode }) =>
      createElement(DesktopControllerProvider, { value: mockDesktop }, children);

    const { result } = renderHook(() => useFocusedWindow(), { wrapper });

    act(() => {
      vi.advanceTimersByTime(100);
    });

    await waitFor(() => {
      expect(result.current).toBe(2);
    });
  });
});

describe('useWindowActions', () => {
  it('returns null functions when no desktop controller', () => {
    const { result } = renderHook(() => useWindowActions());

    expect(result.current.createWindow('Test', 0, 0, 100, 100, 'test', false)).toBeNull();
  });

  it('provides createWindow action', () => {
    const mockDesktop = createMockDesktopController();
    const mockSupervisor = createMockSupervisor();

    const wrapper = ({ children }: { children: React.ReactNode }) =>
      createElement(
        SupervisorProvider,
        { value: mockSupervisor },
        createElement(DesktopControllerProvider, { value: mockDesktop }, children)
      );

    const { result } = renderHook(() => useWindowActions(), { wrapper });

    const windowId = result.current.createWindow('Test Window', 100, 100, 800, 600, 'test-app', false);

    expect(mockDesktop.create_window).toHaveBeenCalledWith('Test Window', 100, 100, 800, 600, 'test-app', false);
    expect(windowId).toBeDefined();
  });

  it('provides closeWindow action', () => {
    const mockDesktop = createMockDesktopControllerWithWindows([{ id: 1, title: 'Test' }]);
    const mockSupervisor = createMockSupervisor();

    const wrapper = ({ children }: { children: React.ReactNode }) =>
      createElement(
        SupervisorProvider,
        { value: mockSupervisor },
        createElement(DesktopControllerProvider, { value: mockDesktop }, children)
      );

    const { result } = renderHook(() => useWindowActions(), { wrapper });

    result.current.closeWindow(1);

    expect(mockDesktop.close_window).toHaveBeenCalledWith(BigInt(1));
  });

  it('provides focusWindow action', () => {
    const mockDesktop = createMockDesktopControllerWithWindows([
      { id: 1, title: 'Window 1' },
      { id: 2, title: 'Window 2' },
    ]);

    const wrapper = ({ children }: { children: React.ReactNode }) =>
      createElement(DesktopControllerProvider, { value: mockDesktop }, children);

    const { result } = renderHook(() => useWindowActions(), { wrapper });

    result.current.focusWindow(1);

    expect(mockDesktop.focus_window).toHaveBeenCalledWith(BigInt(1));
  });

  it('provides minimizeWindow action', () => {
    const mockDesktop = createMockDesktopControllerWithWindows([{ id: 1, title: 'Test' }]);

    const wrapper = ({ children }: { children: React.ReactNode }) =>
      createElement(DesktopControllerProvider, { value: mockDesktop }, children);

    const { result } = renderHook(() => useWindowActions(), { wrapper });

    result.current.minimizeWindow(1);

    expect(mockDesktop.minimize_window).toHaveBeenCalledWith(BigInt(1));
  });

  it('provides maximizeWindow action', () => {
    const mockDesktop = createMockDesktopControllerWithWindows([{ id: 1, title: 'Test' }]);

    const wrapper = ({ children }: { children: React.ReactNode }) =>
      createElement(DesktopControllerProvider, { value: mockDesktop }, children);

    const { result } = renderHook(() => useWindowActions(), { wrapper });

    result.current.maximizeWindow(1);

    expect(mockDesktop.maximize_window).toHaveBeenCalledWith(BigInt(1));
  });

  it('provides restoreWindow action', () => {
    const mockDesktop = createMockDesktopControllerWithWindows([{ id: 1, title: 'Test', state: 'minimized' }]);

    const wrapper = ({ children }: { children: React.ReactNode }) =>
      createElement(DesktopControllerProvider, { value: mockDesktop }, children);

    const { result } = renderHook(() => useWindowActions(), { wrapper });

    result.current.restoreWindow(1);

    expect(mockDesktop.restore_window).toHaveBeenCalledWith(BigInt(1));
  });

  it('provides launchApp action', () => {
    const mockDesktop = createMockDesktopController();

    const wrapper = ({ children }: { children: React.ReactNode }) =>
      createElement(DesktopControllerProvider, { value: mockDesktop }, children);

    const { result } = renderHook(() => useWindowActions(), { wrapper });

    const windowId = result.current.launchApp('terminal');

    expect(mockDesktop.launch_app).toHaveBeenCalledWith('terminal');
    expect(windowId).toBeDefined();
  });

  it('provides panToWindow action', () => {
    const mockDesktop = createMockDesktopControllerWithWindows([{ id: 1, title: 'Test' }]);

    const wrapper = ({ children }: { children: React.ReactNode }) =>
      createElement(DesktopControllerProvider, { value: mockDesktop }, children);

    const { result } = renderHook(() => useWindowActions(), { wrapper });

    result.current.panToWindow(1);

    expect(mockDesktop.pan_to_window).toHaveBeenCalledWith(BigInt(1));
  });
});
