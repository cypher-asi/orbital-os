import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, act, waitFor } from '@testing-library/react';
import { createElement } from 'react';
import { Taskbar } from './Taskbar';
import {
  DesktopControllerProvider,
  SupervisorProvider,
} from '../../../desktop/hooks/useSupervisor';
import {
  createMockDesktopController,
  createMockDesktopControllerWithWindows,
  createMockSupervisor,
} from '../../../test/mocks';

// Mock the @cypher-asi/zui components
vi.mock('@cypher-asi/zui', () => ({
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  Panel: ({ children, className, ...props }: Record<string, any>) =>
    createElement('div', { className, ...props }, children),
}));

// Mock lucide-react icons
vi.mock('lucide-react', () => ({
  TerminalSquare: () => createElement('span', { 'data-testid': 'icon-terminal' }, 'T'),
  AppWindow: () => createElement('span', { 'data-testid': 'icon-window' }, 'W'),
  Circle: () => createElement('span', { 'data-testid': 'icon-circle' }, 'O'),
  Plus: () => createElement('span', { 'data-testid': 'icon-plus' }, '+'),
  KeyRound: () => createElement('span', { 'data-testid': 'icon-key' }, 'K'),
  CreditCard: () => createElement('span', { 'data-testid': 'icon-card' }, 'C'),
}));

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function createTestWrapper(mockDesktop: any, mockSupervisor?: any) {
  const supervisor = mockSupervisor || createMockSupervisor();
  return function Wrapper({ children }: { children: React.ReactNode }) {
    return createElement(
      SupervisorProvider,
      { value: supervisor },
      createElement(DesktopControllerProvider, { value: mockDesktop }, children)
    );
  };
}

describe('Taskbar', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('renders taskbar structure', () => {
    const mockDesktop = createMockDesktopController();
    const { container } = render(createElement(Taskbar), {
      wrapper: createTestWrapper(mockDesktop),
    });

    // Should have the main taskbar element
    expect(container.querySelector('[class*="taskbar"]')).toBeInTheDocument();
  });

  it('renders begin button', () => {
    const mockDesktop = createMockDesktopController();
    render(createElement(Taskbar), {
      wrapper: createTestWrapper(mockDesktop),
    });

    const beginButton = screen.getByTitle('Begin Menu (Press Z)');
    expect(beginButton).toBeInTheDocument();
  });

  it('renders window buttons when windows exist', async () => {
    const mockDesktop = createMockDesktopControllerWithWindows([
      { id: 1, title: 'Terminal 1', appId: 'terminal' },
      { id: 2, title: 'Terminal 2', appId: 'terminal' },
    ]);

    render(createElement(Taskbar), {
      wrapper: createTestWrapper(mockDesktop),
    });

    // Advance timer for hook updates
    act(() => {
      vi.advanceTimersByTime(200);
    });

    await waitFor(() => {
      expect(screen.getByTitle('Terminal 1')).toBeInTheDocument();
      expect(screen.getByTitle('Terminal 2')).toBeInTheDocument();
    });
  });

  it('renders desktop indicators', async () => {
    const mockDesktop = createMockDesktopController({
      desktops: [
        { id: 1, name: 'Main', active: true, windowCount: 0 },
        { id: 2, name: 'Work', active: false, windowCount: 0 },
      ],
    });

    render(createElement(Taskbar), {
      wrapper: createTestWrapper(mockDesktop),
    });

    act(() => {
      vi.advanceTimersByTime(200);
    });

    await waitFor(() => {
      expect(screen.getByTitle('Main')).toBeInTheDocument();
      expect(screen.getByTitle('Work')).toBeInTheDocument();
    });
  });

  it('focuses window on click', async () => {
    const mockDesktop = createMockDesktopControllerWithWindows([
      { id: 1, title: 'Test Window', appId: 'test', focused: false },
    ]);

    render(createElement(Taskbar), {
      wrapper: createTestWrapper(mockDesktop),
    });

    act(() => {
      vi.advanceTimersByTime(200);
    });

    await waitFor(() => {
      expect(screen.getByTitle('Test Window')).toBeInTheDocument();
    });

    const windowButton = screen.getByTitle('Test Window');
    fireEvent.click(windowButton);

    expect(mockDesktop.focus_window).toHaveBeenCalledWith(BigInt(1));
    expect(mockDesktop.pan_to_window).toHaveBeenCalledWith(BigInt(1));
  });

  it('restores minimized window on click', async () => {
    const mockDesktop = createMockDesktopControllerWithWindows([
      { id: 1, title: 'Minimized Window', appId: 'test', state: 'minimized' },
    ]);

    render(createElement(Taskbar), {
      wrapper: createTestWrapper(mockDesktop),
    });

    act(() => {
      vi.advanceTimersByTime(200);
    });

    await waitFor(() => {
      expect(screen.getByTitle('Minimized Window')).toBeInTheDocument();
    });

    const windowButton = screen.getByTitle('Minimized Window');
    fireEvent.click(windowButton);

    expect(mockDesktop.restore_window).toHaveBeenCalledWith(BigInt(1));
  });

  it('switches desktop on indicator click', async () => {
    const mockDesktop = createMockDesktopController({
      desktops: [
        { id: 1, name: 'Main', active: true, windowCount: 0 },
        { id: 2, name: 'Second', active: false, windowCount: 0 },
      ],
    });

    render(createElement(Taskbar), {
      wrapper: createTestWrapper(mockDesktop),
    });

    act(() => {
      vi.advanceTimersByTime(200);
    });

    await waitFor(() => {
      expect(screen.getByTitle('Second')).toBeInTheDocument();
    });

    const secondDesktopButton = screen.getByTitle('Second');
    fireEvent.click(secondDesktopButton);

    expect(mockDesktop.switch_desktop).toHaveBeenCalledWith(1);
  });

  it('creates new desktop on add button click', async () => {
    const mockDesktop = createMockDesktopController({
      desktops: [{ id: 1, name: 'Main', active: true, windowCount: 0 }],
    });

    render(createElement(Taskbar), {
      wrapper: createTestWrapper(mockDesktop),
    });

    act(() => {
      vi.advanceTimersByTime(200);
    });

    const addButton = screen.getByTitle('Add desktop');
    fireEvent.click(addButton);

    expect(mockDesktop.create_desktop).toHaveBeenCalledWith('Desktop 2');
  });

  it('toggles begin menu on button click', () => {
    const mockDesktop = createMockDesktopController();

    render(createElement(Taskbar), {
      wrapper: createTestWrapper(mockDesktop),
    });

    const beginButton = screen.getByTitle('Begin Menu (Press Z)');

    // Initially closed
    expect(screen.queryByText('ZERO OS')).not.toBeInTheDocument();

    // Open
    fireEvent.click(beginButton);
    expect(screen.getByText('ZERO OS')).toBeInTheDocument();

    // Close
    fireEvent.click(beginButton);
    expect(screen.queryByText('ZERO OS')).not.toBeInTheDocument();
  });

  it('toggles begin menu on Z key press', () => {
    const mockDesktop = createMockDesktopController();

    render(createElement(Taskbar), {
      wrapper: createTestWrapper(mockDesktop),
    });

    // Initially closed
    expect(screen.queryByText('ZERO OS')).not.toBeInTheDocument();

    // Press Z to open
    fireEvent.keyDown(window, { key: 'z' });
    expect(screen.getByText('ZERO OS')).toBeInTheDocument();

    // Press Z to close
    fireEvent.keyDown(window, { key: 'z' });
    expect(screen.queryByText('ZERO OS')).not.toBeInTheDocument();
  });

  it('does not toggle begin menu when Z is pressed in input', () => {
    const mockDesktop = createMockDesktopController();

    render(
      createElement(
        'div',
        null,
        createElement('input', { 'data-testid': 'test-input' }),
        createElement(Taskbar)
      ),
      {
        wrapper: createTestWrapper(mockDesktop),
      }
    );

    const input = screen.getByTestId('test-input');
    input.focus();

    // Press Z while input is focused
    fireEvent.keyDown(input, { key: 'z' });

    // Menu should remain closed
    expect(screen.queryByText('ZERO OS')).not.toBeInTheDocument();
  });

  it('does not open begin menu when modifier keys are pressed with Z', () => {
    const mockDesktop = createMockDesktopController();

    render(createElement(Taskbar), {
      wrapper: createTestWrapper(mockDesktop),
    });

    // Press Ctrl+Z
    fireEvent.keyDown(window, { key: 'z', ctrlKey: true });
    expect(screen.queryByText('ZERO OS')).not.toBeInTheDocument();

    // Press Shift+Z
    fireEvent.keyDown(window, { key: 'z', shiftKey: true });
    expect(screen.queryByText('ZERO OS')).not.toBeInTheDocument();

    // Press Alt+Z
    fireEvent.keyDown(window, { key: 'z', altKey: true });
    expect(screen.queryByText('ZERO OS')).not.toBeInTheDocument();
  });
});
