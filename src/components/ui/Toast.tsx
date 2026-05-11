interface ToastProps {
  msg: string;
  type?: 'success' | 'error';
}

export function Toast({ msg, type = 'success' }: ToastProps) {
  const isError = type === 'error';
  return (
    <div
      className={[
        'fixed bottom-14 left-1/2 -translate-x-1/2',
        'rounded-[4px]',
        'px-[14px] py-[7px] text-[12px] font-mono',
        'z-[10000] pointer-events-none whitespace-nowrap',
        'shadow-[0_4px_20px_rgba(0,0,0,.6)]',
        'animate-toast-in',
        isError
          ? 'bg-danger-b border border-danger text-danger'
          : 'bg-raised border border-accent-d text-accent',
      ].join(' ')}
    >
      {msg}
    </div>
  );
}
