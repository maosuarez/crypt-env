import { CopyBtn } from '../ui/CopyBtn';
import { KebabBtn } from '../ui/KebabBtn';
import { CmdHL } from '../ui/CmdHL';
import { Icon } from '../ui/Icon';
import { CatDots } from './SecretRow';
import { useVaultStore } from '../../store';
import type { CommandItem, Category } from '../../types';

interface Props {
  item: CommandItem;
  cats: Category[];
}

export function CommandRow({ item, cats }: Props) {
  const go             = useVaultStore((s) => s.go);
  const setEditTarget  = useVaultStore((s) => s.setEditTarget);
  const deleteItem     = useVaultStore((s) => s.deleteItem);
  const setPlaceholder = useVaultStore((s) => s.setPlaceholder);

  const hasPlaceholders = /\{\{/.test(item.command);

  const kebab = [
    { icon: 'edit',     label: 'Edit',                   onClick: () => { setEditTarget(item); go('edit'); } },
    ...(hasPlaceholders
      ? [{ icon: 'terminal', label: 'Fill placeholders…', onClick: () => setPlaceholder(item) }]
      : []),
    { divider: true },
    { icon: 'trash',    label: 'Delete', danger: true,   onClick: () => deleteItem(item.id) },
  ];

  const truncated = item.command.slice(0, 70) + (item.command.length > 70 ? '…' : '');

  return (
    <div className="px-5 py-4 border-b border-bd hover:bg-raised transition-colors duration-100">
      {/* Header row */}
      <div className="flex items-center gap-3 mb-2">
        <span className="w-2 h-2 rounded-full shrink-0 bg-warn" />
        <span className="flex-1 text-[13px] font-semibold text-tx overflow-hidden text-ellipsis whitespace-nowrap">
          {item.name}
        </span>
        <span className="text-[0.65rem] px-1.5 py-0.5 border border-bd rounded text-tx3 font-mono tracking-wide shrink-0">
          {item.shell}
        </span>
        <CatDots names={item.categories} cats={cats} />
        <KebabBtn menuItems={kebab} />
      </div>
      {/* Command preview */}
      <div className="pl-5 mb-1.5 text-xs font-mono overflow-hidden text-ellipsis whitespace-nowrap text-tx2">
        <span className="text-tx3 mr-1.5">$</span>
        <CmdHL cmd={truncated} />
      </div>
      {/* Actions row */}
      <div className="pl-5 flex items-center gap-2">
        {item.description && (
          <span className="flex-1 text-xs text-tx3 overflow-hidden text-ellipsis whitespace-nowrap italic">
            {item.description}
          </span>
        )}
        <CopyBtn value={item.command} label={hasPlaceholders ? 'COPY RAW' : 'COPY'} />
        {hasPlaceholders && (
          <button
            onClick={() => setPlaceholder(item)}
            className={[
              'bg-warn-b border border-warn-d rounded text-warn cursor-pointer',
              'px-2 py-1 text-xs font-medium tracking-wide font-ui whitespace-nowrap',
              'flex items-center gap-2 transition-all duration-150 hover:opacity-80',
            ].join(' ')}
          >
            <Icon name="terminal" size={12} color="oklch(0.72 0.16 68)" />
            FILL
          </button>
        )}
      </div>
    </div>
  );
}
