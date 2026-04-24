import { useState } from 'react';
import { Icon } from '../ui/Icon';
import { CopyBtn } from '../ui/CopyBtn';
import { KebabBtn } from '../ui/KebabBtn';
import { CatDots } from './SecretRow';
import { useVaultStore } from '../../store';
import type { CredentialItem, Category } from '../../types';

interface Props {
  item: CredentialItem;
  cats: Category[];
}

export function CredentialRow({ item, cats }: Props) {
  const [showPw, setShowPw] = useState(false);
  const go            = useVaultStore((s) => s.go);
  const setEditTarget = useVaultStore((s) => s.setEditTarget);
  const deleteItem    = useVaultStore((s) => s.deleteItem);

  const domain = item.url?.replace(/https?:\/\//, '').split('/')[0] ?? '';

  const kebab = [
    { icon: 'edit',     label: 'Edit',         onClick: () => { setEditTarget(item); go('edit'); } },
    { icon: 'external', label: 'Open URL',      onClick: () => {} },
    { divider: true },
    { icon: 'trash',    label: 'Delete', danger: true, onClick: () => deleteItem(item.id) },
  ];

  return (
    <div className="px-5 py-4 border-b border-bd hover:bg-raised transition-colors duration-100">
      {/* Name row */}
      <div className="flex items-center gap-3 mb-2">
        <span className="w-2 h-2 rounded-full shrink-0 bg-cred" />
        <span className="flex-1 text-[13px] font-semibold text-tx overflow-hidden text-ellipsis whitespace-nowrap">
          {item.name}
        </span>
        <CatDots names={item.categories} cats={cats} />
        <KebabBtn menuItems={kebab} />
      </div>
      {domain && (
        <div className="pl-5 text-xs text-tx3 font-mono mb-2 overflow-hidden text-ellipsis whitespace-nowrap">
          {domain}
        </div>
      )}
      <div className="pl-5 flex flex-col gap-2">
        {/* Username */}
        <div className="flex items-center gap-2">
          <Icon name="person" size={12} color="#4a5268" />
          <span className="flex-1 text-xs font-mono text-tx2 overflow-hidden text-ellipsis whitespace-nowrap">
            {item.username}
          </span>
          <CopyBtn value={item.username} label="USER" title="Copy username" />
        </div>
        {/* Password */}
        <div className="flex items-center gap-2">
          <Icon name="key" size={12} color="#4a5268" />
          <span
            className={[
              'flex-1 text-xs font-mono overflow-hidden text-ellipsis whitespace-nowrap',
              showPw ? 'text-tx tracking-[0.02em]' : 'text-tx3 tracking-[0.12em]',
            ].join(' ')}
          >
            {showPw ? item.password : '••••••••••••'}
          </span>
          <button
            onClick={(e) => { e.stopPropagation(); setShowPw((v) => !v); }}
            className="bg-transparent border-none cursor-pointer text-tx3 flex p-0.5 hover:text-tx transition-colors"
          >
            <Icon name={showPw ? 'eyeOff' : 'eye'} size={13} />
          </button>
          <CopyBtn value={item.password} label="PASS" title="Copy password" />
        </div>
      </div>
      {item.notes && (
        <div className="mt-2 pl-5 text-xs text-tx3 overflow-hidden text-ellipsis whitespace-nowrap italic">
          {item.notes}
        </div>
      )}
    </div>
  );
}
