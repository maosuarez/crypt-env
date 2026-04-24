import { getCurrentWindow } from '@tauri-apps/api/window';
import { Icon } from './ui/Icon';
import { useVaultStore } from '../store';

const win = getCurrentWindow();

export function WindowChrome() {
  const screen = useVaultStore((s) => s.screen);
  const lock   = useVaultStore((s) => s.lock);

  return (
    <div
      className="relative flex items-center h-12 bg-bg border-b border-bd select-none shrink-0"
    >
      {/* Title centered absolutely — pointer-events-none so drag passes through */}
      <div className="absolute inset-0 flex items-center justify-center pointer-events-none">
        <div className="flex items-center gap-2 text-xs font-medium tracking-widest text-tx3 font-mono">
          <Icon name="shield" size={14} color="oklch(0.70 0.17 162)" />
          VAULT
        </div>
      </div>

      {/* Left spacer (draggable) */}
      <div data-tauri-drag-region className="flex-1 h-full" />

      {/* Lock button + Windows controls — z-10 so they sit above the title overlay */}
      {screen !== 'lock' && (
        <button
          onClick={() => lock()}
          className={[
            'relative z-10 flex items-center gap-1.5 px-2.5 py-1 mr-2 rounded',
            'bg-transparent border-none cursor-pointer',
            'text-xs font-ui tracking-wider text-tx3',
            'hover:text-danger transition-colors duration-150',
          ].join(' ')}
        >
          <Icon name="lock" size={13} />
          LOCK
        </button>
      )}

      {/* Windows-style window controls */}
      <div className="relative z-10 flex h-full items-stretch border-l border-bd">
        {/* Minimize */}
        <button
          onClick={() => win.minimize()}
          title="Minimize"
          className={[
            'flex items-center justify-center w-12 h-full border-none cursor-pointer',
            'bg-transparent text-tx3 hover:text-tx hover:bg-surface transition-colors duration-100',
          ].join(' ')}
          aria-label="Minimize"
        >
          <svg width="10" height="1" viewBox="0 0 10 1" fill="currentColor">
            <rect width="10" height="1" />
          </svg>
        </button>

        {/* Close */}
        <button
          onClick={() => win.close()}
          title="Close"
          className={[
            'flex items-center justify-center w-12 h-full border-none cursor-pointer',
            'bg-transparent text-tx3 hover:text-tx hover:bg-danger transition-colors duration-100',
          ].join(' ')}
          aria-label="Close"
        >
          <svg width="10" height="10" viewBox="0 0 10 10" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round">
            <line x1="0" y1="0" x2="10" y2="10" />
            <line x1="10" y1="0" x2="0" y2="10" />
          </svg>
        </button>
      </div>
    </div>
  );
}
