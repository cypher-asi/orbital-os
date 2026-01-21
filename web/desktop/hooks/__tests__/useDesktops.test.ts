import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook, act, waitFor } from '@testing-library/react';
import { createElement } from 'react';
import {
  useDesktops,
  useActiveDesktop,
  useDesktopActions,
  useViewMode,
  useIsInVoid,
  useVoidActions,
} from '../useDesktops';
import { DesktopControllerProvider } from '../useSupervisor';
import { createMockDesktopController } from '../../../test/mocks';

describe('useDesktops', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('returns empty array when no desktop controller', () => {
    const { result } = renderHook(() => useDesktops());
    expect(result.current).toEqual([]);
  });

  it('returns desktops from controller', async () => {
    const mockDesktop = createMockDesktopController({
      desktops: [
        { id: 1, name: 'Main', active: true, windowCount: 2 },
        { id: 2, name: 'Work', active: false, windowCount: 0 },
      ],
    });

    const wrapper = ({ children }: { children: React.ReactNode }) =>
      createElement(DesktopControllerProvider, { value: mockDesktop }, children);

    const { result } = renderHook(() => useDesktops(), { wrapper });

    act(() => {
      vi.advanceTimersByTime(200);
    });

    await waitFor(() => {
      expect(result.current.length).toBe(2);
    });

    expect(result.current[0].name).toBe('Main');
    expect(result.current[1].name).toBe('Work');
  });

  it('handles JSON parse errors gracefully', () => {
    const mockDesktop = createMockDesktopController();
    mockDesktop.get_desktops_json = vi.fn(() => 'invalid json');

    const wrapper = ({ children }: { children: React.ReactNode }) =>
      createElement(DesktopControllerProvider, { value: mockDesktop }, children);

    const consoleSpy = vi.spyOn(console, 'error').mockImplementation(() => {});

    const { result } = renderHook(() => useDesktops(), { wrapper });

    act(() => {
      vi.advanceTimersByTime(200);
    });

    expect(result.current).toEqual([]);

    consoleSpy.mockRestore();
  });
});

describe('useActiveDesktop', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('returns 0 when no desktop controller', () => {
    const { result } = renderHook(() => useActiveDesktop());
    expect(result.current).toBe(0);
  });

  it('returns active desktop index', async () => {
    const mockDesktop = createMockDesktopController({ activeDesktop: 2 });

    const wrapper = ({ children }: { children: React.ReactNode }) =>
      createElement(DesktopControllerProvider, { value: mockDesktop }, children);

    const { result } = renderHook(() => useActiveDesktop(), { wrapper });

    act(() => {
      vi.advanceTimersByTime(200);
    });

    await waitFor(() => {
      expect(result.current).toBe(2);
    });
  });
});

describe('useDesktopActions', () => {
  it('provides createDesktop action', () => {
    const mockDesktop = createMockDesktopController();

    const wrapper = ({ children }: { children: React.ReactNode }) =>
      createElement(DesktopControllerProvider, { value: mockDesktop }, children);

    const { result } = renderHook(() => useDesktopActions(), { wrapper });

    result.current.createDesktop('New Desktop');

    expect(mockDesktop.create_desktop).toHaveBeenCalledWith('New Desktop');
  });

  it('provides switchDesktop action', () => {
    const mockDesktop = createMockDesktopController({
      desktops: [
        { id: 1, name: 'Main', active: true, windowCount: 0 },
        { id: 2, name: 'Second', active: false, windowCount: 0 },
      ],
    });

    const wrapper = ({ children }: { children: React.ReactNode }) =>
      createElement(DesktopControllerProvider, { value: mockDesktop }, children);

    const { result } = renderHook(() => useDesktopActions(), { wrapper });

    result.current.switchDesktop(1);

    expect(mockDesktop.switch_desktop).toHaveBeenCalledWith(1);
  });

  it('returns null for createDesktop when no controller', () => {
    const { result } = renderHook(() => useDesktopActions());

    expect(result.current.createDesktop('Test')).toBeNull();
  });
});

describe('useViewMode', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('returns desktop by default', () => {
    const { result } = renderHook(() => useViewMode());
    expect(result.current).toBe('desktop');
  });

  it('returns current view mode from controller', async () => {
    const mockDesktop = createMockDesktopController({ viewMode: 'void' });

    const wrapper = ({ children }: { children: React.ReactNode }) =>
      createElement(DesktopControllerProvider, { value: mockDesktop }, children);

    const { result } = renderHook(() => useViewMode(), { wrapper });

    act(() => {
      vi.advanceTimersByTime(100);
    });

    await waitFor(() => {
      expect(result.current).toBe('void');
    });
  });

  it('maps workspace to desktop for backward compatibility', async () => {
    const mockDesktop = createMockDesktopController();
    mockDesktop.get_view_mode = vi.fn(() => 'workspace');

    const wrapper = ({ children }: { children: React.ReactNode }) =>
      createElement(DesktopControllerProvider, { value: mockDesktop }, children);

    const { result } = renderHook(() => useViewMode(), { wrapper });

    act(() => {
      vi.advanceTimersByTime(100);
    });

    await waitFor(() => {
      expect(result.current).toBe('desktop');
    });
  });
});

describe('useIsInVoid', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('returns false by default', () => {
    const { result } = renderHook(() => useIsInVoid());
    expect(result.current).toBe(false);
  });

  it('returns true when in void mode', async () => {
    const mockDesktop = createMockDesktopController({ viewMode: 'void' });

    const wrapper = ({ children }: { children: React.ReactNode }) =>
      createElement(DesktopControllerProvider, { value: mockDesktop }, children);

    const { result } = renderHook(() => useIsInVoid(), { wrapper });

    act(() => {
      vi.advanceTimersByTime(100);
    });

    await waitFor(() => {
      expect(result.current).toBe(true);
    });
  });
});

describe('useVoidActions', () => {
  it('provides enterVoid action', () => {
    const mockDesktop = createMockDesktopController();

    const wrapper = ({ children }: { children: React.ReactNode }) =>
      createElement(DesktopControllerProvider, { value: mockDesktop }, children);

    const { result } = renderHook(() => useVoidActions(), { wrapper });

    result.current.enterVoid();

    expect(mockDesktop.enter_void).toHaveBeenCalled();
  });

  it('provides exitVoid action', () => {
    const mockDesktop = createMockDesktopController({ viewMode: 'void' });

    const wrapper = ({ children }: { children: React.ReactNode }) =>
      createElement(DesktopControllerProvider, { value: mockDesktop }, children);

    const { result } = renderHook(() => useVoidActions(), { wrapper });

    result.current.exitVoid(1);

    expect(mockDesktop.exit_void).toHaveBeenCalledWith(1);
  });

  it('handles missing controller gracefully', () => {
    const { result } = renderHook(() => useVoidActions());

    // Should not throw
    expect(() => result.current.enterVoid()).not.toThrow();
    expect(() => result.current.exitVoid(0)).not.toThrow();
  });
});
