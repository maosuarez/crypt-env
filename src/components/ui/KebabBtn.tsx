import { useRef } from 'react';
import { Icon } from './Icon';
import { useVaultStore } from '../../store';
import type { ContextMenuItemDef } from '../../types';

interface KebabBtnProps {
  menuItems: ContextMenuItemDef[];
}

export function KebabBtn({ menuItems }: KebabBtnProps) {
  const ref = useRef<HTMLButtonElement>(null);
  const openMenu = useVaultStore((s) => s.openMenu);

  const handle = (e: React.MouseEvent) => {
    e.stopPropagation();
    const r = ref.current!.getBoundingClientRect();
    openMenu({ x: r.right, y: r.bottom, items: menuItems });
  };

  return (
    <button
      ref={ref}
      onClick={handle}
      className={[
        'flex items-center shrink-0 rounded px-1 py-1',
        'border border-bd bg-transparent text-tx3 cursor-pointer',
        'hover:border-bd2 hover:text-tx transition-all duration-150',
      ].join(' ')}
    >
      <Icon name="more" size={14} />
    </button>
  );
}
