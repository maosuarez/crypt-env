import { useState, useEffect, useRef, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { writeText } from '@tauri-apps/plugin-clipboard-manager';
import { Icon } from './ui/Icon';

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

type Method = 'lan' | 'file';
type LanRole = 'send' | 'receive';

type SessionState =
  | 'listening'
  | 'connecting'
  | 'awaiting_fingerprint'
  | 'active'
  | 'done'
  | 'failed'
  | 'cancelled';

interface PollResult {
  state: SessionState;
  fingerprint?: string;
  receivedNames?: string[];
  error?: string;
}

type Step =
  | 'method'
  | 'lan-send'
  | 'lan-receive'
  | 'fingerprint'
  | 'done-send'
  | 'done-receive'
  | 'done-file-export'
  | 'done-file-import'
  | 'failed'
  | 'file-export'
  | 'file-import';

export interface ShareModalProps {
  selectedIds: number[];
  onClose: () => void;
  onImportDone: () => void;
  onSendDone?: () => void;
}

// ---------------------------------------------------------------------------
// Small primitives
// ---------------------------------------------------------------------------

function SectionLabel({ children }: { children: React.ReactNode }) {
  return (
    <div className="text-[10px] font-mono text-tx3 tracking-[0.12em] uppercase mb-3">
      {children}
    </div>
  );
}

function Divider() {
  return <div className="border-t border-bd my-4" />;
}

function Spinner() {
  return (
    <div className="w-4 h-4 rounded-full border-2 border-bd2 border-t-accent animate-spin-fast shrink-0" />
  );
}

function BtnPrimary({
  children,
  onClick,
  disabled,
  className = '',
}: {
  children: React.ReactNode;
  onClick?: () => void;
  disabled?: boolean;
  className?: string;
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      className={[
        'flex items-center justify-center gap-2 rounded px-4 h-9',
        'bg-accent text-[#020504] text-[11px] font-bold tracking-wider font-ui',
        'cursor-pointer transition-opacity shrink-0',
        'disabled:opacity-30 disabled:cursor-not-allowed hover:opacity-90',
        className,
      ].join(' ')}
    >
      {children}
    </button>
  );
}

function BtnSecondary({
  children,
  onClick,
  disabled,
  className = '',
}: {
  children: React.ReactNode;
  onClick?: () => void;
  disabled?: boolean;
  className?: string;
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      className={[
        'flex items-center justify-center gap-2 rounded px-4 h-9',
        'border border-bd2 text-tx2 text-[11px] font-medium tracking-wider font-ui bg-transparent',
        'cursor-pointer transition-all duration-150 shrink-0',
        'disabled:opacity-30 disabled:cursor-not-allowed hover:border-tx3 hover:text-tx',
        className,
      ].join(' ')}
    >
      {children}
    </button>
  );
}

function BtnDanger({
  children,
  onClick,
  disabled,
}: {
  children: React.ReactNode;
  onClick?: () => void;
  disabled?: boolean;
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      className={[
        'flex items-center justify-center gap-2 rounded px-4 h-9',
        'bg-danger-b border border-danger text-danger',
        'text-[11px] font-bold tracking-wider font-ui',
        'cursor-pointer transition-opacity shrink-0',
        'disabled:opacity-30 disabled:cursor-not-allowed hover:opacity-80',
      ].join(' ')}
    >
      {children}
    </button>
  );
}

function InlineError({ msg }: { msg: string }) {
  return (
    <div className="text-[12px] text-danger font-mono bg-danger-b border border-danger rounded-[3px] px-3 py-2">
      {msg}
    </div>
  );
}

// Breadcrumb path indicator
function Breadcrumb({ path }: { path: string }) {
  return (
    <div className="text-[10px] font-mono text-tx3 tracking-[0.08em] mb-4">{path}</div>
  );
}

// 6-digit pairing code display
function PairingCodeDisplay({ code }: { code: string }) {
  const digits = code.replace(/\s/g, '').split('');
  return (
    <div className="flex items-center gap-2 justify-center my-4">
      {digits.map((d, i) => (
        <div
          key={i}
          className="w-9 h-11 bg-raised border border-bd2 rounded-[3px] flex items-center justify-center text-[24px] font-mono text-accent select-all"
        >
          {d}
        </div>
      ))}
    </div>
  );
}

// 6 individual input boxes for pairing code entry
function PairingCodeInput({
  value,
  onChange,
  onComplete,
}: {
  value: string;
  onChange: (v: string) => void;
  onComplete: () => void;
}) {
  const refs = useRef<(HTMLInputElement | null)[]>([]);
  const digits = Array.from({ length: 6 }, (_, i) => value[i] ?? '');

  const handleKey = (i: number, e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Backspace') {
      e.preventDefault();
      const next = value.slice(0, i) + value.slice(i + 1);
      onChange(next);
      if (i > 0) refs.current[i - 1]?.focus();
    }
  };

  const handleInput = (i: number, e: React.ChangeEvent<HTMLInputElement>) => {
    const raw = e.target.value.replace(/\D/g, '');
    if (!raw) return;
    const ch = raw[raw.length - 1];
    const next = (value.slice(0, i) + ch + value.slice(i + 1)).slice(0, 6);
    onChange(next);
    if (i < 5) {
      refs.current[i + 1]?.focus();
    } else if (next.length === 6) {
      onComplete();
    }
  };

  const handlePaste = (e: React.ClipboardEvent) => {
    const pasted = e.clipboardData.getData('text').replace(/\D/g, '').slice(0, 6);
    if (pasted.length > 0) {
      onChange(pasted);
      const focusIdx = Math.min(pasted.length, 5);
      refs.current[focusIdx]?.focus();
      if (pasted.length === 6) onComplete();
    }
    e.preventDefault();
  };

  return (
    <div className="flex items-center gap-2 justify-center my-4">
      {digits.map((d, i) => (
        <input
          key={i}
          ref={(el) => { refs.current[i] = el; }}
          type="text"
          inputMode="numeric"
          maxLength={1}
          value={d === ' ' ? '' : d}
          onChange={(e) => handleInput(i, e)}
          onKeyDown={(e) => handleKey(i, e)}
          onPaste={handlePaste}
          className={[
            'w-9 h-11 bg-raised border rounded-[3px] text-center text-[24px] font-mono text-accent',
            'outline-none transition-colors duration-100',
            d !== ' ' && d !== '' ? 'border-accent-d' : 'border-bd2',
            'focus:border-accent',
          ].join(' ')}
        />
      ))}
    </div>
  );
}

// Fingerprint display
function FingerprintDisplay({ fp }: { fp: string }) {
  return (
    <div className="bg-raised border border-bd2 rounded-[3px] px-4 py-3 text-center my-3">
      <span className="text-[18px] font-mono text-accent tracking-[0.15em] select-all">
        {fp}
      </span>
    </div>
  );
}

// Countdown timer component
function Countdown({ seconds }: { seconds: number }) {
  const [remaining, setRemaining] = useState(seconds);

  useEffect(() => {
    setRemaining(seconds);
    const id = setInterval(() => {
      setRemaining((r) => {
        if (r <= 1) { clearInterval(id); return 0; }
        return r - 1;
      });
    }, 1000);
    return () => clearInterval(id);
  }, [seconds]);

  const m = Math.floor(remaining / 60);
  const s = remaining % 60;
  const display = `${m}:${s.toString().padStart(2, '0')}`;
  const urgent = remaining < 60;

  return (
    <span className={['font-mono text-[12px]', urgent ? 'text-danger' : 'text-tx3'].join(' ')}>
      {display}
    </span>
  );
}

// ---------------------------------------------------------------------------
// Main modal
// ---------------------------------------------------------------------------

export function ShareModal({ selectedIds, onClose, onImportDone, onSendDone }: ShareModalProps) {
  const [step, setStep] = useState<Step>('method');
  const [method, setMethod] = useState<Method | null>(null);
  const [lanRole, setLanRole] = useState<LanRole | null>(null);

  // LAN send state
  const [pairingCode, setPairingCode] = useState('');
  // LAN receive state
  const [inputCode, setInputCode] = useState('');
  const [receiveStatus, setReceiveStatus] = useState<'idle' | 'loading' | 'connecting'>('idle');

  // Fingerprint state
  const [fingerprint, setFingerprint] = useState('');
  const [fpConfirming, setFpConfirming] = useState(false);

  // Done state
  const [receivedNames, setReceivedNames] = useState<string[]>([]);
  const [sentCount, setSentCount] = useState(0);

  // File export state
  const [exportPassphrase, setExportPassphrase] = useState('');
  const [exportLoading, setExportLoading] = useState(false);
  const [copiedPass, setCopiedPass] = useState(false);

  // File import state
  const [importPassphrase, setImportPassphrase] = useState('');
  const [importFileReady, setImportFileReady] = useState(false);
  const [importLoading, setImportLoading] = useState(false);
  const [importedNames, setImportedNames] = useState<string[]>([]);

  // Error state
  const [error, setError] = useState('');
  const [failedError, setFailedError] = useState('');

  // Polling
  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const stopPolling = useCallback(() => {
    if (pollRef.current !== null) {
      clearInterval(pollRef.current);
      pollRef.current = null;
    }
  }, []);

  useEffect(() => {
    return () => { stopPolling(); };
  }, [stopPolling]);

  const startPolling = useCallback(() => {
    stopPolling();
    pollRef.current = setInterval(async () => {
      try {
        const result = await invoke<PollResult>('share_poll_status');
        if (result.state === 'awaiting_fingerprint' && result.fingerprint) {
          setFingerprint(result.fingerprint);
          stopPolling();
          setStep('fingerprint');
        } else if (result.state === 'done') {
          stopPolling();
          if (result.receivedNames && result.receivedNames.length > 0) {
            setReceivedNames(result.receivedNames);
            setStep('done-receive');
          } else {
            setStep('done-send');
          }
        } else if (result.state === 'failed') {
          stopPolling();
          setFailedError(result.error ?? 'Transfer failed');
          setStep('failed');
        } else if (result.state === 'cancelled') {
          stopPolling();
          onClose();
        } else if (result.state === 'active') {
          // actively transferring — keep polling
        }
      } catch (e) {
        stopPolling();
        setFailedError(String(e));
        setStep('failed');
      }
    }, 800);
  }, [stopPolling, onClose]);

  // ------------------------------------------------------------------
  // Action handlers
  // ------------------------------------------------------------------

  const handleCancel = async () => {
    stopPolling();
    try { await invoke('share_cancel'); } catch { /* best effort */ }
    onClose();
  };


  const handleLanSend = async () => {
    setLanRole('send');
    setSentCount(selectedIds.length);
    setError('');
    try {
      const res = await invoke<{ pairingCode: string }>('share_start_send', { itemIds: selectedIds });
      setPairingCode(res.pairingCode);
      setStep('lan-send');
      startPolling();
    } catch (e) {
      setError(String(e));
    }
  };

  const handleLanReceive = () => {
    setLanRole('receive');
    setStep('lan-receive');
    setInputCode('');
    setReceiveStatus('idle');
  };

  const handleReceiveSubmit = async () => {
    if (inputCode.length < 6) return;
    setReceiveStatus('loading');
    setError('');
    try {
      await invoke<{ fingerprint: string }>('share_start_receive', { pairingCode: inputCode });
      setReceiveStatus('connecting');
      startPolling();
    } catch (e) {
      setError(String(e));
      setReceiveStatus('idle');
    }
  };

  const handleFingerprintConfirm = async (confirmed: boolean) => {
    setFpConfirming(true);
    try {
      await invoke('share_confirm_fingerprint', { confirmed });
      if (!confirmed) {
        onClose();
        return;
      }
      // If confirmed, start polling again for done/failed
      startPolling();
    } catch (e) {
      setError(String(e));
    } finally {
      setFpConfirming(false);
    }
  };

  const handleFileExport = async () => {
    setExportLoading(true);
    setError('');
    try {
      const res = await invoke<{ passphrase: string }>('share_export_file', { itemIds: selectedIds });
      setExportPassphrase(res.passphrase);
      setStep('done-file-export');
    } catch (e) {
      setError(String(e));
    } finally {
      setExportLoading(false);
    }
  };

  const handleFileImportOpen = async () => {
    setImportLoading(true);
    setError('');
    try {
      // The Rust side opens a native file dialog first, then waits for passphrase
      // We just trigger the flow — actual import happens on submit
      setImportFileReady(true);
    } finally {
      setImportLoading(false);
    }
  };

  const handleFileImportSubmit = async () => {
    if (!importPassphrase) return;
    setImportLoading(true);
    setError('');
    try {
      const res = await invoke<{ names: string[] }>('share_import_file', { passphrase: importPassphrase });
      setImportedNames(res.names);
      setStep('done-file-import');
    } catch (e) {
      setError(String(e));
    } finally {
      setImportLoading(false);
    }
  };

  const handleCopyPassphrase = async () => {
    try {
      await writeText(exportPassphrase);
      setCopiedPass(true);
      setTimeout(() => setCopiedPass(false), 2000);
    } catch { /* ignore */ }
  };

  const handleImportDoneClose = () => {
    onImportDone();
    onClose();
  };

  // ------------------------------------------------------------------
  // Render helpers
  // ------------------------------------------------------------------

  const hasItems = selectedIds.length > 0;

  // ------------------------------------------------------------------
  // Step renderers
  // ------------------------------------------------------------------

  function renderMethod() {
    return (
      <>
        <div className="flex gap-3 mb-4">
          {/* LAN Card */}
          <button
            onClick={() => { setMethod('lan'); setError(''); }}
            className={[
              'flex-1 border rounded-[4px] p-4 text-left transition-all duration-150 cursor-pointer',
              method === 'lan'
                ? 'bg-accent-b border-accent-d'
                : 'bg-raised border-bd hover:border-bd2',
            ].join(' ')}
          >
            <div className="flex items-center gap-2 mb-2">
              <Icon name="globe" size={15} color={method === 'lan' ? 'oklch(0.70 0.17 162)' : '#6b7899'} />
              <span className={['text-[12px] font-bold tracking-wider font-ui', method === 'lan' ? 'text-accent' : 'text-tx'].join(' ')}>
                LAN
              </span>
            </div>
            <div className="text-[11px] text-tx2 leading-[1.5]">Share on local network</div>
            <div className="text-[10px] text-tx3 mt-1 font-mono">peer-to-peer · end-to-end encrypted</div>
          </button>

          {/* File Card */}
          <button
            onClick={() => { setMethod('file'); setError(''); }}
            className={[
              'flex-1 border rounded-[4px] p-4 text-left transition-all duration-150 cursor-pointer',
              method === 'file'
                ? 'bg-accent-b border-accent-d'
                : 'bg-raised border-bd hover:border-bd2',
            ].join(' ')}
          >
            <div className="flex items-center gap-2 mb-2">
              <Icon name="export" size={15} color={method === 'file' ? 'oklch(0.70 0.17 162)' : '#6b7899'} />
              <span className={['text-[12px] font-bold tracking-wider font-ui', method === 'file' ? 'text-accent' : 'text-tx'].join(' ')}>
                FILE
              </span>
            </div>
            <div className="text-[11px] text-tx2 leading-[1.5]">Export / Import file</div>
            <div className="text-[10px] text-tx3 mt-1 font-mono">encrypted package · passphrase</div>
          </button>
        </div>

        {/* Item badge */}
        {hasItems ? (
          <div className="flex items-center gap-2 mb-4">
            <span className="w-1.5 h-1.5 rounded-full bg-accent shrink-0" />
            <span className="text-[11px] text-tx2 font-mono">
              Sharing <span className="text-accent font-bold">{selectedIds.length}</span> item{selectedIds.length !== 1 ? 's' : ''}
            </span>
          </div>
        ) : (
          <div className="flex items-start gap-2 mb-4 bg-raised border border-bd rounded-[3px] px-3 py-2.5">
            <span className="text-warn shrink-0 mt-0.5">
              <Icon name="note" size={12} color="oklch(0.72 0.16 68)" />
            </span>
            <span className="text-[11px] text-tx3 leading-[1.5]">
              No items selected. Send and Export require a selection. Receive and Import are always available.
            </span>
          </div>
        )}

        {/* LAN sub-actions */}
        {method === 'lan' && (
          <div className="flex gap-2">
            <BtnPrimary onClick={handleLanSend} disabled={!hasItems} className="flex-1">
              <Icon name="export" size={12} color="#020504" />
              SEND
            </BtnPrimary>
            <BtnSecondary onClick={handleLanReceive} className="flex-1">
              <Icon name="back" size={12} />
              RECEIVE
            </BtnSecondary>
          </div>
        )}

        {/* FILE sub-actions */}
        {method === 'file' && (
          <div className="flex gap-2">
            <BtnPrimary onClick={() => setStep('file-export')} disabled={!hasItems} className="flex-1">
              <Icon name="export" size={12} color="#020504" />
              EXPORT
            </BtnPrimary>
            <BtnSecondary onClick={() => setStep('file-import')} className="flex-1">
              <Icon name="external" size={12} />
              IMPORT
            </BtnSecondary>
          </div>
        )}

        {error && <div className="mt-3"><InlineError msg={error} /></div>}
      </>
    );
  }

  function renderLanSend() {
    return (
      <>
        <Breadcrumb path="LAN  /  SEND" />
        <SectionLabel>Waiting for peer</SectionLabel>
        <div className="flex items-center gap-2 text-[11px] text-tx3 font-mono mb-1">
          <Spinner />
          <span>Listening for incoming connection…</span>
        </div>
        <PairingCodeDisplay code={pairingCode} />
        <div className="text-center text-[11px] text-tx3 font-mono mb-1">
          Share this code with the recipient
        </div>
        <div className="flex items-center justify-center gap-1.5 mb-4">
          <span className="text-[11px] text-tx3 font-mono">Connection timeout:</span>
          <Countdown seconds={300} />
        </div>
        <Divider />
        <div className="flex justify-end">
          <BtnSecondary onClick={handleCancel}>CANCEL</BtnSecondary>
        </div>
      </>
    );
  }

  function renderLanReceive() {
    const isConnecting = receiveStatus === 'connecting';
    const isLoading    = receiveStatus === 'loading';

    return (
      <>
        <Breadcrumb path="LAN  /  RECEIVE" />
        <SectionLabel>Enter pairing code</SectionLabel>
        <PairingCodeInput
          value={inputCode}
          onChange={setInputCode}
          onComplete={() => { if (!isConnecting && !isLoading) handleReceiveSubmit(); }}
        />
        {isConnecting && (
          <div className="flex items-center justify-center gap-2 text-[11px] text-tx3 font-mono mb-2">
            <Spinner />
            <span>Connecting…</span>
          </div>
        )}
        {error && <div className="mb-3"><InlineError msg={error} /></div>}
        <Divider />
        <div className="flex justify-between">
          <BtnSecondary onClick={() => { setStep('method'); setError(''); }}>BACK</BtnSecondary>
          <BtnPrimary
            onClick={handleReceiveSubmit}
            disabled={inputCode.length < 6 || isLoading || isConnecting}
          >
            {isLoading ? <Spinner /> : null}
            CONNECT
          </BtnPrimary>
        </div>
      </>
    );
  }

  function renderFingerprint() {
    return (
      <>
        <Breadcrumb path={`LAN  /  ${lanRole === 'send' ? 'SEND' : 'RECEIVE'}  /  VERIFY`} />
        <SectionLabel>Verify fingerprint</SectionLabel>
        <p className="text-[12px] text-tx2 mb-3 leading-[1.6]">
          Confirm that both devices show the same fingerprint before proceeding:
        </p>
        <FingerprintDisplay fp={fingerprint} />
        <p className="text-[11px] text-tx3 font-mono mt-2 mb-4 leading-[1.5]">
          If they match, you are connected to the right device.
          If they differ, reject immediately.
        </p>
        {error && <div className="mb-3"><InlineError msg={error} /></div>}
        <Divider />
        <div className="flex gap-2 justify-between">
          <BtnDanger
            onClick={() => handleFingerprintConfirm(false)}
            disabled={fpConfirming}
          >
            REJECT
          </BtnDanger>
          <BtnPrimary
            onClick={() => handleFingerprintConfirm(true)}
            disabled={fpConfirming}
          >
            {fpConfirming ? <Spinner /> : <Icon name="check" size={12} color="#020504" />}
            CONFIRM
          </BtnPrimary>
        </div>
      </>
    );
  }

  function renderFileExport() {
    return (
      <>
        <Breadcrumb path="FILE  /  EXPORT" />
        <SectionLabel>Export encrypted file</SectionLabel>
        <p className="text-[12px] text-tx2 mb-4 leading-[1.6]">
          A native save dialog will open. The file is AES-256-GCM encrypted.
          A random passphrase will be generated — share it with the recipient via a separate channel.
        </p>
        <div className="flex items-center gap-2 mb-4">
          <span className="w-1.5 h-1.5 rounded-full bg-accent shrink-0" />
          <span className="text-[11px] text-tx2 font-mono">
            {selectedIds.length} item{selectedIds.length !== 1 ? 's' : ''} selected for export
          </span>
        </div>
        {error && <div className="mb-3"><InlineError msg={error} /></div>}
        <Divider />
        <div className="flex justify-between">
          <BtnSecondary onClick={() => { setStep('method'); setError(''); }}>BACK</BtnSecondary>
          <BtnPrimary onClick={handleFileExport} disabled={exportLoading}>
            {exportLoading ? <Spinner /> : <Icon name="export" size={12} color="#020504" />}
            SAVE ENCRYPTED FILE
          </BtnPrimary>
        </div>
      </>
    );
  }

  function renderFileImport() {
    return (
      <>
        <Breadcrumb path="FILE  /  IMPORT" />
        <SectionLabel>Import encrypted file</SectionLabel>
        <p className="text-[12px] text-tx2 mb-4 leading-[1.6]">
          Select the encrypted package file, then enter the passphrase you received.
        </p>

        {!importFileReady ? (
          <BtnSecondary onClick={handleFileImportOpen} disabled={importLoading} className="w-full mb-4">
            {importLoading ? <Spinner /> : <Icon name="external" size={12} />}
            OPEN PACKAGE FILE
          </BtnSecondary>
        ) : (
          <>
            <div className="flex items-center gap-2 mb-3">
              <Icon name="check" size={12} color="oklch(0.70 0.17 162)" />
              <span className="text-[11px] text-accent font-mono">File selected</span>
            </div>
            <div className="mb-4">
              <label className="block text-[10px] font-mono text-tx3 tracking-[0.08em] mb-1.5">
                PASSPHRASE
              </label>
              <input
                type="text"
                value={importPassphrase}
                onChange={(e) => setImportPassphrase(e.target.value)}
                onKeyDown={(e) => { if (e.key === 'Enter') handleFileImportSubmit(); }}
                placeholder="word-word-word-word"
                className={[
                  'w-full h-9 bg-raised border border-bd2 rounded-[3px] px-3',
                  'text-[13px] font-mono text-tx placeholder:text-tx4',
                  'outline-none focus:border-accent transition-colors duration-150',
                ].join(' ')}
              />
            </div>
          </>
        )}

        {error && <div className="mb-3"><InlineError msg={error} /></div>}
        <Divider />
        <div className="flex justify-between">
          <BtnSecondary onClick={() => { setStep('method'); setError(''); setImportFileReady(false); setImportPassphrase(''); }}>
            BACK
          </BtnSecondary>
          <BtnPrimary
            onClick={handleFileImportSubmit}
            disabled={!importFileReady || !importPassphrase || importLoading}
          >
            {importLoading ? <Spinner /> : null}
            IMPORT
          </BtnPrimary>
        </div>
      </>
    );
  }

  function renderDoneSend() {
    return (
      <>
        <Breadcrumb path="LAN  /  SEND  /  DONE" />
        <div className="py-6 text-center">
          <div className="w-10 h-10 rounded-full bg-accent-b border border-accent-d flex items-center justify-center mx-auto mb-4">
            <Icon name="check" size={18} color="oklch(0.70 0.17 162)" />
          </div>
          <div className="text-[13px] font-semibold text-tx mb-1">Transfer complete</div>
          <div className="text-[12px] text-tx3 font-mono">
            {sentCount} item{sentCount !== 1 ? 's' : ''} sent successfully
          </div>
        </div>
        <Divider />
        <div className="flex justify-end">
          <BtnPrimary onClick={() => { onSendDone?.(); onClose(); }}>CLOSE</BtnPrimary>
        </div>
      </>
    );
  }

  function renderDoneReceive() {
    return (
      <>
        <Breadcrumb path="LAN  /  RECEIVE  /  DONE" />
        <div className="py-4">
          <div className="flex items-center gap-2 mb-3">
            <div className="w-7 h-7 rounded-full bg-accent-b border border-accent-d flex items-center justify-center shrink-0">
              <Icon name="check" size={13} color="oklch(0.70 0.17 162)" />
            </div>
            <div className="text-[13px] font-semibold text-tx">Transfer complete</div>
          </div>
          <div className="text-[11px] text-tx3 font-mono mb-3">
            {receivedNames.length} item{receivedNames.length !== 1 ? 's' : ''} received:
          </div>
          <div className="bg-raised border border-bd rounded-[3px] divide-y divide-bd max-h-[120px] overflow-y-auto">
            {receivedNames.map((name, i) => (
              <div key={i} className="px-3 py-2 text-[12px] font-mono text-tx2 flex items-center gap-2">
                <span className="w-1.5 h-1.5 rounded-full bg-accent shrink-0" />
                {name}
              </div>
            ))}
          </div>
        </div>
        <Divider />
        <div className="flex justify-end">
          <BtnPrimary onClick={handleImportDoneClose}>
            <Icon name="check" size={12} color="#020504" />
            RELOAD VAULT
          </BtnPrimary>
        </div>
      </>
    );
  }

  function renderDoneFileExport() {
    return (
      <>
        <Breadcrumb path="FILE  /  EXPORT  /  DONE" />
        <div className="flex items-center gap-2 mb-4">
          <div className="w-7 h-7 rounded-full bg-accent-b border border-accent-d flex items-center justify-center shrink-0">
            <Icon name="check" size={13} color="oklch(0.70 0.17 162)" />
          </div>
          <div className="text-[13px] font-semibold text-tx">File saved</div>
        </div>

        <SectionLabel>Passphrase — share via separate channel</SectionLabel>
        <div className="bg-raised border border-bd2 rounded-[3px] px-4 py-3 flex items-center gap-3 mb-3">
          <span className="flex-1 font-mono text-[13px] text-accent tracking-[0.05em] select-all break-all">
            {exportPassphrase}
          </span>
          <button
            onClick={handleCopyPassphrase}
            title="Copy passphrase"
            className={[
              'flex items-center gap-1.5 border rounded px-2 py-1',
              'text-[10px] font-mono tracking-wide shrink-0 transition-all duration-150 cursor-pointer',
              copiedPass
                ? 'bg-accent-b border-accent-d text-accent'
                : 'border-bd2 text-tx3 bg-transparent hover:border-tx3 hover:text-tx',
            ].join(' ')}
          >
            <Icon name={copiedPass ? 'check' : 'copy'} size={11} color={copiedPass ? 'oklch(0.70 0.17 162)' : 'currentColor'} />
            {copiedPass ? 'COPIED' : 'COPY'}
          </button>
        </div>
        <p className="text-[11px] text-tx3 font-mono leading-[1.5]">
          The recipient will need this passphrase to decrypt the file.
          Do not share it in the same channel as the file.
        </p>
        <Divider />
        <div className="flex justify-end">
          <BtnPrimary onClick={onClose}>DONE</BtnPrimary>
        </div>
      </>
    );
  }

  function renderDoneFileImport() {
    return (
      <>
        <Breadcrumb path="FILE  /  IMPORT  /  DONE" />
        <div className="flex items-center gap-2 mb-4">
          <div className="w-7 h-7 rounded-full bg-accent-b border border-accent-d flex items-center justify-center shrink-0">
            <Icon name="check" size={13} color="oklch(0.70 0.17 162)" />
          </div>
          <div className="text-[13px] font-semibold text-tx">Import complete</div>
        </div>
        <div className="text-[11px] text-tx3 font-mono mb-3">
          {importedNames.length} item{importedNames.length !== 1 ? 's' : ''} imported:
        </div>
        <div className="bg-raised border border-bd rounded-[3px] divide-y divide-bd max-h-[120px] overflow-y-auto mb-4">
          {importedNames.map((name, i) => (
            <div key={i} className="px-3 py-2 text-[12px] font-mono text-tx2 flex items-center gap-2">
              <span className="w-1.5 h-1.5 rounded-full bg-accent shrink-0" />
              {name}
            </div>
          ))}
        </div>
        <Divider />
        <div className="flex justify-end">
          <BtnPrimary onClick={handleImportDoneClose}>
            <Icon name="check" size={12} color="#020504" />
            RELOAD VAULT
          </BtnPrimary>
        </div>
      </>
    );
  }

  function renderFailed() {
    return (
      <>
        <Breadcrumb path="ERROR" />
        <div className="py-6 text-center">
          <div className="w-10 h-10 rounded-full bg-danger-b border border-danger flex items-center justify-center mx-auto mb-4">
            <Icon name="close" size={18} color="oklch(0.62 0.20 22)" />
          </div>
          <div className="text-[13px] font-semibold text-tx mb-2">Transfer failed</div>
          <div className="text-[12px] text-danger font-mono bg-danger-b border border-danger rounded-[3px] px-3 py-2 mt-2 text-left">
            {failedError}
          </div>
        </div>
        <Divider />
        <div className="flex justify-between">
          <BtnSecondary onClick={onClose}>CLOSE</BtnSecondary>
          <BtnPrimary onClick={() => { setStep('method'); setMethod(null); setError(''); setFailedError(''); }}>
            RETRY
          </BtnPrimary>
        </div>
      </>
    );
  }

  // ------------------------------------------------------------------
  // Body dispatch
  // ------------------------------------------------------------------

  function renderBody() {
    switch (step) {
      case 'method':          return renderMethod();
      case 'lan-send':        return renderLanSend();
      case 'lan-receive':     return renderLanReceive();
      case 'fingerprint':     return renderFingerprint();
      case 'file-export':     return renderFileExport();
      case 'file-import':     return renderFileImport();
      case 'done-send':       return renderDoneSend();
      case 'done-receive':    return renderDoneReceive();
      case 'done-file-export':return renderDoneFileExport();
      case 'done-file-import':return renderDoneFileImport();
      case 'failed':          return renderFailed();
      default:                return null;
    }
  }

  // ------------------------------------------------------------------
  // Outer shell
  // ------------------------------------------------------------------

  return (
    <div className="absolute inset-0 bg-black/70 flex items-center justify-center z-20 p-5">
      <div className="bg-surface border border-bd rounded-[4px] p-4 w-full animate-fade-in">
        {/* Modal header */}
        <div className="flex items-center justify-between mb-4">
          <div className="flex items-center gap-2">
            <Icon name="shield" size={14} color="oklch(0.70 0.17 162)" />
            <span className="text-[12px] font-bold tracking-wider font-ui text-tx">SHARE</span>
          </div>
          <button
            onClick={step === 'method' ? onClose : handleCancel}
            className="flex items-center justify-center w-6 h-6 rounded text-tx3 hover:text-tx hover:bg-raised transition-all duration-150 cursor-pointer border-none bg-transparent"
          >
            <Icon name="close" size={13} />
          </button>
        </div>

        {/* Step content */}
        {renderBody()}
      </div>
    </div>
  );
}
