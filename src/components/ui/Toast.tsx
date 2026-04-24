interface ToastProps {
  msg: string;
}

export function Toast({ msg }: ToastProps) {
  return (
    <div
      className={[
        'fixed bottom-14 left-1/2 -translate-x-1/2',
        'bg-raised border border-accent-d rounded-[4px]',
        'px-[14px] py-[7px] text-[12px] text-accent font-mono',
        'z-[10000] pointer-events-none whitespace-nowrap',
        'shadow-[0_4px_20px_rgba(0,0,0,.6)]',
        'animate-toast-in',
      ].join(' ')}
    >
      {msg}
    </div>
  );
}
