import { useState, useEffect, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { AnimatePresence, motion } from 'framer-motion';
import { WindowChrome } from './components/WindowChrome';
import { LockScreen } from './components/LockScreen';
import { MainVault } from './components/MainVault';
import { EditItem } from './components/EditItem';
import { CategoryManager } from './components/CategoryManager';
import { Settings } from './components/Settings';
import { WorkspaceManager } from './components/WorkspaceManager';
import { ContextMenu } from './components/ui/ContextMenu';
import { Toast } from './components/ui/Toast';
import { PlaceholderModal } from './components/ui/PlaceholderModal';
import { SetupWizard } from './components/SetupWizard';
import { useVaultStore } from './store';
import { useAutoLock } from './hooks/useAutoLock';
import type { Screen } from './types';

const SCREENS: Record<Screen, React.ReactElement> = {
  lock:       <LockScreen />,
  vault:      <MainVault />,
  edit:       <EditItem />,
  categories: <CategoryManager />,
  settings:   <Settings />,
  workspaces: <WorkspaceManager />,
};

export default function App() {
  useAutoLock();
  const screen         = useVaultStore((s) => s.screen);
  const menu           = useVaultStore((s) => s.menu);
  const closeMenu      = useVaultStore((s) => s.closeMenu);
  const toast          = useVaultStore((s) => s.toast);
  const placeholder    = useVaultStore((s) => s.placeholder);
  const setPlaceholder = useVaultStore((s) => s.setPlaceholder);

  const [showSetupWizard, setShowSetupWizard] = useState(false);
  const prevScreenRef = useRef<Screen>(screen);

  useEffect(() => {
    const prev = prevScreenRef.current;
    prevScreenRef.current = screen;
    if (prev === 'lock' && screen === 'vault') {
      invoke<boolean>('app_is_first_run')
        .then((isFirst) => { if (isFirst) setShowSetupWizard(true); })
        .catch(() => {});
    }
  }, [screen]);

  return (
    <div className="flex flex-col w-full h-full bg-bg overflow-hidden">
      <WindowChrome />

      {/* Screen area */}
      <div className="flex-1 flex flex-col overflow-hidden relative">
        <AnimatePresence mode="wait">
          <motion.div
            key={screen}
            className="absolute inset-0 flex flex-col overflow-hidden"
            initial={{ opacity: 0, y: 4 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -4 }}
            transition={{ duration: 0.13, ease: 'easeOut' }}
          >
            {SCREENS[screen]}
          </motion.div>
        </AnimatePresence>
      </div>

      {/* Global overlays */}
      {menu && <ContextMenu {...menu} onClose={closeMenu} />}
      {toast && <Toast msg={toast.type === 'error' ? toast.msg : `✓ ${toast.msg}`} type={toast.type} />}
      {placeholder && placeholder.type === 'command' && (
        <PlaceholderModal
          command={(placeholder as any).command}
          onClose={() => setPlaceholder(null)}
        />
      )}
      {showSetupWizard && (
        <SetupWizard onClose={() => setShowSetupWizard(false)} />
      )}
    </div>
  );
}
