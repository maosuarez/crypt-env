import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Icon } from './ui/Icon';
import { ImportModal } from './ImportModal';
import { BackupModal } from './BackupModal';
import { useVaultStore } from '../store';
import type { IconName } from '../types';

function Row({ icon, label, children }: { icon: IconName; label: string; children: React.ReactNode }) {
  return (
    <div className="py-3 border-b border-bd flex items-center gap-3">
      <span className="text-tx3 shrink-0"><Icon name={icon} size={14} /></span>
      <span className="flex-1 text-[12px] font-semibold text-tx">{label}</span>
      {children}
    </div>
  );
}

function Sec({ title }: { title: string }) {
  return (
    <div className="text-[10px] font-semibold text-tx3 tracking-[0.09em] mt-4 pb-1.5 border-b border-bd font-mono">
      {title}
    </div>
  );
}

function PwField({
  label, value, show, onChange, onToggle,
}: { label: string; value: string; show: boolean; onChange: (v: string) => void; onToggle: () => void }) {
  return (
    <div className="mb-3">
      <div className="text-[10px] font-semibold text-tx3 font-mono tracking-[0.06em] mb-1">{label}</div>
      <div className="relative">
        <input
          type={show ? 'text' : 'password'}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          autoComplete="off"
          className="w-full bg-bg border border-bd2 text-tx font-mono text-[13px] rounded-[3px] px-3 py-[7px] pr-9 outline-none focus:border-accent-d transition-colors"
        />
        <button
          type="button"
          onClick={onToggle}
          className="absolute right-2.5 top-1/2 -translate-y-1/2 text-tx3 hover:text-tx transition-colors"
        >
          <Icon name={show ? 'eyeOff' : 'eye'} size={14} />
        </button>
      </div>
    </div>
  );
}

export function Settings() {
  const go             = useVaultStore((s) => s.go);
  const showToast      = useVaultStore((s) => s.showToast);
  const storeWipe      = useVaultStore((s) => s.wipe);
  const storeLockTimeout = useVaultStore((s) => s.lockTimeout);
  const storeHotkey      = useVaultStore((s) => s.hotkey);
  const setLockTimeout   = useVaultStore((s) => s.setLockTimeout);
  const setHotkey        = useVaultStore((s) => s.setHotkey);

  const [timeoutDraft, setTimeoutDraft] = useState(storeLockTimeout);
  const [hotkeyDraft,  setHotkeyDraft]  = useState(storeHotkey);
  const [capturing,    setCapturing]    = useState(false);
  const [saving,       setSaving]       = useState(false);
  const [saved,        setSaved]        = useState(false);
  const [wipeOpen,     setWipeOpen]     = useState(false);
  const [wiping,       setWiping]       = useState(false);
  const [importOpen,   setImportOpen]   = useState(false);
  const [backupOpen,   setBackupOpen]   = useState(false);

  // MCP token
  const [mcpToken,        setMcpToken]        = useState<string | null>(null);
  const [mcpTokenVisible, setMcpTokenVisible] = useState(false);
  const [generatingMcp,   setGeneratingMcp]   = useState(false);

  // Change master password modal state
  const [changePwOpen, setChangePwOpen] = useState(false);
  const [currentPw,    setCurrentPw]    = useState('');
  const [newPw,        setNewPw]        = useState('');
  const [confirmPw,    setConfirmPw]    = useState('');
  const [showCurrent,  setShowCurrent]  = useState(false);
  const [showNew,      setShowNew]      = useState(false);
  const [showConfirm,  setShowConfirm]  = useState(false);
  const [pwError,      setPwError]      = useState('');
  const [pwChanging,   setPwChanging]   = useState(false);

  const openChangePw = () => {
    setCurrentPw(''); setNewPw(''); setConfirmPw('');
    setShowCurrent(false); setShowNew(false); setShowConfirm(false);
    setPwError('');
    setChangePwOpen(true);
  };

  const closeChangePw = () => { setChangePwOpen(false); setPwError(''); };

  const handleChangePassword = async () => {
    if (newPw.length < 8) { setPwError('New password must be at least 8 characters'); return; }
    if (newPw !== confirmPw) { setPwError('New passwords do not match'); return; }
    setPwChanging(true);
    setPwError('');
    try {
      await invoke('vault_change_password', { currentPassword: currentPw, newPassword: newPw });
      closeChangePw();
      showToast('Master password changed successfully');
    } catch (e: unknown) {
      setPwError(e instanceof Error ? e.message : String(e));
    } finally {
      setPwChanging(false);
    }
  };

  useEffect(() => {
    invoke<{ autoLockTimeout: number; hotkey: string }>('vault_get_settings')
      .then((s) => {
        setTimeoutDraft(s.autoLockTimeout);
        setHotkeyDraft(s.hotkey);
        setLockTimeout(s.autoLockTimeout);
        setHotkey(s.hotkey);
      })
      .catch(() => {});

    invoke<string | null>('vault_get_mcp_token')
      .then((t) => setMcpToken(t))
      .catch(() => {});
  }, []);

  const handleSave = async () => {
    setSaving(true);
    try {
      await invoke('vault_save_settings', {
        autoLockTimeout: timeoutDraft,
        hotkey: hotkeyDraft,
      });
      setLockTimeout(timeoutDraft);
      setHotkey(hotkeyDraft);
      setSaved(true);
      setTimeout(() => setSaved(false), 1800);
    } finally {
      setSaving(false);
    }
  };

  const handleGenerateMcpToken = async () => {
    setGeneratingMcp(true);
    try {
      const token = await invoke<string>('vault_generate_mcp_token');
      setMcpToken(token);
      setMcpTokenVisible(true);
    } catch (e) {
      showToast('Failed to generate token');
    } finally {
      setGeneratingMcp(false);
    }
  };

  const handleWipe = async () => {
    setWiping(true);
    try {
      await storeWipe();
    } finally {
      setWiping(false);
      setWipeOpen(false);
    }
  };

  return (
    <div className="flex-1 flex flex-col overflow-hidden animate-fade-in relative">
      {/* Header */}
      <div className="px-3.5 py-[9px] border-b border-bd flex items-center gap-[10px] shrink-0">
        <button
          onClick={() => go('vault')}
          className="flex items-center gap-1 text-[12px] font-medium font-ui text-tx3 bg-transparent border-none cursor-pointer hover:text-tx transition-colors"
        >
          <Icon name="back" size={13} />Back
        </button>
        <div className="flex-1 text-[13px] font-semibold text-center text-tx">Settings</div>
        <div className="w-[50px]" />
      </div>

      {/* Body */}
      <div className="flex-1 overflow-y-auto pt-1 px-4 pb-4 bg-surface">
        <Sec title="// SECURITY" />
        <Row icon="key" label="Master Password">
          <button
            onClick={openChangePw}
            className="bg-transparent border border-bd2 rounded-[3px] text-tx2 px-[11px] py-[5px] text-[11px] cursor-pointer font-ui font-medium tracking-[0.04em] hover:text-tx transition-colors"
          >
            CHANGE
          </button>
        </Row>
        <Row icon="timer" label="Auto-lock Timeout">
          <select
            value={timeoutDraft}
            onChange={(e) => setTimeoutDraft(Number(e.target.value))}
            className="bg-raised border border-bd2 text-tx rounded-[3px] px-2 py-[5px] text-[12px] font-ui cursor-pointer outline-none"
          >
            {[{ v: 1, l: '1 min' }, { v: 5, l: '5 min' }, { v: 15, l: '15 min' }, { v: 30, l: '30 min' }, { v: 0, l: 'Never' }].map((o) => (
              <option key={o.v} value={o.v}>{o.l}</option>
            ))}
          </select>
        </Row>

        <Sec title="// INTERFACE" />
        <Row icon="kbd" label="Global Hotkey">
          <button
            onClick={() => setCapturing(true)}
            onKeyDown={(e) => {
              if (!capturing) return;
              e.preventDefault();
              const m: string[] = [];
              if (e.ctrlKey)  m.push('Ctrl');
              if (e.altKey)   m.push('Alt');
              if (e.shiftKey) m.push('Shift');
              if (e.metaKey)  m.push('Meta');
              const k = e.key.length === 1 ? e.key.toUpperCase() : e.key;
              if (!['Control', 'Alt', 'Shift', 'Meta'].includes(e.key)) {
                setHotkeyDraft([...m, k].join('+'));
                setCapturing(false);
              }
            }}
            onBlur={() => setCapturing(false)}
            className={[
              'rounded-[3px] px-[10px] py-[5px] text-[11px] cursor-pointer font-mono tracking-[0.06em]',
              'border outline-none transition-all duration-150',
              capturing
                ? 'bg-accent-b border-accent-d text-accent animate-blink'
                : 'bg-raised border-bd2 text-tx hover:border-accent-d',
            ].join(' ')}
          >
            {capturing ? 'Press keys…' : hotkeyDraft}
          </button>
        </Row>
        <Row icon="tag" label="Manage Categories">
          <button
            onClick={() => go('categories')}
            className="flex items-center gap-[5px] bg-transparent border border-bd2 rounded-[3px] text-tx2 px-[11px] py-[5px] text-[11px] cursor-pointer font-ui font-medium tracking-[0.04em] hover:text-tx transition-colors"
          >
            MANAGE →
          </button>
        </Row>

        <Sec title="// DATA" />
        <Row icon="export" label="Import from Password Manager">
          <button
            onClick={() => setImportOpen(true)}
            className="bg-transparent border border-bd2 rounded-[3px] text-tx2 px-[11px] py-[5px] text-[11px] cursor-pointer font-ui font-medium tracking-[0.04em] hover:text-tx transition-colors"
          >
            IMPORT
          </button>
        </Row>
        <Row icon="export" label="Backup & Restore">
          <button
            onClick={() => setBackupOpen(true)}
            className="bg-transparent border border-bd2 rounded-[3px] text-tx2 px-[11px] py-[5px] text-[11px] cursor-pointer font-ui font-medium tracking-[0.04em] hover:text-tx transition-colors"
          >
            MANAGE
          </button>
        </Row>
        <Row icon="trash" label="Wipe All Data">
          <button
            onClick={() => setWipeOpen(true)}
            className="bg-danger-b border border-danger rounded-[3px] text-danger px-[11px] py-[5px] text-[11px] cursor-pointer font-ui font-medium tracking-[0.04em] hover:opacity-80 transition-opacity"
          >
            WIPE
          </button>
        </Row>

        <Sec title="// INTEGRATIONS" />
        <Row icon="key" label="MCP Token">
          <button
            onClick={handleGenerateMcpToken}
            disabled={generatingMcp}
            className="bg-transparent border border-bd2 rounded-[3px] text-tx2 px-[11px] py-[5px] text-[11px] cursor-pointer font-ui font-medium tracking-[0.04em] hover:text-tx transition-colors disabled:opacity-40 flex items-center gap-1"
          >
            {generatingMcp
              ? <><div className="w-2.5 h-2.5 rounded-full border-2 border-transparent border-t-current animate-spin-fast" />GEN…</>
              : mcpToken ? 'REGENERATE' : 'GENERATE'}
          </button>
        </Row>
        {mcpToken && (
          <div className="mt-1.5 mb-2 p-2.5 bg-raised border border-bd rounded-[3px] flex items-center gap-2">
            <code className="flex-1 text-[10px] font-mono text-tx2 truncate select-all">
              {mcpTokenVisible ? mcpToken : '••••••••••••••••••••••••••••••••'}
            </code>
            <button
              onClick={() => setMcpTokenVisible((v) => !v)}
              className="text-tx3 hover:text-tx transition-colors shrink-0"
              title={mcpTokenVisible ? 'Hide' : 'Show'}
            >
              <Icon name={mcpTokenVisible ? 'eyeOff' : 'eye'} size={12} />
            </button>
            <button
              onClick={() => { navigator.clipboard.writeText(mcpToken); showToast('Token copied'); }}
              className="text-tx3 hover:text-tx transition-colors shrink-0"
              title="Copy"
            >
              <Icon name="copy" size={12} />
            </button>
          </div>
        )}

        {/* About block */}
        <div className="mt-5 p-3 bg-raised border border-bd rounded-[3px]">
          <div className="text-[10px] text-tx3 font-mono leading-[1.8]">
            vault v2.0.0 · tauri 2.0 · rust 1.77<br />
            storage: ~/.vault/data.enc · argon2id m=65536 t=3
          </div>
        </div>
      </div>

      {/* Footer */}
      <div className="px-3.5 py-[10px] border-t border-bd shrink-0 bg-bg">
        <button
          onClick={handleSave}
          className={[
            'w-full py-[9px] rounded-[3px] text-[12px] font-bold tracking-[0.06em] cursor-pointer font-ui',
            'flex items-center justify-center gap-1.5 transition-all duration-200',
            saving
              ? 'bg-accent-d text-[#020504] border-none'
              : saved
              ? 'bg-accent-b border border-accent-d text-accent'
              : 'bg-accent border-none text-[#020504] hover:opacity-90',
          ].join(' ')}
        >
          {saving ? (
            <><div className="w-3 h-3 rounded-full border-2 border-transparent border-t-[#020504] animate-spin-fast" />SAVING…</>
          ) : saved ? (
            <><Icon name="check" size={12} color="oklch(0.70 0.17 162)" />SAVED</>
          ) : (
            'SAVE SETTINGS'
          )}
        </button>
      </div>

      {/* Wipe Confirmation Modal */}
      {wipeOpen && (
        <div className="absolute inset-0 bg-black/70 flex items-center justify-center z-20 p-5">
          <div className="w-full bg-surface border border-danger rounded-[4px] p-4">
            <div className="text-[10px] font-semibold text-danger font-mono tracking-[0.09em] mb-3">
              // WIPE ALL DATA
            </div>
            <p className="text-[12px] text-tx mb-1 font-ui">
              This will permanently delete your vault database and all stored secrets.
            </p>
            <p className="text-[11px] text-tx3 font-mono mb-4">
              This action cannot be undone.
            </p>
            <div className="flex gap-2">
              <button
                onClick={() => setWipeOpen(false)}
                disabled={wiping}
                className="flex-1 py-[8px] rounded-[3px] text-[11px] font-bold tracking-[0.06em] font-ui cursor-pointer bg-transparent border border-bd2 text-tx2 hover:text-tx transition-colors disabled:opacity-40"
              >
                CANCEL
              </button>
              <button
                onClick={handleWipe}
                disabled={wiping}
                className="flex-1 py-[8px] rounded-[3px] text-[11px] font-bold tracking-[0.06em] font-ui cursor-pointer bg-danger border-none text-white hover:opacity-90 transition-opacity disabled:opacity-40 flex items-center justify-center gap-1.5"
              >
                {wiping
                  ? <><div className="w-3 h-3 rounded-full border-2 border-transparent border-t-white animate-spin-fast" />WIPING…</>
                  : 'CONFIRM WIPE'}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Change Master Password Modal */}
      {changePwOpen && (
        <div className="absolute inset-0 bg-black/70 flex items-center justify-center z-20 p-5">
          <div className="w-full bg-surface border border-bd rounded-[4px] p-4">
            <div className="text-[10px] font-semibold text-tx3 font-mono tracking-[0.09em] mb-3">
              // CHANGE MASTER PASSWORD
            </div>

            <PwField
              label="CURRENT PASSWORD"
              value={currentPw}
              show={showCurrent}
              onChange={setCurrentPw}
              onToggle={() => setShowCurrent((v) => !v)}
            />
            <PwField
              label="NEW PASSWORD"
              value={newPw}
              show={showNew}
              onChange={setNewPw}
              onToggle={() => setShowNew((v) => !v)}
            />
            <PwField
              label="CONFIRM NEW PASSWORD"
              value={confirmPw}
              show={showConfirm}
              onChange={setConfirmPw}
              onToggle={() => setShowConfirm((v) => !v)}
            />

            {pwError && (
              <div className="mb-3 px-3 py-2 bg-danger-b border border-danger rounded-[3px] text-danger text-[11px] font-mono">
                {pwError}
              </div>
            )}

            <div className="flex gap-2 mt-1">
              <button
                onClick={closeChangePw}
                disabled={pwChanging}
                className="flex-1 py-[8px] rounded-[3px] text-[11px] font-bold tracking-[0.06em] font-ui cursor-pointer bg-transparent border border-bd2 text-tx2 hover:text-tx transition-colors disabled:opacity-40"
              >
                CANCEL
              </button>
              <button
                onClick={handleChangePassword}
                disabled={pwChanging || !currentPw || !newPw || !confirmPw}
                className="flex-1 py-[8px] rounded-[3px] text-[11px] font-bold tracking-[0.06em] font-ui cursor-pointer bg-accent border-none text-[#020504] hover:opacity-90 transition-opacity disabled:opacity-40 flex items-center justify-center gap-1.5"
              >
                {pwChanging
                  ? <><div className="w-3 h-3 rounded-full border-2 border-transparent border-t-[#020504] animate-spin-fast" />CHANGING…</>
                  : 'CONFIRM CHANGE'}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Import Modal */}
      {importOpen && (
        <ImportModal onClose={() => setImportOpen(false)} />
      )}

      {/* Backup Modal */}
      {backupOpen && (
        <BackupModal onClose={() => setBackupOpen(false)} />
      )}
    </div>
  );
}
