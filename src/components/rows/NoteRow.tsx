import { KebabBtn } from '../ui/KebabBtn';
import { CatDots } from './SecretRow';
import { useVaultStore } from '../../store';
import type { NoteItem, Category } from '../../types';

interface Props {
  item: NoteItem;
  cats: Category[];
}

export function NoteRow({ item, cats }: Props) {
  const go            = useVaultStore((s) => s.go);
  const setEditTarget = useVaultStore((s) => s.setEditTarget);
  const deleteItem    = useVaultStore((s) => s.deleteItem);

  const preview = item.content.replace(/\n+/g, ' ').slice(0, 80) + (item.content.length > 80 ? '…' : '');

  const kebab = [
    { icon: 'edit',  label: 'Edit',                  onClick: () => { setEditTarget(item); go('edit'); } },
    { divider: true },
    { icon: 'trash', label: 'Delete', danger: true,  onClick: () => deleteItem(item.id) },
  ];

  return (
    <div className="px-5 py-4 border-b border-bd hover:bg-raised transition-colors duration-100">
      <div className="flex items-center gap-3 mb-2">
        <span className="w-2 h-2 rounded-full shrink-0" style={{ background: 'oklch(0.72 0.15 350)' }} />
        <span className="flex-1 text-[13px] font-semibold text-tx overflow-hidden text-ellipsis whitespace-nowrap">
          {item.title}
        </span>
        <CatDots names={item.categories} cats={cats} />
        <KebabBtn menuItems={kebab} />
      </div>
      {preview && (
        <div className="pl-5 text-xs text-tx3 overflow-hidden text-ellipsis whitespace-nowrap leading-[1.5] italic">
          {preview}
        </div>
      )}
    </div>
  );
}
