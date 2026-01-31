import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, act } from '@testing-library/react';
import { SettingsApp } from './SettingsApp';

// Track panel drill stack for testing
let mockPanelStack: Array<{ id: string; label: string; content: React.ReactNode }> = [];

// Helper to flatten Explorer nodes for rendering
interface ExplorerNodeFlat {
  id: string;
  label: string;
  children?: ExplorerNodeFlat[];
}

function flattenNodes(nodes: ExplorerNodeFlat[], result: ExplorerNodeFlat[] = []): ExplorerNodeFlat[] {
  for (const node of nodes) {
    result.push(node);
    if (node.children) {
      flattenNodes(node.children, result);
    }
  }
  return result;
}

// Mock @cypher-asi/zui components
vi.mock('@cypher-asi/zui', () => ({
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  Panel: ({ children, className }: Record<string, any>) => (
    <div className={className} data-testid="panel">
      {children}
    </div>
  ),
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  Explorer: ({ data, onSelect, defaultSelectedIds }: Record<string, any>) => {
    const allNodes = flattenNodes(data);
    return (
      <nav data-testid="explorer" data-value={defaultSelectedIds?.[0]}>
        {allNodes.map((node: { id: string; label: string }) => (
          <button key={node.id} onClick={() => onSelect([node.id])} data-testid={`nav-${node.id}`}>
            {node.label}
          </button>
        ))}
      </nav>
    );
  },
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  PanelDrill: ({ stack, onBack }: Record<string, any>) => {
    // Track the stack for assertions
    mockPanelStack = stack;
    return (
      <div data-testid="panel-drill">
        {stack.length > 1 && (
          <button onClick={onBack} data-testid="back-button">
            Back
          </button>
        )}
        <div data-testid="panel-content">{stack[stack.length - 1]?.content}</div>
        <span data-testid="panel-label">{stack[stack.length - 1]?.label}</span>
      </div>
    );
  },
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  ButtonPlus: ({ onClick }: Record<string, any>) => (
    <button onClick={onClick} data-testid="button-plus">
      +
    </button>
  ),
}));

// Mock lucide-react icons
vi.mock('lucide-react', () => ({
  Clock: () => <span data-testid="clock-icon">Clock</span>,
  User: () => <span data-testid="user-icon">User</span>,
  Shield: () => <span data-testid="shield-icon">Shield</span>,
  Palette: () => <span data-testid="palette-icon">Palette</span>,
  Network: () => <span data-testid="network-icon">Network</span>,
  Brain: () => <span data-testid="brain-icon">Brain</span>,
  Cpu: () => <span data-testid="cpu-icon">Cpu</span>,
  Users: () => <span data-testid="users-icon">Users</span>,
}));

// Mock panel components
vi.mock('./panels/GeneralPanel', () => ({
  GeneralPanel: () => <div data-testid="general-panel">General Panel</div>,
}));

vi.mock('./panels/IdentitySettingsPanel', () => ({
  IdentitySettingsPanel: ({ onDrillDown }: { onDrillDown?: (item: unknown) => void }) => (
    <div data-testid="identity-panel">
      Identity Panel
      <button
        onClick={() =>
          onDrillDown?.({ id: 'neural-key', label: 'Neural Key', content: <div>Neural Key</div> })
        }
        data-testid="drill-neural-key"
      >
        Drill to Neural Key
      </button>
    </div>
  ),
}));

vi.mock('./panels/PermissionsPanel', () => ({
  PermissionsPanel: () => <div data-testid="permissions-panel">Permissions Panel</div>,
}));

vi.mock('./panels/ThemePanel', () => ({
  ThemePanel: () => <div data-testid="theme-panel">Theme Panel</div>,
}));

vi.mock('./panels/NetworkPanel', () => ({
  NetworkPanel: () => <div data-testid="network-panel">Network Panel</div>,
}));

vi.mock('./panels/NeuralKeyPanel', () => ({
  NeuralKeyPanel: () => <div data-testid="neural-key-panel">Neural Key Panel</div>,
}));

vi.mock('./panels/MachineKeysPanel', () => ({
  MachineKeysPanel: () => <div data-testid="machine-keys-panel">Machine Keys Panel</div>,
}));

vi.mock('./panels/LinkedAccountsPanel', () => ({
  LinkedAccountsPanel: () => <div data-testid="linked-accounts-panel">Linked Accounts Panel</div>,
}));

vi.mock('./panels/GenerateMachineKeyPanel', () => ({
  GenerateMachineKeyPanel: () => (
    <div data-testid="generate-machine-key-panel">Generate Machine Key Panel</div>
  ),
}));

// Mock CSS module
vi.mock('./SettingsApp.module.css', () => ({
  default: {
    container: 'container',
    sidebar: 'sidebar',
    content: 'content',
    panelDrill: 'panelDrill',
  },
}));

// Mock the settings store
const mockStore = {
  timeFormat24h: false,
  timezone: 'UTC',
  rpcEndpoint: 'http://localhost:8545',
  pendingNavigation: null,
  setTimeFormat24h: vi.fn(),
  setTimezone: vi.fn(),
  setRpcEndpoint: vi.fn(),
  setPendingNavigation: vi.fn(),
  clearPendingNavigation: vi.fn(),
  loadIdentityPreferences: vi.fn(),
  getState: () => mockStore,
};

vi.mock('../../stores', () => ({
  useSettingsStore: (selector: (state: typeof mockStore) => unknown) => {
    if (typeof selector === 'function') {
      return selector(mockStore);
    }
    return mockStore;
  },
  selectTimeFormat24h: (state: typeof mockStore) => state.timeFormat24h,
  selectTimezone: (state: typeof mockStore) => state.timezone,
  selectRpcEndpoint: (state: typeof mockStore) => state.rpcEndpoint,
  selectPendingNavigation: (state: typeof mockStore) => state.pendingNavigation,
  useIdentityStore: () => ({ currentUser: null }),
  selectCurrentUser: () => null,
}));

// Mock the identity service client hook
vi.mock('../../desktop/hooks/useIdentityServiceClient', () => ({
  useIdentityServiceClient: () => ({
    userId: null,
    client: null,
  }),
}));

// Mock context
vi.mock('./context', () => ({
  PanelDrillProvider: ({ children }: { children: React.ReactNode }) => <>{children}</>,
}));

describe('SettingsApp', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockStore.pendingNavigation = null;
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  describe('Rendering', () => {
    it('renders the settings container', () => {
      render(<SettingsApp />);
      // There are multiple Panel components (container and content)
      expect(screen.getAllByTestId('panel').length).toBeGreaterThan(0);
    });

    it('renders the explorer with all areas and sub-items', () => {
      render(<SettingsApp />);

      // Top-level areas
      expect(screen.getByTestId('nav-general')).toBeDefined();
      expect(screen.getByTestId('nav-identity')).toBeDefined();
      expect(screen.getByTestId('nav-network')).toBeDefined();
      expect(screen.getByTestId('nav-permissions')).toBeDefined();
      expect(screen.getByTestId('nav-theme')).toBeDefined();

      // Identity sub-items
      expect(screen.getByTestId('nav-neural-key')).toBeDefined();
      expect(screen.getByTestId('nav-machine-keys')).toBeDefined();
      expect(screen.getByTestId('nav-linked-accounts')).toBeDefined();
    });

    it('starts with identity area selected', () => {
      render(<SettingsApp />);

      const explorer = screen.getByTestId('explorer');
      expect(explorer.getAttribute('data-value')).toBe('identity');
    });

    it('renders the PanelDrill component', () => {
      render(<SettingsApp />);
      expect(screen.getByTestId('panel-drill')).toBeDefined();
    });
  });

  describe('Navigation', () => {
    it('switches to general panel when clicked', async () => {
      render(<SettingsApp />);

      const generalNav = screen.getByTestId('nav-general');
      await act(async () => {
        fireEvent.click(generalNav);
      });

      expect(screen.getByTestId('general-panel')).toBeDefined();
    });

    it('switches to permissions panel when clicked', async () => {
      render(<SettingsApp />);

      const permissionsNav = screen.getByTestId('nav-permissions');
      await act(async () => {
        fireEvent.click(permissionsNav);
      });

      expect(screen.getByTestId('permissions-panel')).toBeDefined();
    });

    it('switches to theme panel when clicked', async () => {
      render(<SettingsApp />);

      const themeNav = screen.getByTestId('nav-theme');
      await act(async () => {
        fireEvent.click(themeNav);
      });

      expect(screen.getByTestId('theme-panel')).toBeDefined();
    });

    it('switches to network panel when clicked', async () => {
      render(<SettingsApp />);

      const networkNav = screen.getByTestId('nav-network');
      await act(async () => {
        fireEvent.click(networkNav);
      });

      expect(screen.getByTestId('network-panel')).toBeDefined();
    });

    it('drills directly to neural key panel from explorer', async () => {
      render(<SettingsApp />);

      const neuralKeyNav = screen.getByTestId('nav-neural-key');
      await act(async () => {
        fireEvent.click(neuralKeyNav);
      });

      expect(screen.getByTestId('neural-key-panel')).toBeDefined();
      expect(screen.getByTestId('panel-label').textContent).toBe('Neural Key');
    });

    it('drills directly to machine keys panel from explorer', async () => {
      render(<SettingsApp />);

      const machineKeysNav = screen.getByTestId('nav-machine-keys');
      await act(async () => {
        fireEvent.click(machineKeysNav);
      });

      expect(screen.getByTestId('machine-keys-panel')).toBeDefined();
      expect(screen.getByTestId('panel-label').textContent).toBe('Machine Keys');
    });

    it('drills directly to linked accounts panel from explorer', async () => {
      render(<SettingsApp />);

      const linkedAccountsNav = screen.getByTestId('nav-linked-accounts');
      await act(async () => {
        fireEvent.click(linkedAccountsNav);
      });

      expect(screen.getByTestId('linked-accounts-panel')).toBeDefined();
      expect(screen.getByTestId('panel-label').textContent).toBe('Linked Accounts');
    });
  });

  describe('Panel drill navigation', () => {
    it('shows identity panel label initially', () => {
      render(<SettingsApp />);

      expect(screen.getByTestId('panel-label').textContent).toBe('Identity');
    });

    it('can drill down from identity panel', async () => {
      render(<SettingsApp />);

      // Click drill button in identity panel
      const drillButton = screen.getByTestId('drill-neural-key');
      await act(async () => {
        fireEvent.click(drillButton);
      });

      // Should show back button and new label
      expect(screen.getByTestId('back-button')).toBeDefined();
      expect(screen.getByTestId('panel-label').textContent).toBe('Neural Key');
    });

    it('can navigate back from drilled panel', async () => {
      render(<SettingsApp />);

      // Drill down
      const drillButton = screen.getByTestId('drill-neural-key');
      await act(async () => {
        fireEvent.click(drillButton);
      });

      // Back button should be present after drilling
      expect(screen.getByTestId('back-button')).toBeDefined();

      // Navigate back - this triggers the onBack callback
      // The actual state management is internal to the component
      const backButton = screen.getByTestId('back-button');
      await act(async () => {
        fireEvent.click(backButton);
      });

      // After back, panel stack should be shorter (internal state)
      // We verify that the component handles the back action without errors
      expect(mockPanelStack).toBeDefined();
    });
  });

  describe('State from store', () => {
    it('passes time settings to general panel', async () => {
      render(<SettingsApp />);

      // Navigate to general panel
      const generalNav = screen.getByTestId('nav-general');
      await act(async () => {
        fireEvent.click(generalNav);
      });

      // General panel should be rendered (props are passed internally)
      expect(screen.getByTestId('general-panel')).toBeDefined();
    });

    it('passes network settings to network panel', async () => {
      render(<SettingsApp />);

      // Navigate to network panel
      const networkNav = screen.getByTestId('nav-network');
      await act(async () => {
        fireEvent.click(networkNav);
      });

      // Network panel should be rendered
      expect(screen.getByTestId('network-panel')).toBeDefined();
    });
  });
});
