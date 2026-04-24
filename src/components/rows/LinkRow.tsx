import { CopyBtn } from '../ui/CopyBtn';
import { KebabBtn } from '../ui/KebabBtn';
import { CatDots } from './SecretRow';
import { useVaultStore } from '../../store';
import type { LinkItem, Category } from '../../types';

interface Props {
  item: LinkItem;
  cats: Category[];
}

export function LinkRow({ item, cats }: Props) {
  const go            = useVaultStore((s) => s.go);
  const setEditTarget = useVaultStore((s) => s.setEditTarget);
  const deleteItem    = useVaultStore((s) => s.deleteItem);

  const short = item.url.replace(/https?:\/\//, '').replace(/\/$/, '').slice(0, 48);

  const kebab = [
    { icon: 'edit',     label: 'Edit',             onClick: () => { setEditTarget(item); go('edit'); } },
    { icon: 'external', label: 'Open in browser',  onClick: () => {} },
    { divider: true },
    { icon: 'trash',    label: 'Delete', danger: true, onClick: () => deleteItem(item.id) },
  ];

  return (
    <div className="px-5 py-4 border-b border-bd hover:bg-raised transition-colors duration-100">
      <div className="flex items-center gap-3 mb-2">
        <span className="w-2 h-2 rounded-full shrink-0 bg-lnk" />
        <span className="flex-1 text-[13px] font-semibold text-tx overflow-hidden text-ellipsis whitespace-nowrap">
          {item.title}
        </span>
        <CatDots names={item.categories} cats={cats} />
        <CopyBtn value={item.url} label="URL" />
        <KebabBtn menuItems={kebab} />
      </div>
      <div className="pl-5 text-xs text-tx3 font-mono overflow-hidden text-ellipsis whitespace-nowrap mb-1.5">
        {short}
      </div>
      {item.description && (
        <div className="pl-5 text-xs text-tx2 overflow-hidden text-ellipsis whitespace-nowrap">
          {item.description}
        </div>
      )}
    </div>
  );
}
