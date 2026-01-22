import { useRef, useEffect, useState, useCallback } from 'react';
import { useSupervisor } from '../../desktop/hooks/useSupervisor';
import { Drawer, GroupCollapsible, Button, Text, Label } from '@cypher-asi/zui';
import styles from './TerminalApp.module.css';

interface TerminalAppProps {
  windowId: number;
  /** Process ID for this terminal instance (for process isolation) */
  processId?: number;
}

interface ProcessInfo {
  pid: number;
  name: string;
  state: string;
  memory: number;
  worker_id: number;
}

interface AxiomStats {
  commits: number;
  events: number;
  persisted: number;
  pending: number;
  storage_ready: boolean;
}

// Color palette for processes
const COLORS = [
  '#01f4cb', '#60a5fa', '#f472b6', '#facc15',
  '#a78bfa', '#fb923c', '#01f4cb', '#f87171',
  '#818cf8', '#01f4cb', '#fbbf24', '#f97316',
];

function formatBytes(bytes: number): string {
  if (bytes >= 1024 * 1024) return (bytes / (1024 * 1024)).toFixed(1) + ' MB';
  if (bytes >= 1024) return (bytes / 1024).toFixed(1) + ' KB';
  return bytes + ' B';
}

export function TerminalApp({ windowId: _windowId, processId }: TerminalAppProps) {
  const supervisor = useSupervisor();
  const terminalRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const [output, setOutput] = useState<Array<{ text: string; className: string }>>([]);
  const [commandHistory, setCommandHistory] = useState<string[]>([]);
  const [historyIndex, setHistoryIndex] = useState(-1);
  const [processes, setProcesses] = useState<ProcessInfo[]>([]);
  const [axiomStats, setAxiomStats] = useState<AxiomStats | null>(null);
  const [isDrawerOpen, setIsDrawerOpen] = useState(false);

  // Set up console callback - either per-process (if processId provided) or legacy global
  useEffect(() => {
    console.log('[TerminalApp] useEffect running, supervisor:', supervisor ? 'available' : 'null', 'processId:', processId);
    if (!supervisor) return;

    const handleOutput = (text: string) => {
      console.log('[TerminalApp] Received:', JSON.stringify(text), 'processId:', processId);
      // Handle clear screen escape sequence
      if (text.includes('\x1B[2J')) {
        setOutput([]);
        return;
      }
      setOutput((prev) => [...prev, { text, className: styles.outputText }]);
    };

    if (processId != null) {
      // Process-isolated mode: register callback for specific PID
      // Note: Rust u64 maps to JavaScript BigInt in wasm-bindgen
      console.log('[TerminalApp] Registering per-process callback for PID', processId);
      supervisor.register_console_callback(BigInt(processId), handleOutput);

      // Cleanup: unregister callback when unmounting
      return () => {
        console.log('[TerminalApp] Unregistering callback for PID', processId);
        supervisor.unregister_console_callback(BigInt(processId));
      };
    } else {
      // Legacy mode: use global callback (backward compatible)
      console.log('[TerminalApp] Using legacy global console callback');
      supervisor.set_console_callback(handleOutput);
    }
  }, [supervisor, processId]);

  // Update dashboard data
  useEffect(() => {
    if (!supervisor) return;

    const update = () => {
      try {
        const procs = JSON.parse(supervisor.get_process_list_json());
        setProcesses(procs);

        const stats = JSON.parse(supervisor.get_axiom_stats_json());
        setAxiomStats(stats);
      } catch {
        // Ignore parse errors
      }
    };

    update();
    const interval = setInterval(update, 500);
    return () => clearInterval(interval);
  }, [supervisor]);

  // Scroll to bottom on new output
  useEffect(() => {
    if (terminalRef.current) {
      terminalRef.current.scrollTop = terminalRef.current.scrollHeight;
    }
  }, [output]);

  // Auto-focus input when terminal opens
  useEffect(() => {
    // Small delay to ensure the window is fully rendered
    const timer = setTimeout(() => {
      inputRef.current?.focus();
    }, 50);
    return () => clearTimeout(timer);
  }, []);

  const handleSubmit = useCallback(() => {
    const input = inputRef.current;
    if (!input || !supervisor) return;

    const line = input.value;
    input.value = '';

    if (line.trim()) {
      setCommandHistory((prev) => [...prev, line]);
      setHistoryIndex(-1);
    }

    setOutput((prev) => [...prev, { text: `z::> ${line}\n`, className: styles.inputEcho }]);
    
    // Send input to specific process (if processId provided) or legacy global
    if (processId != null) {
      supervisor.send_input_to_process(BigInt(processId), line);
    } else {
      supervisor.send_input(line);
    }
  }, [supervisor, processId]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === 'Enter') {
        handleSubmit();
      } else if (e.key === 'Escape') {
        // Blur the input to allow desktop keyboard shortcuts
        e.preventDefault();
        if (inputRef.current) {
          inputRef.current.blur();
        }
      } else if (e.key === 'ArrowUp') {
        e.preventDefault();
        if (commandHistory.length > 0) {
          const newIndex = historyIndex < 0 ? commandHistory.length - 1 : Math.max(0, historyIndex - 1);
          setHistoryIndex(newIndex);
          if (inputRef.current) {
            inputRef.current.value = commandHistory[newIndex];
          }
        }
      } else if (e.key === 'ArrowDown') {
        e.preventDefault();
        if (historyIndex >= 0) {
          const newIndex = historyIndex + 1;
          if (newIndex >= commandHistory.length) {
            setHistoryIndex(-1);
            if (inputRef.current) inputRef.current.value = '';
          } else {
            setHistoryIndex(newIndex);
            if (inputRef.current) inputRef.current.value = commandHistory[newIndex];
          }
        }
      }
    },
    [commandHistory, historyIndex, handleSubmit]
  );

  const spawnProcess = (type: string) => {
    if (supervisor) {
      setOutput((prev) => [...prev, { text: `z::> spawn ${type}\n`, className: styles.inputEcho }]);
      // Send spawn command to this terminal's process (or legacy global)
      if (processId != null) {
        supervisor.send_input_to_process(BigInt(processId), `spawn ${type}`);
      } else {
        supervisor.send_input(`spawn ${type}`);
      }
    }
  };

  const killProcess = (pid: number) => {
    if (supervisor) {
      setOutput((prev) => [...prev, { text: `z::> kill ${pid}\n`, className: styles.inputEcho }]);
      // Send kill command to this terminal's process (or legacy global)
      if (processId != null) {
        supervisor.send_input_to_process(BigInt(processId), `kill ${pid}`);
      } else {
        supervisor.send_input(`kill ${pid}`);
      }
    }
  };

  return (
    <div className={styles.terminalApp}>
      {/* Terminal panel - main content */}
      <div className={styles.terminalPanel}>
        <div ref={terminalRef} className={styles.terminal} data-selectable-text onClick={() => {
          // Only focus input if no text is selected (preserve text selection)
          const selection = window.getSelection();
          if (!selection || selection.isCollapsed) {
            inputRef.current?.focus();
          }
        }}>
          {output.map((line, i) => (
            <span key={i} className={line.className}>
              {line.text}
            </span>
          ))}
          <span className={styles.inputLine}>
            <span className={styles.prompt}>z::&gt;</span>
            <input
              ref={inputRef}
              type="text"
              className={styles.input}
              autoComplete="off"
              spellCheck={false}
              onKeyDown={handleKeyDown}
            />
          </span>
        </div>
      </div>

      {/* Dashboard drawer on right */}
      <Drawer
        side="right"
        isOpen={isDrawerOpen}
        onClose={() => setIsDrawerOpen(false)}
        onOpen={() => setIsDrawerOpen(true)}
        title="Dashboard"
        showToggle
        transparent
        noBorder
        defaultSize={280}
        minSize={200}
        maxSize={400}
      >
        <div className={styles.drawerContent}>
          <GroupCollapsible
            title="Processes"
            count={processes.length}
            defaultOpen
            className={styles.collapsibleGroup}
          >
            <div className={styles.panelContent}>
              {processes.map((p, i) => (
                <div key={p.pid} className={styles.processItem}>
                  <Text as="span" size="xs" variant="muted" className={styles.processPid}>{p.pid}</Text>
                  <span className={styles.processName} style={{ color: COLORS[i % COLORS.length] }}>
                    {p.name}
                  </span>
                  <Text as="span" size="xs" variant="muted" className={styles.processMem}>{formatBytes(p.memory)}</Text>
                  <Label
                    variant={p.state === 'Running' ? 'success' : p.state === 'Blocked' ? 'warning' : 'danger'}
                    size="xs"
                  >
                    {p.state}
                  </Label>
                  {p.pid > 2 && (
                    <Button
                      variant="danger"
                      size="sm"
                      iconOnly
                      onClick={() => killProcess(p.pid)}
                    >
                      ×
                    </Button>
                  )}
                </div>
              ))}
              <div className={styles.quickActions}>
                <Button variant="ghost" size="sm" onClick={() => spawnProcess('memhog')}>
                  + memhog
                </Button>
                <Button variant="ghost" size="sm" onClick={() => spawnProcess('idle')}>
                  + idle
                </Button>
              </div>
            </div>
          </GroupCollapsible>

          <GroupCollapsible
            title="Memory"
            stats={formatBytes(processes.reduce((s, p) => s + p.memory, 0))}
            defaultOpen
            className={styles.collapsibleGroup}
          >
            <div className={styles.panelContent}>
              <div className={styles.memBar}>
                {processes.map((p, i) => {
                  const total = processes.reduce((s, proc) => s + proc.memory, 0) || 1;
                  const pct = (p.memory / total) * 100;
                  return (
                    <div
                      key={p.pid}
                      className={styles.memSegment}
                      style={{ width: `${pct}%`, background: COLORS[i % COLORS.length] }}
                      title={`${p.name}: ${formatBytes(p.memory)}`}
                    />
                  );
                })}
              </div>
            </div>
          </GroupCollapsible>

          {axiomStats && (
            <GroupCollapsible
              title="Axiom"
              stats={`${axiomStats.commits} commits`}
              defaultOpen
              className={styles.collapsibleGroup}
            >
              <div className={styles.panelContent}>
                <div className={styles.axiomStats}>
                  <Text as="div" size="xs" variant="muted">Storage: {axiomStats.storage_ready ? '✓ Ready' : 'Not initialized'}</Text>
                  <Text as="div" size="xs" variant="muted">Events: {axiomStats.events}</Text>
                  <Text as="div" size="xs" variant="muted">Persisted: {axiomStats.persisted} | Pending: {axiomStats.pending}</Text>
                </div>
              </div>
            </GroupCollapsible>
          )}
        </div>
      </Drawer>
    </div>
  );
}
