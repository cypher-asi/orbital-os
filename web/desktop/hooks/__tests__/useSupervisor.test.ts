import { describe, it, expect, vi } from 'vitest';
import { renderHook } from '@testing-library/react';
import { createElement } from 'react';
import {
  useSupervisor,
  useDesktopController,
  SupervisorProvider,
  DesktopControllerProvider,
} from '../useSupervisor';
import { createMockSupervisor, createMockDesktopController } from '../../../test/mocks';

describe('useSupervisor', () => {
  it('returns null when used outside provider', () => {
    const { result } = renderHook(() => useSupervisor());
    expect(result.current).toBeNull();
  });

  it('returns supervisor when used inside provider', () => {
    const mockSupervisor = createMockSupervisor();

    const wrapper = ({ children }: { children: React.ReactNode }) =>
      createElement(SupervisorProvider, { value: mockSupervisor }, children);

    const { result } = renderHook(() => useSupervisor(), { wrapper });

    expect(result.current).toBe(mockSupervisor);
  });

  it('provides working supervisor methods', () => {
    const mockSupervisor = createMockSupervisor();

    const wrapper = ({ children }: { children: React.ReactNode }) =>
      createElement(SupervisorProvider, { value: mockSupervisor }, children);

    const { result } = renderHook(() => useSupervisor(), { wrapper });

    expect(result.current?.boot).toBeDefined();
    expect(result.current?.get_process_count).toBeDefined();

    // Call methods
    result.current?.boot();
    expect(mockSupervisor.boot).toHaveBeenCalled();

    result.current?.get_process_count();
    expect(mockSupervisor.get_process_count).toHaveBeenCalled();
  });
});

describe('useDesktopController', () => {
  it('returns null when used outside provider', () => {
    const { result } = renderHook(() => useDesktopController());
    expect(result.current).toBeNull();
  });

  it('returns desktop controller when used inside provider', () => {
    const mockDesktop = createMockDesktopController();

    const wrapper = ({ children }: { children: React.ReactNode }) =>
      createElement(DesktopControllerProvider, { value: mockDesktop }, children);

    const { result } = renderHook(() => useDesktopController(), { wrapper });

    expect(result.current).toBe(mockDesktop);
  });

  it('provides working desktop controller methods', () => {
    const mockDesktop = createMockDesktopController();

    const wrapper = ({ children }: { children: React.ReactNode }) =>
      createElement(DesktopControllerProvider, { value: mockDesktop }, children);

    const { result } = renderHook(() => useDesktopController(), { wrapper });

    expect(result.current?.init).toBeDefined();
    expect(result.current?.create_window).toBeDefined();
    expect(result.current?.get_windows_json).toBeDefined();

    // Call init
    result.current?.init(1920, 1080);
    expect(mockDesktop.init).toHaveBeenCalledWith(1920, 1080);

    // Call create_window
    result.current?.create_window('Test', 100, 100, 800, 600, 'test-app', false);
    expect(mockDesktop.create_window).toHaveBeenCalledWith('Test', 100, 100, 800, 600, 'test-app', false);
  });
});

describe('nested providers', () => {
  it('supports both providers together', () => {
    const mockSupervisor = createMockSupervisor();
    const mockDesktop = createMockDesktopController();

    const wrapper = ({ children }: { children: React.ReactNode }) =>
      createElement(
        SupervisorProvider,
        { value: mockSupervisor },
        createElement(DesktopControllerProvider, { value: mockDesktop }, children)
      );

    const { result } = renderHook(
      () => ({
        supervisor: useSupervisor(),
        desktop: useDesktopController(),
      }),
      { wrapper }
    );

    expect(result.current.supervisor).toBe(mockSupervisor);
    expect(result.current.desktop).toBe(mockDesktop);
  });
});
