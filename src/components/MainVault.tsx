import { useState, useMemo, useRef, useEffect } from 'react';
import { Icon } from './ui/Icon';
import { SecretRow } from './rows/SecretRow';
import { CredentialRow } from './rows/CredentialRow';
import { LinkRow } from './rows/LinkRow';
import { CommandRow } from './rows/CommandRow';
import { NoteRow } from './rows/NoteRow';
import { useVaultStore } from '../store';
import type { ItemType } from '../types';

type TypeFilter = 'all' | ItemType;

const TYPE_PILLS: { id: TypeFilter; label: string; dot?: string }[] = [
  { id: 'all',        label: 'ALL' },
  { id: 'secret',     label: 'KEY',  dot: 'oklch(0.70 0.17 162)' },
  { id: 'credential', label: 'CRED', dot: 'oklch(0.70 0.15 220)' },
  { id: 'link',       label: 'LINK', dot: 'oklch(0.68 0.15 270)' },
  { id: 'command',    label: 'CMD',  dot: 'oklch(0.72 0.16 68)'  },
  { id: 'note',       label: 'NOTE', dot: 'oklch(0.72 0.15 350)' },
];

export function MainVault() {
  const items = useVaultStore((s) => s.items);
  const cats  = useVaultStore((s) => s.cats);
  const go    = useVaultStore((s) => s.go);
  const setEditTarget = useVaultStore((s) => s.setEditTarget);

  const [query,   setQuery]   = useState('');
  const [typeF,   setTypeF]   = useState<TypeFilter>('all');
  const [loading, setLoading] = useState(true);
  const ref = useRef<HTMLInputElement>(null);

  useEffect(() => {
    setTimeout(() => setLoading(false), 400);
    ref.current?.focus();
  }, []);

  const filtered = useMemo(() => {
    return items.filter((it) => {
      if (typeF !== 'all' && it.type !== typeF) return false;
      const q = query.toLowerCase();
      if (!q) return true;
      const name = (('name' in it ? it.name : 'title' in it ? it.title : '') as string).toLowerCase();
      const val  = (('value' in it ? it.value : 'url' in it ? it.url : 'command' in it ? it.command : 'content' in it ? it.content : '') as string).toLowerCase();
      const desc = (it.notes ?? ('description' in it ? (it as any).description : '')).toLowerCase();
      const user = (('username' in it ? it.username : '') as string).toLowerCase();
      return name.includes(q) || val.includes(q) || desc.includes(q) || user.includes(q) || it.categories.join(' ').toLowerCase().includes(q);
    });
  }, [items, typeF, query]);

  const typeCounts = useMemo(() => {
    const c = { all: items.length, secret: 0, credential: 0, link: 0, command: 0, note: 0 };
    items.forEach((it) => { (c as any)[it.type] = ((c as any)[it.type] ?? 0) + 1; });
    return c;
  }, [items]);

  return (
    <div className="flex-1 flex flex-col overflow-hidden animate-fade-in">
      {/* Search bar */}
      <div className="flex items-center gap-3 pl-5 pr-4 h-12 border-b border-bd bg-bg shrink-0">
        <Icon name="search" size={16} />
        <input
          ref={ref}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Search name, value, URL, user, notes…"
          className="flex-1 text-[13px] text-tx font-ui bg-transparent outline-none placeholder:text-tx3"
        />
        {query && (
          <button
            onClick={() => setQuery('')}
            className="flex items-center justify-center w-6 h-6 rounded-md text-tx3 hover:text-tx hover:bg-raised transition"
          >
            <Icon name="close" size={14} />
          </button>
        )}
        <span className="text-[11px] text-tx3 font-mono min-w-[24px] text-right">
          {filtered.length}
        </span>
      </div>

      {/* Type filter pills + NEW button */}
      <div className="px-3 h-10 border-b border-bd bg-bg flex items-center gap-1.5 shrink-0">
        {TYPE_PILLS.map(({ id, label, dot }) => {
          const active = typeF === id;
          const count  = (typeCounts as any)[id] ?? 0;
          return (
            <button
              key={id}
              onClick={() => setTypeF(id)}
              className={[
                'flex items-center gap-1 rounded px-2.5 h-[28px]',
                'border text-[10px] font-medium tracking-wide font-ui cursor-pointer',
                'transition-all duration-150 shrink-0 whitespace-nowrap',
                active
                  ? 'bg-accent-b border-accent-d text-accent'
                  : 'bg-transparent border-bd text-tx3 hover:border-bd2 hover:text-tx2',
              ].join(' ')}
            >
              {dot && !active && (
                <span className="w-1.5 h-1.5 rounded-full shrink-0" style={{ background: dot }} />
              )}
              {label}
              <span className="text-[0.6rem] opacity-50">{count}</span>
            </button>
          );
        })}

        <div className="flex-1" />

        <button
          onClick={() => { setEditTarget(null); go('edit'); }}
          className={[
            'flex items-center gap-1 bg-accent border-none rounded',
            'px-3 h-[28px] text-[10px] font-bold tracking-wider font-ui',
            'text-[#020504] cursor-pointer shrink-0 hover:opacity-90 transition-opacity',
          ].join(' ')}
        >
          <Icon name="plus" size={11} color="#020504" />
          NEW
        </button>
      </div>

      {/* Item list */}
      <div className="flex-1 overflow-y-auto bg-surface">
        {loading ? (
          <div className="py-16 text-center text-tx3 text-sm font-mono">
            <div className="w-5 h-5 rounded-full border-2 border-bd2 border-t-accent animate-spin-fast mx-auto mb-3" />
            decrypting vault…
          </div>
        ) : filtered.length === 0 ? (
          <div className="py-16 text-center text-tx3 text-sm font-mono">// no items match</div>
        ) : (
          filtered.map((it) => {
            const props = { key: it.id, cats };
            if (it.type === 'secret')     return <SecretRow     {...props} item={it} />;
            if (it.type === 'credential') return <CredentialRow {...props} item={it} />;
            if (it.type === 'link')       return <LinkRow       {...props} item={it} />;
            if (it.type === 'command')    return <CommandRow    {...props} item={it} />;
            if (it.type === 'note')       return <NoteRow       {...props} item={it} />;
            return null;
          })
        )}
      </div>

      {/* Status bar */}
      <div className="flex items-center justify-between px-5 h-10 border-t border-bd bg-bg shrink-0">
        <div className="text-xs text-tx3 font-mono">{items.length} items · AES-256-GCM</div>
        <div className="flex gap-3">
          <button
            onClick={() => go('categories')}
            className="flex items-center gap-1.5 text-xs font-mono text-tx3 bg-transparent border-none cursor-pointer hover:text-tx transition-colors"
          >
            <Icon name="tag" size={13} />categories
          </button>
          <button
            onClick={() => go('settings')}
            className="flex items-center gap-1.5 text-xs font-mono text-tx3 bg-transparent border-none cursor-pointer hover:text-tx transition-colors"
          >
            <Icon name="settings" size={13} />settings
          </button>
        </div>
      </div>
    </div>
  );
}
