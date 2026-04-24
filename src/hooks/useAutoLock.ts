import { useEffect, useRef } from 'react';
import { useVaultStore } from '../store';

const ACTIVITY_EVENTS = ['mousemove', 'keydown', 'mousedown', 'touchstart'] as const;

export function useAutoLock() {
  const screen      = useVaultStore((s) => s.screen);
  const lockTimeout = useVaultStore((s) => s.lockTimeout);
  const lock        = useVaultStore((s) => s.lock);

  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    if (screen === 'lock' || lockTimeout === 0) {
      if (timerRef.current) clearTimeout(timerRef.current);
      return;
    }

    const ms = lockTimeout * 60 * 1000;

    const reset = () => {
      if (timerRef.current) clearTimeout(timerRef.current);
      timerRef.current = setTimeout(() => lock(), ms);
    };

    ACTIVITY_EVENTS.forEach((e) => window.addEventListener(e, reset, { passive: true }));
    reset();

    return () => {
      ACTIVITY_EVENTS.forEach((e) => window.removeEventListener(e, reset));
      if (timerRef.current) clearTimeout(timerRef.current);
    };
  }, [screen, lockTimeout, lock]);
}
