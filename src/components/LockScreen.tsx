import { useState, useRef, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { Icon } from './ui/Icon';
import { useVaultStore } from '../store';
import type { VaultItem, Category } from '../types';

export function LockScreen() {
  const unlock             = useVaultStore((s) => s.unlock);
  const unlockWithPayload  = useVaultStore((s) => s.unlockWithPayload);

  const [pw,           setPw]           = useState('');
  const [loading,      setLoading]      = useState(false);
  const [error,        setError]        = useState(false);
  const [show,         setShow]         = useState(false);
  const [isSetup,      setIsSetup]      = useState<boolean | null>(null);
  const [bioAvailable, setBioAvailable] = useState(false);
  const [bioLoading,   setBioLoading]   = useState(false);
  const [bioError,     setBioError]     = useState('');
  const ref = useRef<HTMLInputElement>(null);

  useEffect(() => {
    ref.current?.focus();
    invoke<boolean>('vault_is_setup').then(setIsSetup).catch(() => setIsSetup(false));

    invoke<string>('biometric_check').then((status) => {
      if (status === 'available') {
        invoke<boolean>('biometric_is_enrolled').then((enrolled) => {
          setBioAvailable(enrolled);
        }).catch(() => {});
      }
    }).catch(() => {});
  }, []);

  const handle = async () => {
    if (!pw.trim()) return;
    setLoading(true);
    setError(false);
    try {
      await unlock(pw);
    } catch {
      setError(true);
      setTimeout(() => setError(false), 600);
    } finally {
      setLoading(false);
    }
  };

  const handleBiometric = async () => {
    setBioLoading(true);
    setBioError('');
    try {
      const payload = await invoke<{ items: VaultItem[]; categories: Category[] }>('biometric_unlock');
      await getCurrentWindow().setFocus();
      await unlockWithPayload(payload);
    } catch (e: unknown) {
      await getCurrentWindow().setFocus();
      setBioError(e instanceof Error ? e.message : String(e));
      setTimeout(() => setBioError(''), 3000);
    } finally {
      setBioLoading(false);
    }
  };

  const btnLabel = isSetup === false ? 'CREATE VAULT' : 'UNLOCK VAULT';

  return (
    <div className="flex-1 flex flex-col bg-bg animate-fade-in">
      {/* Top spacer — pushes content toward vertical center */}
      <div className="flex-1" />

      {/* Content block — fixed max width, centered horizontally */}
      <div className="flex flex-col items-center px-10" style={{ maxWidth: '460px', margin: '0 auto', width: '100%' }}>

        {/* ── SECTION 1: Branding ── */}
        <div className="flex flex-col items-center gap-3">
          <div className="relative">
            <div className="w-[60px] h-[60px] border border-bd2 rounded-[4px] flex items-center justify-center bg-raised">
              <Icon name="shield" size={28} color="oklch(0.70 0.17 162)" />
            </div>
            <div className="absolute inset-[-1px] rounded-[4px] pointer-events-none shadow-[0_0_20px_-4px_color-mix(in_oklch,oklch(0.70_0.17_162)_40%,transparent)]" />
          </div>
          <div className="text-[22px] font-bold tracking-[-0.01em] text-tx">CryptEnv</div>
          <div className="text-[11px] text-tx3 font-mono tracking-[0.06em]">ENCRYPTED LOCAL STORE</div>
        </div>

        {/* ── SECTION 2: Form ── */}
        <div className="flex flex-col gap-5 w-full" style={{ marginTop: '64px' }}>
          {/* Label + Input */}
          <div className="flex flex-col gap-2">
            <div className={`text-[11px] font-medium tracking-[0.07em] ${error ? 'text-danger' : 'text-tx3'}`}>
              {isSetup === false ? 'SET MASTER PASSWORD' : 'MASTER PASSWORD'}
            </div>
            <div
              className={[
                'flex items-center h-13 border rounded-[3px] bg-raised transition-[border-color] duration-150',
                error ? 'border-danger animate-shake' : 'border-bd2',
              ].join(' ')}
            >
              <input
                ref={ref}
                type={show ? 'text' : 'password'}
                value={pw}
                onChange={(e) => setPw(e.target.value)}
                onKeyDown={(e) => e.key === 'Enter' && handle()}
                placeholder={isSetup === false ? 'Choose a strong password…' : 'Enter master password…'}
                className={[
                  'flex-1 h-full pl-6 pr-2 text-[15px] text-tx bg-transparent border-none outline-none',
                  show ? 'font-mono tracking-[0.02em]' : 'font-ui tracking-[0.05em]',
                ].join(' ')}
              />
              <button
                onClick={() => setShow((v) => !v)}
                className="pl-4 pr-6 bg-transparent border-none cursor-pointer h-full text-tx3 flex items-center hover:text-tx transition-colors"
              >
                <Icon name={show ? 'eyeOff' : 'eye'} size={16} />
              </button>
            </div>
            {error && (
              <div className="text-[11px] text-danger font-mono">// incorrect password</div>
            )}
          </div>

          {/* Button */}
          <button
            onClick={handle}
            disabled={loading}
            className={[
              'w-full h-[52px] border-none rounded-[3px]',
              'text-[14px] font-bold tracking-[0.07em] font-ui',
              'flex items-center justify-center gap-2 transition-all duration-150 cursor-pointer',
              loading ? 'bg-accent-d text-[#020504]' : 'bg-accent text-[#020504] hover:opacity-90',
            ].join(' ')}
          >
            {loading ? (
              <>
                <div className="w-3 h-3 rounded-full border-2 border-transparent border-t-[#020504] animate-spin-fast" />
                {isSetup === false ? 'CREATING…' : 'UNLOCKING…'}
              </>
            ) : (
              <>
                <Icon name="unlock" size={14} color="#020504" />
                {btnLabel}
              </>
            )}
          </button>

          {/* Windows Hello button — only shown when biometric is enrolled */}
          {bioAvailable && (
            <div className="flex flex-col gap-1">
              <button
                onClick={handleBiometric}
                disabled={bioLoading}
                className={[
                  'w-full h-[44px] rounded-[3px] border',
                  'text-[13px] font-semibold tracking-[0.05em] font-ui',
                  'flex items-center justify-center gap-2 transition-all duration-150 cursor-pointer',
                  'bg-raised border-bd2 text-tx2 hover:border-accent-d hover:text-tx',
                  bioLoading ? 'opacity-70' : '',
                ].join(' ')}
              >
                {bioLoading ? (
                  <>
                    <div className="w-3 h-3 rounded-full border-2 border-transparent border-t-current animate-spin-fast" />
                    VERIFYING…
                  </>
                ) : (
                  <>
                    <Icon name="fingerprint" size={15} />
                    UNLOCK WITH WINDOWS HELLO
                  </>
                )}
              </button>
              {bioError && (
                <div className="text-[11px] text-danger font-mono">// {bioError}</div>
              )}
            </div>
          )}
        </div>

        {/* ── SECTION 3: Footer ── */}
        <div className="text-[11px] text-tx3 text-center leading-[2]" style={{ marginTop: '56px' }}>
          AES-256-GCM · Argon2id key derivation
          <br />
          <span className="font-mono text-[10px]">vault v2.0.0 · local only</span>
        </div>

      </div>

      {/* Bottom spacer — mirrors top, keeps content centered */}
      <div className="flex-1" />
    </div>
  );
}
