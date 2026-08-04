import { useState } from 'react';
import { Icon } from './Icon';

/**
 * Two-box "code" + "passphrase" display with copy buttons, used after a
 * successful relay upload. Extracted out of `ShareModal.tsx`'s internet-send
 * "done" step so `ProjectShareModal.tsx` (whole-project relay share, issue
 * #4) doesn't duplicate the same markup — behavior is unchanged from the
 * original inline version.
 */
export function RelayCodeDisplay({ code, passphrase }: { code: string; passphrase: string }) {
  const [copiedCode, setCopiedCode] = useState(false);
  const [copiedPass, setCopiedPass] = useState(false);

  const copyCode = () => {
    navigator.clipboard.writeText(code);
    setCopiedCode(true);
    setTimeout(() => setCopiedCode(false), 2000);
  };
  const copyPass = () => {
    navigator.clipboard.writeText(passphrase);
    setCopiedPass(true);
    setTimeout(() => setCopiedPass(false), 2000);
  };

  return (
    <>
      <div className="mb-3">
        <div className="text-[9px] font-mono text-tx3 tracking-[0.08em] mb-1">CODE</div>
        <div className="bg-raised border border-bd2 rounded-[3px] px-4 py-2.5 flex items-center gap-3">
          <span className="flex-1 font-mono text-[18px] text-accent tracking-[0.3em] font-bold select-all">{code}</span>
          <button
            onClick={copyCode}
            className={[
              'flex items-center gap-1.5 border rounded px-2 py-1 text-[10px] font-mono tracking-wide transition-all cursor-pointer',
              copiedCode ? 'bg-accent-b border-accent-d text-accent' : 'border-bd2 text-tx3 bg-transparent hover:border-tx3',
            ].join(' ')}
          >
            <Icon name={copiedCode ? 'check' : 'copy'} size={10} color={copiedCode ? 'oklch(0.70 0.17 162)' : 'currentColor'} />
            {copiedCode ? 'COPIED' : 'COPY'}
          </button>
        </div>
      </div>

      <div className="mb-4">
        <div className="text-[9px] font-mono text-tx3 tracking-[0.08em] mb-1">PASSPHRASE</div>
        <div className="bg-raised border border-bd2 rounded-[3px] px-4 py-2.5 flex items-center gap-3">
          <span className="flex-1 font-mono text-[13px] text-accent tracking-[0.05em] select-all break-all">{passphrase}</span>
          <button
            onClick={copyPass}
            className={[
              'flex items-center gap-1.5 border rounded px-2 py-1 text-[10px] font-mono tracking-wide transition-all cursor-pointer',
              copiedPass ? 'bg-accent-b border-accent-d text-accent' : 'border-bd2 text-tx3 bg-transparent hover:border-tx3',
            ].join(' ')}
          >
            <Icon name={copiedPass ? 'check' : 'copy'} size={10} color={copiedPass ? 'oklch(0.70 0.17 162)' : 'currentColor'} />
            {copiedPass ? 'COPIED' : 'COPY'}
          </button>
        </div>
      </div>

      <p className="text-[10px] text-tx3 font-mono leading-[1.5] mb-3">
        The relay link expires in 24 hours and is destroyed after first use. Never share code + passphrase in the same message.
      </p>
    </>
  );
}
