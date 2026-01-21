import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { createElement } from 'react';
import { BeginMenu } from '../BeginMenu/BeginMenu';
import { DesktopControllerProvider, SupervisorProvider } from '../../desktop/hooks/useSupervisor';
import {
  createMockDesktopController,
  createMockSupervisor,
} from '../../test/mocks';

// Mock the @cypher-asi/zui components
vi.mock('@cypher-asi/zui', () => ({
  Panel: ({ children, className, ...props }: any) =>
    createElement('div', { className, ...props }, children),
}));

// Mock lucide-react icons
vi.mock('lucide-react', () => ({
  TerminalSquare: () => createElement('span', { 'data-testid': 'icon-terminal' }, 'T'),
  Settings: () => createElement('span', { 'data-testid': 'icon-settings' }, 'S'),
  Folder: () => createElement('span', { 'data-testid': 'icon-folder' }, 'F'),
  Power: () => createElement('span', { 'data-testid': 'icon-power' }, 'P'),
}));

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
    render(
      createElement(BeginMenu, { onClose }),
      { wrapper: createTestWrapper(mockDesktop, mockSupervisor) }
    );

    expect(screen.getByText('ZERO OS')).toBeInTheDocument();
  });

  it('renders menu items', () => {
    render(
      createElement(BeginMenu, { onClose }),
      { wrapper: createTestWrapper(mockDesktop, mockSupervisor) }
    );

    expect(screen.getByText('Terminal')).toBeInTheDocument();
    expect(screen.getByText('Settings')).toBeInTheDocument();
    expect(screen.getByText('Files')).toBeInTheDocument();
    expect(screen.getByText('Shutdown')).toBeInTheDocument();
  });

  it('launches terminal app on click', () => {
    render(
      createElement(BeginMenu, { onClose }),
      { wrapper: createTestWrapper(mockDesktop, mockSupervisor) }
    );

    const terminalButton = screen.getByText('Terminal');
    fireEvent.click(terminalButton);

    expect(mockDesktop.launch_app).toHaveBeenCalledWith('terminal');
    expect(onClose).toHaveBeenCalled();
  });

  it('launches settings app on click', () => {
    render(
      createElement(BeginMenu, { onClose }),
      { wrapper: createTestWrapper(mockDesktop, mockSupervisor) }
    );

    const settingsButton = screen.getByText('Settings');
    fireEvent.click(settingsButton);

    expect(mockDesktop.launch_app).toHaveBeenCalledWith('settings');
    expect(onClose).toHaveBeenCalled();
  });

  it('launches files app on click', () => {
    render(
      createElement(BeginMenu, { onClose }),
      { wrapper: createTestWrapper(mockDesktop, mockSupervisor) }
    );

    const filesButton = screen.getByText('Files');
    fireEvent.click(filesButton);

    expect(mockDesktop.launch_app).toHaveBeenCalledWith('files');
    expect(onClose).toHaveBeenCalled();
  });

  it('sends shutdown command on shutdown click', () => {
    render(
      createElement(BeginMenu, { onClose }),
      { wrapper: createTestWrapper(mockDesktop, mockSupervisor) }
    );

    const shutdownButton = screen.getByText('Shutdown');
    fireEvent.click(shutdownButton);

    expect(mockSupervisor.send_input).toHaveBeenCalledWith('shutdown');
    expect(onClose).toHaveBeenCalled();
  });

  it('closes menu on click outside', () => {
    const { container } = render(
      createElement(
        'div',
        { 'data-testid': 'outside' },
        createElement(BeginMenu, { onClose })
      ),
      { wrapper: createTestWrapper(mockDesktop, mockSupervisor) }
    );

    // Click outside the menu
    fireEvent.mouseDown(container.querySelector('[data-testid="outside"]')!);

    expect(onClose).toHaveBeenCalled();
  });

  it('does not close menu on click inside', () => {
    render(
      createElement(BeginMenu, { onClose }),
      { wrapper: createTestWrapper(mockDesktop, mockSupervisor) }
    );

    const menu = screen.getByText('ZERO OS').parentElement!;
    fireEvent.mouseDown(menu);

    // onClose should not be called from click inside (only from item selection)
    expect(onClose).not.toHaveBeenCalled();
  });

  it('renders icons for menu items', () => {
    render(
      createElement(BeginMenu, { onClose }),
      { wrapper: createTestWrapper(mockDesktop, mockSupervisor) }
    );

    expect(screen.getByTestId('icon-terminal')).toBeInTheDocument();
    expect(screen.getByTestId('icon-settings')).toBeInTheDocument();
    expect(screen.getByTestId('icon-folder')).toBeInTheDocument();
    expect(screen.getByTestId('icon-power')).toBeInTheDocument();
  });

  it('cleanup removes event listener', () => {
    const removeEventListenerSpy = vi.spyOn(document, 'removeEventListener');

    const { unmount } = render(
      createElement(BeginMenu, { onClose }),
      { wrapper: createTestWrapper(mockDesktop, mockSupervisor) }
    );

    unmount();

    expect(removeEventListenerSpy).toHaveBeenCalledWith('mousedown', expect.any(Function));

    removeEventListenerSpy.mockRestore();
  });
});
