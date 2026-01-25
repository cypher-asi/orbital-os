import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { createElement } from 'react';
import { BeginMenu } from './BeginMenu';
import {
  DesktopControllerProvider,
  SupervisorProvider,
} from '../../../../desktop/hooks/useSupervisor';
import { createMockDesktopController, createMockSupervisor } from '../../../../test/mocks';

// Track the onChange callback for testing
let _capturedOnChange: ((id: string) => void) | null = null;

let capturedItems: Record<string, unknown>[] = [];

// Mock the @cypher-asi/zui components
vi.mock('@cypher-asi/zui', () => ({
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  Menu: ({ items, onChange, title, ...props }: Record<string, any>) => {
    // Capture the onChange and items for testing
    _capturedOnChange = onChange;
    capturedItems = items;

    // Render a simple representation of the menu for testing
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const renderItems = (menuItems: Record<string, any>[]): any[] => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      return menuItems.map((item: Record<string, any>, index: number) => {
        // Handle separator
        if (item.type === 'separator') {
          return createElement('hr', {
            key: `separator-${index}`,
            'data-testid': 'menu-separator',
          });
        }
        if (item.children) {
          // Render submenu header and children
          return createElement('div', { key: item.id, 'data-testid': `submenu-${item.id}` }, [
            createElement('span', { key: `${item.id}-label` }, item.label),
            ...renderItems(item.children),
          ]);
        }
        return createElement(
          'div',
          {
            key: item.id,
            'data-testid': `menu-item-${item.id}`,
            onClick: () => onChange?.(item.id),
          },
          item.label
        );
      });
    };

    return createElement('div', { 'data-testid': 'menu', ...props }, [
      title && createElement('div', { key: 'title', 'data-testid': 'menu-title' }, title),
      ...renderItems(items),
    ]);
  },
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

describe('BeginMenu', () => {
  let mockDesktop: ReturnType<typeof createMockDesktopController>;
  let mockSupervisor: ReturnType<typeof createMockSupervisor>;
  let onClose: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    mockDesktop = createMockDesktopController();
    mockSupervisor = createMockSupervisor();
    onClose = vi.fn();
  });

  it('renders menu title', () => {
    render(createElement(BeginMenu, { onClose }), {
      wrapper: createTestWrapper(mockDesktop, mockSupervisor),
    });

    expect(screen.getByTestId('menu-title')).toHaveTextContent('ZERO OS');
  });

  it('renders main menu items', () => {
    render(createElement(BeginMenu, { onClose }), {
      wrapper: createTestWrapper(mockDesktop, mockSupervisor),
    });

    expect(screen.getByText('Programs')).toBeInTheDocument();
    expect(screen.getByText('Terminal')).toBeInTheDocument();
    expect(screen.getByText('Files')).toBeInTheDocument();
    expect(screen.getByText('Settings')).toBeInTheDocument();
    expect(screen.getByText('Shutdown')).toBeInTheDocument();
  });

  it('renders program submenu items alphabetically', () => {
    render(createElement(BeginMenu, { onClose }), {
      wrapper: createTestWrapper(mockDesktop, mockSupervisor),
    });

    expect(screen.getByText('Calculator')).toBeInTheDocument();
    expect(screen.getByText('Clock')).toBeInTheDocument();

    // Verify alphabetical order in the captured items
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const programsItem = capturedItems.find((item: any) => item.id === 'programs');
    expect(programsItem?.children).toEqual([
      { id: 'calculator', label: 'Calculator' },
      { id: 'clock', label: 'Clock' },
    ]);
  });

  it('launches terminal app on click', () => {
    render(createElement(BeginMenu, { onClose }), {
      wrapper: createTestWrapper(mockDesktop, mockSupervisor),
    });

    const terminalButton = screen.getByTestId('menu-item-terminal');
    fireEvent.click(terminalButton);

    expect(mockDesktop.launch_app).toHaveBeenCalledWith('terminal');
    expect(onClose).toHaveBeenCalled();
  });

  it('launches files app on click', () => {
    render(createElement(BeginMenu, { onClose }), {
      wrapper: createTestWrapper(mockDesktop, mockSupervisor),
    });

    const filesButton = screen.getByTestId('menu-item-files');
    fireEvent.click(filesButton);

    expect(mockDesktop.launch_app).toHaveBeenCalledWith('files');
    expect(onClose).toHaveBeenCalled();
  });

  it('sends shutdown command on shutdown click', () => {
    render(createElement(BeginMenu, { onClose }), {
      wrapper: createTestWrapper(mockDesktop, mockSupervisor),
    });

    const shutdownButton = screen.getByTestId('menu-item-shutdown');
    fireEvent.click(shutdownButton);

    expect(mockSupervisor.send_input).toHaveBeenCalledWith('shutdown');
    expect(onClose).toHaveBeenCalled();
  });

  it('launches calculator from programs submenu', () => {
    render(createElement(BeginMenu, { onClose }), {
      wrapper: createTestWrapper(mockDesktop, mockSupervisor),
    });

    const calculatorButton = screen.getByTestId('menu-item-calculator');
    fireEvent.click(calculatorButton);

    expect(mockDesktop.launch_app).toHaveBeenCalledWith('calculator');
    expect(onClose).toHaveBeenCalled();
  });

  it('closes menu on click outside', () => {
    const { container } = render(
      createElement('div', { 'data-testid': 'outside' }, createElement(BeginMenu, { onClose })),
      { wrapper: createTestWrapper(mockDesktop, mockSupervisor) }
    );

    // Click outside the menu
    const outside = container.querySelector('[data-testid="outside"]');
    if (outside) fireEvent.mouseDown(outside);

    expect(onClose).toHaveBeenCalled();
  });

  it('does not close menu on click inside', () => {
    render(createElement(BeginMenu, { onClose }), {
      wrapper: createTestWrapper(mockDesktop, mockSupervisor),
    });

    const menu = screen.getByTestId('menu');
    fireEvent.mouseDown(menu);

    // onClose should not be called from click inside (only from item selection)
    expect(onClose).not.toHaveBeenCalled();
  });

  it('has correct menu structure with separator before shutdown', () => {
    render(createElement(BeginMenu, { onClose }), {
      wrapper: createTestWrapper(mockDesktop, mockSupervisor),
    });

    // Verify structure: Programs, Terminal, Files, Settings, separator, Shutdown
    expect(capturedItems[0]).toEqual(expect.objectContaining({ id: 'programs' }));
    expect(capturedItems[1]).toEqual({ id: 'terminal', label: 'Terminal' });
    expect(capturedItems[2]).toEqual({ id: 'files', label: 'Files' });
    expect(capturedItems[3]).toEqual(expect.objectContaining({ id: 'settings' }));
    expect(capturedItems[4]).toEqual({ type: 'separator' });
    expect(capturedItems[5]).toEqual({ id: 'shutdown', label: 'Shutdown' });

    // Verify separator is rendered
    expect(screen.getByTestId('menu-separator')).toBeInTheDocument();
  });

  it('cleanup removes event listener', () => {
    const removeEventListenerSpy = vi.spyOn(document, 'removeEventListener');

    const { unmount } = render(createElement(BeginMenu, { onClose }), {
      wrapper: createTestWrapper(mockDesktop, mockSupervisor),
    });

    unmount();

    expect(removeEventListenerSpy).toHaveBeenCalledWith('mousedown', expect.any(Function));

    removeEventListenerSpy.mockRestore();
  });
});
