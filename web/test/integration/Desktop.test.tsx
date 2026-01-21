import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, act, waitFor } from '@testing-library/react';
import { createElement } from 'react';
import { Desktop } from '../../components/Desktop/Desktop';
import {
  createMockDesktopController,
  createMockDesktopControllerWithWindows,
  createMockSupervisor,
} from '../mocks';

// Mock the @cypher-asi/zui components
vi.mock('@cypher-asi/zui', () => ({
  Panel: ({ children, className, style, onPointerDown, ...props }: any) =>
    createElement('div', { className, style, onPointerDown, ...props }, children),
  ButtonWindow: ({ action, onClick }: any) =>
    createElement('button', { onClick, 'data-testid': `btn-${action}` }, action),
  Menu: ({ title, items, value, onChange, ...props }: any) =>
    createElement(
      'div',
      { 'data-testid': 'background-menu', ...props },
      createElement('div', null, title),
      items.map((item: any) =>
        createElement(
          'button',
          {
            key: item.id,
            onClick: () => onChange(item.id),
            'data-selected': item.id === value,
          },
          item.label
        )
      )
    ),
}));

// Mock lucide-react icons
vi.mock('lucide-react', () => ({
  TerminalSquare: () => createElement('span', { 'data-testid': 'icon-terminal' }, 'T'),
  Settings: () => createElement('span', { 'data-testid': 'icon-settings' }, 'S'),
  Folder: () => createElement('span', { 'data-testid': 'icon-folder' }, 'F'),
  Power: () => createElement('span', { 'data-testid': 'icon-power' }, 'P'),
  AppWindow: () => createElement('span', { 'data-testid': 'icon-window' }, 'W'),
  Circle: () => createElement('span', { 'data-testid': 'icon-circle' }, 'O'),
  Plus: () => createElement('span', { 'data-testid': 'icon-plus' }, '+'),
  KeyRound: () => createElement('span', { 'data-testid': 'icon-key' }, 'K'),
  CreditCard: () => createElement('span', { 'data-testid': 'icon-card' }, 'C'),
}));

// Mock the WASM module import
vi.mock('../../pkg/orbital_web.js', () => ({
  DesktopBackground: vi.fn().mockImplementation(() => ({
    init: vi.fn().mockResolvedValue(undefined),
    is_initialized: vi.fn().mockReturnValue(true),
    resize: vi.fn(),
    render: vi.fn(),
    get_available_backgrounds: vi.fn().mockReturnValue(JSON.stringify([
      { id: 'grain', name: 'Film Grain' },
      { id: 'mist', name: 'Misty Smoke' },
    ])),
    get_current_background: vi.fn().mockReturnValue('grain'),
    set_background: vi.fn().mockReturnValue(true),
    set_viewport: vi.fn(),
    set_workspace_info: vi.fn(),
    set_transitioning: vi.fn(),
    set_workspace_dimensions: vi.fn(),
  })),
}));

describe('Desktop Integration', () => {
  let mockDesktop: ReturnType<typeof createMockDesktopController>;
  let mockSupervisor: ReturnType<typeof createMockSupervisor>;

  beforeEach(() => {
    vi.useFakeTimers();
    mockDesktop = createMockDesktopController();
    mockSupervisor = createMockSupervisor();

    // Mock getBoundingClientRect for container
    Element.prototype.getBoundingClientRect = vi.fn().mockReturnValue({
      width: 1920,
      height: 1080,
      top: 0,
      left: 0,
      right: 1920,
      bottom: 1080,
      x: 0,
      y: 0,
      toJSON: () => ({}),
    });
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it('initializes desktop with correct dimensions', async () => {
    render(createElement(Desktop, { supervisor: mockSupervisor, desktop: mockDesktop }));

    await waitFor(() => {
      expect(mockDesktop.init).toHaveBeenCalledWith(1920, 1080);
    });
  });

  it('launches terminal on initialization', async () => {
    render(createElement(Desktop, { supervisor: mockSupervisor, desktop: mockDesktop }));

    await waitFor(() => {
      expect(mockDesktop.launch_app).toHaveBeenCalledWith('terminal');
    });
  });

  it('renders taskbar', async () => {
    render(createElement(Desktop, { supervisor: mockSupervisor, desktop: mockDesktop }));

    await waitFor(() => {
      expect(screen.getByTitle('Begin Menu (Press Z)')).toBeInTheDocument();
    });
  });

  it('handles resize events', async () => {
    render(createElement(Desktop, { supervisor: mockSupervisor, desktop: mockDesktop }));

    await waitFor(() => {
      expect(mockDesktop.init).toHaveBeenCalled();
    });

    // Simulate resize
    act(() => {
      window.dispatchEvent(new Event('resize'));
    });

    expect(mockDesktop.resize).toHaveBeenCalledWith(1920, 1080);
  });

  it('forwards pointer events to desktop controller', async () => {
    const { container } = render(
      createElement(Desktop, { supervisor: mockSupervisor, desktop: mockDesktop })
    );

    await waitFor(() => {
      expect(mockDesktop.init).toHaveBeenCalled();
    });

    const desktopElement = container.firstChild as HTMLElement;

    // Simulate pointer down
    fireEvent.pointerDown(desktopElement, {
      clientX: 500,
      clientY: 500,
      button: 0,
      ctrlKey: false,
      shiftKey: false,
    });

    expect(mockDesktop.pointer_down).toHaveBeenCalledWith(500, 500, 0, false, false);
  });

  it('handles wheel events with ctrl for zoom', async () => {
    const { container } = render(
      createElement(Desktop, { supervisor: mockSupervisor, desktop: mockDesktop })
    );

    await waitFor(() => {
      expect(mockDesktop.init).toHaveBeenCalled();
    });

    const desktopElement = container.firstChild as HTMLElement;

    // Simulate ctrl+wheel for zoom
    fireEvent.wheel(desktopElement, {
      deltaX: 0,
      deltaY: -100,
      clientX: 960,
      clientY: 540,
      ctrlKey: true,
    });

    expect(mockDesktop.wheel).toHaveBeenCalledWith(0, -100, 960, 540, true);
  });

  it('handles keyboard shortcuts for void mode', async () => {
    mockDesktop.get_view_mode = vi.fn().mockReturnValue('desktop');

    render(createElement(Desktop, { supervisor: mockSupervisor, desktop: mockDesktop }));

    await waitFor(() => {
      expect(mockDesktop.init).toHaveBeenCalled();
    });

    // Press F3 to enter void
    fireEvent.keyDown(window, { key: 'F3' });

    expect(mockDesktop.enter_void).toHaveBeenCalled();
  });

  it('handles keyboard shortcuts for new terminal', async () => {
    render(createElement(Desktop, { supervisor: mockSupervisor, desktop: mockDesktop }));

    await waitFor(() => {
      expect(mockDesktop.init).toHaveBeenCalled();
    });

    // Press T to create new terminal
    fireEvent.keyDown(window, { key: 't' });

    // launch_app called once on init, once on T press
    expect(mockDesktop.launch_app).toHaveBeenCalledWith('terminal');
    expect(mockDesktop.launch_app).toHaveBeenCalledTimes(2);
  });

  it('handles keyboard shortcuts for close window', async () => {
    mockDesktop = createMockDesktopControllerWithWindows([
      { id: 1, title: 'Test Window', focused: true },
    ]);

    render(createElement(Desktop, { supervisor: mockSupervisor, desktop: mockDesktop }));

    await waitFor(() => {
      expect(mockDesktop.init).toHaveBeenCalled();
    });

    // Press C to close focused window
    fireEvent.keyDown(window, { key: 'c' });

    expect(mockDesktop.close_window).toHaveBeenCalledWith(BigInt(1));
  });

  it('handles arrow keys for desktop switching', async () => {
    mockDesktop = createMockDesktopController({
      desktops: [
        { id: 1, name: 'Main', active: true, windowCount: 0 },
        { id: 2, name: 'Second', active: false, windowCount: 0 },
      ],
    });

    render(createElement(Desktop, { supervisor: mockSupervisor, desktop: mockDesktop }));

    await waitFor(() => {
      expect(mockDesktop.init).toHaveBeenCalled();
    });

    // Press Ctrl+ArrowRight to switch to next desktop
    fireEvent.keyDown(window, { key: 'ArrowRight', ctrlKey: true });

    expect(mockDesktop.switch_desktop).toHaveBeenCalledWith(1);
  });

  it('handles context menu for background selection', async () => {
    const { container } = render(
      createElement(Desktop, { supervisor: mockSupervisor, desktop: mockDesktop })
    );

    await waitFor(() => {
      expect(mockDesktop.init).toHaveBeenCalled();
    });

    const desktopElement = container.firstChild as HTMLElement;

    // Right-click on desktop
    fireEvent.contextMenu(desktopElement, {
      clientX: 500,
      clientY: 500,
    });

    // Background menu should appear (once background is ready)
    // Note: In the real component, this depends on background initialization
  });

  it('global pointer move updates desktop controller', async () => {
    render(createElement(Desktop, { supervisor: mockSupervisor, desktop: mockDesktop }));

    await waitFor(() => {
      expect(mockDesktop.init).toHaveBeenCalled();
    });

    // Simulate global pointer move
    act(() => {
      window.dispatchEvent(
        new PointerEvent('pointermove', {
          clientX: 600,
          clientY: 400,
        })
      );
    });

    expect(mockDesktop.pointer_move).toHaveBeenCalledWith(600, 400);
  });

  it('global pointer up ends drag', async () => {
    render(createElement(Desktop, { supervisor: mockSupervisor, desktop: mockDesktop }));

    await waitFor(() => {
      expect(mockDesktop.init).toHaveBeenCalled();
    });

    // Simulate global pointer up
    act(() => {
      window.dispatchEvent(new PointerEvent('pointerup'));
    });

    expect(mockDesktop.pointer_up).toHaveBeenCalled();
  });

  it('prevents browser zoom on ctrl+scroll', async () => {
    render(createElement(Desktop, { supervisor: mockSupervisor, desktop: mockDesktop }));

    await waitFor(() => {
      expect(mockDesktop.init).toHaveBeenCalled();
    });

    const preventDefault = vi.fn();
    const wheelEvent = new WheelEvent('wheel', {
      deltaY: -100,
      ctrlKey: true,
      cancelable: true,
    });
    Object.defineProperty(wheelEvent, 'preventDefault', { value: preventDefault });

    // The capture listener should prevent default
    act(() => {
      window.dispatchEvent(wheelEvent);
    });

    expect(preventDefault).toHaveBeenCalled();
  });
});

describe('Desktop with Windows', () => {
  beforeEach(() => {
    vi.useFakeTimers();

    Element.prototype.getBoundingClientRect = vi.fn().mockReturnValue({
      width: 1920,
      height: 1080,
      top: 0,
      left: 0,
      right: 1920,
      bottom: 1080,
      x: 0,
      y: 0,
      toJSON: () => ({}),
    });
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it('renders windows from tick_frame data', async () => {
    const mockDesktop = createMockDesktopControllerWithWindows([
      { id: 1, title: 'Window 1', appId: 'test1' },
      { id: 2, title: 'Window 2', appId: 'test2' },
    ]);
    const mockSupervisor = createMockSupervisor();

    render(createElement(Desktop, { supervisor: mockSupervisor, desktop: mockDesktop }));

    await waitFor(() => {
      expect(mockDesktop.init).toHaveBeenCalled();
    });

    // Advance timer to trigger render loop
    act(() => {
      vi.advanceTimersByTime(100);
    });

    // Windows should be rendered (their content comes from AppRouter)
    // In real app, tick_frame provides window data for rendering
    expect(mockDesktop.tick_frame).toHaveBeenCalled();
  });

  it('handles window focus through taskbar', async () => {
    const mockDesktop = createMockDesktopControllerWithWindows([
      { id: 1, title: 'Window 1', focused: false },
      { id: 2, title: 'Window 2', focused: true },
    ]);
    const mockSupervisor = createMockSupervisor();

    render(createElement(Desktop, { supervisor: mockSupervisor, desktop: mockDesktop }));

    await waitFor(() => {
      expect(mockDesktop.init).toHaveBeenCalled();
    });

    act(() => {
      vi.advanceTimersByTime(200);
    });

    await waitFor(() => {
      const window1Button = screen.getByTitle('Window 1');
      expect(window1Button).toBeInTheDocument();
    });

    // Click on Window 1 in taskbar
    const window1Button = screen.getByTitle('Window 1');
    fireEvent.click(window1Button);

    expect(mockDesktop.focus_window).toHaveBeenCalledWith(BigInt(1));
  });
});

describe('Desktop Selection Box', () => {
  beforeEach(() => {
    vi.useFakeTimers();

    Element.prototype.getBoundingClientRect = vi.fn().mockReturnValue({
      width: 1920,
      height: 1080,
      top: 0,
      left: 0,
      right: 1920,
      bottom: 1080,
      x: 0,
      y: 0,
      toJSON: () => ({}),
    });
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it('shows selection box on drag on empty desktop', async () => {
    const mockDesktop = createMockDesktopController();
    mockDesktop.pointer_down = vi.fn().mockReturnValue(JSON.stringify({ type: 'unhandled' }));

    const mockSupervisor = createMockSupervisor();

    const { container } = render(
      createElement(Desktop, { supervisor: mockSupervisor, desktop: mockDesktop })
    );

    await waitFor(() => {
      expect(mockDesktop.init).toHaveBeenCalled();
    });

    const desktopElement = container.firstChild as HTMLElement;

    // Start selection
    fireEvent.pointerDown(desktopElement, {
      clientX: 100,
      clientY: 100,
      button: 0,
      ctrlKey: false,
      shiftKey: false,
      target: desktopElement,
    });

    // Move to create selection box
    fireEvent.pointerMove(desktopElement, {
      clientX: 300,
      clientY: 300,
    });

    // Selection box should appear
    const selectionBox = container.querySelector('[class*="selectionBox"]');
    // Note: Selection box visibility depends on minimum size (width > 2 && height > 2)
  });

  it('clears selection box on pointer up', async () => {
    const mockDesktop = createMockDesktopController();
    mockDesktop.pointer_down = vi.fn().mockReturnValue(JSON.stringify({ type: 'unhandled' }));

    const mockSupervisor = createMockSupervisor();

    const { container } = render(
      createElement(Desktop, { supervisor: mockSupervisor, desktop: mockDesktop })
    );

    await waitFor(() => {
      expect(mockDesktop.init).toHaveBeenCalled();
    });

    const desktopElement = container.firstChild as HTMLElement;

    // Start and end selection
    fireEvent.pointerDown(desktopElement, {
      clientX: 100,
      clientY: 100,
      button: 0,
      ctrlKey: false,
      shiftKey: false,
      target: desktopElement,
    });

    fireEvent.pointerUp(desktopElement);

    // Selection box should be gone
    const selectionBox = container.querySelector('[class*="selectionBox"]');
    expect(selectionBox).not.toBeInTheDocument();
  });
});
