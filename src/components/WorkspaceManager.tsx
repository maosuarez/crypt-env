import { useEffect, useState } from 'react';
import { Icon } from './ui/Icon';
import { useVaultStore } from '../store';
import { useWorkspaceStore } from '../store/workspaceStore';
import type { WorkspaceVar } from '../types';
import type { WorkspaceTemplate } from '../types';

// ─── Template definitions ─────────────────────────────────────────────────────

const TEMPLATES: { id: WorkspaceTemplate; label: string; vars: string[] }[] = [
  { id: 'generic',  label: 'Generic',     vars: [] },
  { id: 'node',     label: 'Node.js',     vars: ['DATABASE_URL', 'PORT', 'NODE_ENV', 'JWT_SECRET', 'API_KEY'] },
  { id: 'postgres', label: 'PostgreSQL',  vars: ['POSTGRES_HOST', 'POSTGRES_PORT', 'POSTGRES_USER', 'POSTGRES_PASSWORD', 'POSTGRES_DB'] },
  { id: 'mongo',    label: 'MongoDB',     vars: ['MONGO_URI', 'MONGO_DB_NAME', 'MONGO_USER', 'MONGO_PASSWORD'] },
  { id: 'docker',   label: 'Docker',      vars: ['REGISTRY_URL', 'DOCKER_USERNAME', 'DOCKER_PASSWORD', 'BUILD_TAG', 'IMAGE_NAME'] },
  { id: 'python',   label: 'Python',      vars: ['FLASK_ENV', 'SECRET_KEY', 'DATABASE_URL', 'REDIS_URL', 'DEBUG'] },
];

// ─── Sub-components ───────────────────────────────────────────────────────────

function TemplateModal({
  onSelect,
  onClose,
}: {
  onSelect: (t: WorkspaceTemplate) => void;
  onClose:  () => void;
}) {
  return (
    <div className="absolute inset-0 bg-black/70 flex items-center justify-center z-30 p-4">
      <div className="w-full max-w-sm bg-surface border border-bd rounded-[4px] p-4">
        <div className="text-[10px] font-semibold text-tx3 font-mono tracking-[0.09em] mb-3">
          // SELECT TEMPLATE
        </div>
        <div className="grid grid-cols-2 gap-2">
          {TEMPLATES.map((t) => (
            <button
              key={t.id}
              onClick={() => onSelect(t.id)}
              className="p-3 bg-raised border border-bd2 rounded-[3px] text-left hover:border-accent-d hover:text-accent transition-colors group"
            >
              <div className="text-[12px] font-semibold text-tx group-hover:text-accent font-ui">{t.label}</div>
              <div className="text-[10px] text-tx3 font-mono mt-0.5">
                {t.vars.length > 0 ? `${t.vars.length} vars` : 'empty'}
              </div>
            </button>
          ))}
        </div>
        <button
          onClick={onClose}
          className="mt-3 w-full py-[7px] rounded-[3px] text-[11px] font-bold tracking-[0.06em] font-ui cursor-pointer bg-transparent border border-bd2 text-tx2 hover:text-tx transition-colors"
        >
          CANCEL
        </button>
      </div>
    </div>
  );
}

function VarRow({
  v,
  vaultItems,
  onChange,
  onDelete,
}: {
  v:          WorkspaceVar;
  vaultItems: { id: number; name: string }[];
  onChange:   (updated: WorkspaceVar) => void;
  onDelete:   () => void;
}) {
  return (
    <div className="flex items-center gap-2 py-1.5 border-b border-bd">
      {/* Key */}
      <input
        value={v.key}
        onChange={(e) => onChange({ ...v, key: e.target.value.toUpperCase().replace(/[^A-Z0-9_]/g, '') })}
        placeholder="KEY_NAME"
        className="w-[160px] bg-bg border border-bd2 text-tx font-mono text-[12px] rounded-[3px] px-2 py-[4px] outline-none focus:border-accent-d transition-colors"
      />
      {/* Source */}
      <select
        value={v.itemId != null ? `item:${v.itemId}` : 'literal'}
        onChange={(e) => {
          const val = e.target.value;
          if (val === 'literal') {
            onChange({ ...v, itemId: undefined, literal: v.literal ?? '' });
          } else {
            const id = parseInt(val.replace('item:', ''), 10);
            onChange({ ...v, itemId: id, literal: undefined });
          }
        }}
        className="flex-1 bg-raised border border-bd2 text-tx rounded-[3px] px-2 py-[4px] text-[11px] font-ui cursor-pointer outline-none"
      >
        <option value="literal">— literal —</option>
        {vaultItems.map((i) => (
          <option key={i.id} value={`item:${i.id}`}>{i.name}</option>
        ))}
      </select>
      {/* Literal input if not from vault */}
      {v.itemId == null && (
        <input
          value={v.literal ?? ''}
          onChange={(e) => onChange({ ...v, literal: e.target.value })}
          placeholder="value"
          className="w-[120px] bg-bg border border-bd2 text-tx font-mono text-[12px] rounded-[3px] px-2 py-[4px] outline-none focus:border-accent-d transition-colors"
        />
      )}
      <button
        onClick={onDelete}
        className="text-tx3 hover:text-danger transition-colors shrink-0"
      >
        <Icon name="trash" size={13} />
      </button>
    </div>
  );
}

// ─── WorkspaceManager ─────────────────────────────────────────────────────────

export function WorkspaceManager() {
  const go         = useVaultStore((s) => s.go);
  const showToast  = useVaultStore((s) => s.showToast);
  const items      = useVaultStore((s) => s.items);

  const { workspaces, selected, loading, load, select, save, remove, inject } = useWorkspaceStore();

  // Draft state for the editor panel
  const [name,        setName]        = useState('');
  const [description, setDescription] = useState('');
  const [path,        setPath]        = useState('');
  const [template,    setTemplate]    = useState<WorkspaceTemplate>('generic');
  const [vars,        setVars]        = useState<WorkspaceVar[]>([]);
  const [saving,      setSaving]      = useState(false);
  const [injecting,   setInjecting]   = useState(false);
  const [confirmDel,  setConfirmDel]  = useState(false);
  const [templateModal, setTemplateModal] = useState(false);

  useEffect(() => { load(); }, []);

  useEffect(() => {
    if (selected) {
      setName(selected.name);
      setDescription(selected.description ?? '');
      setPath(selected.path ?? '');
      setTemplate(selected.template as WorkspaceTemplate);
      setVars(selected.vars);
      setConfirmDel(false);
    }
  }, [selected?.id]);

  const vaultItems = items.map((i) => ({
    id:   i.id,
    name: ('name' in i ? i.name : 'title' in i ? (i as any).title : '') || `#${i.id}`,
  }));

  const handleNew = () => {
    select(null);
    setName('');
    setDescription('');
    setPath('');
    setTemplate('generic');
    setVars([]);
    setConfirmDel(false);
    setTemplateModal(true);
  };

  const handleTemplateSelect = (t: WorkspaceTemplate) => {
    setTemplate(t);
    const tpl = TEMPLATES.find((x) => x.id === t);
    if (tpl) {
      setVars(tpl.vars.map((key, idx) => ({ id: -(idx + 1), key, itemId: undefined, literal: '' })));
    }
    setTemplateModal(false);
  };

  const handleSave = async () => {
    if (!name.trim()) { showToast('Name is required', 'error'); return; }
    setSaving(true);
    try {
      await save({
        id:          selected?.id,
        name:        name.trim(),
        description: description || undefined,
        path:        path || undefined,
        template,
        vars,
      });
      showToast('Workspace saved');
    } catch (e) {
      showToast(String(e), 'error');
    } finally {
      setSaving(false);
    }
  };

  const handleDelete = async () => {
    if (!selected) return;
    try {
      await remove(selected.id);
      showToast('Workspace deleted');
    } catch (e) {
      showToast(String(e), 'error');
    }
  };

  const handleInject = async () => {
    if (!selected) return;
    if (!path) { showToast('Set a path before injecting', 'error'); return; }
    setInjecting(true);
    try {
      const result = await inject(selected.id);
      showToast(`Injected ${result.written.length} variable${result.written.length !== 1 ? 's' : ''} → ${result.path}`);
    } catch (e) {
      showToast(String(e), 'error');
    } finally {
      setInjecting(false);
    }
  };

  const addVar = () => {
    setVars((v) => [...v, { id: -(Date.now()), key: '', itemId: undefined, literal: '' }]);
  };

  const isEditing = selected !== null || name !== '';

  return (
    <div className="flex-1 flex flex-col overflow-hidden animate-fade-in relative">
      {/* Header */}
      <div className="px-3.5 py-[9px] border-b border-bd flex items-center gap-[10px] shrink-0">
        <button
          onClick={() => go('vault')}
          className="flex items-center gap-1 text-[12px] font-medium font-ui text-tx3 bg-transparent border-none cursor-pointer hover:text-tx transition-colors"
        >
          <Icon name="back" size={13} />Back
        </button>
        <div className="flex-1 text-[13px] font-semibold text-center text-tx">Workspaces</div>
        <button
          onClick={handleNew}
          className="flex items-center gap-1 text-[11px] font-bold font-ui text-accent border border-accent-d rounded-[3px] px-2.5 py-[4px] hover:bg-accent-b transition-colors"
        >
          <Icon name="plus" size={12} />NEW
        </button>
      </div>

      <div className="flex-1 flex overflow-hidden">
        {/* Sidebar */}
        <div className="w-[220px] shrink-0 border-r border-bd overflow-y-auto bg-bg">
          {loading && (
            <div className="p-4 text-[11px] text-tx3 font-mono">Loading…</div>
          )}
          {!loading && workspaces.length === 0 && (
            <div className="p-4 text-[11px] text-tx3 font-mono leading-[1.7]">
              No workspaces yet.<br />Click NEW to create one.
            </div>
          )}
          {workspaces.map((ws) => (
            <button
              key={ws.id}
              onClick={() => select(ws)}
              className={[
                'w-full text-left px-3 py-2.5 border-b border-bd transition-colors',
                selected?.id === ws.id
                  ? 'bg-accent-b border-l-2 border-l-accent'
                  : 'hover:bg-raised border-l-2 border-l-transparent',
              ].join(' ')}
            >
              <div className="text-[12px] font-semibold text-tx font-ui truncate">{ws.name}</div>
              <div className="flex items-center gap-1.5 mt-0.5">
                <span className="text-[9px] font-mono text-tx3 bg-raised px-1.5 py-[1px] rounded-[2px] uppercase">{ws.template}</span>
                <span className="text-[9px] text-tx3 font-mono">{ws.vars.length} var{ws.vars.length !== 1 ? 's' : ''}</span>
              </div>
            </button>
          ))}
        </div>

        {/* Editor panel */}
        <div className="flex-1 flex flex-col overflow-hidden bg-surface">
          {!isEditing && !selected ? (
            <div className="flex-1 flex items-center justify-center">
              <div className="text-center text-tx3 font-mono text-[12px] leading-[1.8]">
                Select or create a workspace<br />
                <span className="text-[10px]">Workspaces group env vars for a project context</span>
              </div>
            </div>
          ) : (
            <>
              <div className="flex-1 overflow-y-auto px-4 py-3">
                <div className="text-[10px] font-semibold text-tx3 font-mono tracking-[0.12em] mb-3 pb-1.5 border-b border-bd">
                  // WORKSPACE
                </div>

                {/* Name */}
                <div className="mb-3">
                  <div className="text-[10px] font-semibold text-tx3 font-mono tracking-[0.06em] mb-1">NAME</div>
                  <input
                    value={name}
                    onChange={(e) => setName(e.target.value)}
                    placeholder="my-project"
                    className="w-full bg-bg border border-bd2 text-tx font-mono text-[13px] rounded-[3px] px-3 py-[7px] outline-none focus:border-accent-d transition-colors"
                  />
                </div>

                {/* Description */}
                <div className="mb-3">
                  <div className="text-[10px] font-semibold text-tx3 font-mono tracking-[0.06em] mb-1">DESCRIPTION</div>
                  <input
                    value={description}
                    onChange={(e) => setDescription(e.target.value)}
                    placeholder="Optional notes"
                    className="w-full bg-bg border border-bd2 text-tx font-mono text-[12px] rounded-[3px] px-3 py-[7px] outline-none focus:border-accent-d transition-colors"
                  />
                </div>

                {/* Path */}
                <div className="mb-3">
                  <div className="text-[10px] font-semibold text-tx3 font-mono tracking-[0.06em] mb-1">
                    PATH <span className="text-tx3 normal-case tracking-normal font-normal">.env file path for inject</span>
                  </div>
                  <input
                    value={path}
                    onChange={(e) => setPath(e.target.value)}
                    placeholder="C:\projects\myapp\.env"
                    className="w-full bg-bg border border-bd2 text-tx font-mono text-[12px] rounded-[3px] px-3 py-[7px] outline-none focus:border-accent-d transition-colors"
                  />
                </div>

                {/* Template */}
                <div className="mb-3">
                  <div className="text-[10px] font-semibold text-tx3 font-mono tracking-[0.06em] mb-1">TEMPLATE</div>
                  <div className="flex items-center gap-2">
                    <span className="text-[11px] font-mono text-accent bg-accent-b border border-accent-d rounded-[3px] px-2 py-[3px] capitalize">
                      {template}
                    </span>
                    <button
                      onClick={() => setTemplateModal(true)}
                      className="text-[10px] font-ui font-medium text-tx2 border border-bd2 rounded-[3px] px-2 py-[3px] hover:text-tx transition-colors"
                    >
                      CHANGE
                    </button>
                  </div>
                </div>

                {/* Variables */}
                <div className="mt-4">
                  <div className="text-[10px] font-semibold text-tx3 font-mono tracking-[0.12em] mb-2 pb-1.5 border-b border-bd flex items-center justify-between">
                    <span>// VARIABLES</span>
                    <button
                      onClick={addVar}
                      className="flex items-center gap-1 text-accent text-[10px] font-ui font-bold hover:opacity-80 transition-opacity"
                    >
                      <Icon name="plus" size={10} />ADD
                    </button>
                  </div>

                  {vars.length === 0 && (
                    <div className="text-[11px] text-tx3 font-mono py-2">
                      No variables. Click ADD or change template.
                    </div>
                  )}

                  {vars.map((v, idx) => (
                    <VarRow
                      key={v.id}
                      v={v}
                      vaultItems={vaultItems}
                      onChange={(updated) =>
                        setVars((prev) => prev.map((x, i) => (i === idx ? updated : x)))
                      }
                      onDelete={() => setVars((prev) => prev.filter((_, i) => i !== idx))}
                    />
                  ))}
                </div>
              </div>

              {/* Footer actions */}
              <div className="px-4 py-3 border-t border-bd bg-bg shrink-0">
                <div className="flex items-center gap-2">
                  {/* Inject */}
                  {selected && (
                    <button
                      onClick={handleInject}
                      disabled={injecting || !path}
                      className="flex items-center gap-1.5 px-3 py-[7px] rounded-[3px] text-[11px] font-bold tracking-[0.06em] font-ui cursor-pointer bg-transparent border border-accent-d text-accent hover:bg-accent-b transition-colors disabled:opacity-40"
                    >
                      {injecting
                        ? <><div className="w-2.5 h-2.5 rounded-full border-2 border-transparent border-t-current animate-spin-fast" />INJECTING…</>
                        : <><Icon name="export" size={11} />INJECT TO PATH</>}
                    </button>
                  )}
                  <div className="flex-1" />
                  {/* Delete */}
                  {selected && !confirmDel && (
                    <button
                      onClick={() => setConfirmDel(true)}
                      className="px-3 py-[7px] rounded-[3px] text-[11px] font-bold tracking-[0.06em] font-ui cursor-pointer bg-transparent border border-bd2 text-tx3 hover:text-danger hover:border-danger transition-colors"
                    >
                      DELETE
                    </button>
                  )}
                  {confirmDel && (
                    <>
                      <button
                        onClick={() => setConfirmDel(false)}
                        className="px-3 py-[7px] rounded-[3px] text-[11px] font-ui cursor-pointer bg-transparent border border-bd2 text-tx2 hover:text-tx transition-colors"
                      >
                        CANCEL
                      </button>
                      <button
                        onClick={handleDelete}
                        className="px-3 py-[7px] rounded-[3px] text-[11px] font-bold font-ui cursor-pointer bg-danger border-none text-white hover:opacity-90 transition-opacity"
                      >
                        CONFIRM DELETE
                      </button>
                    </>
                  )}
                  {/* Save */}
                  <button
                    onClick={handleSave}
                    disabled={saving || !name.trim()}
                    className="px-4 py-[7px] rounded-[3px] text-[11px] font-bold tracking-[0.06em] font-ui cursor-pointer bg-accent border-none text-[#020504] hover:opacity-90 transition-opacity disabled:opacity-40 flex items-center gap-1.5"
                  >
                    {saving
                      ? <><div className="w-2.5 h-2.5 rounded-full border-2 border-transparent border-t-[#020504] animate-spin-fast" />SAVING…</>
                      : 'SAVE'}
                  </button>
                </div>
              </div>
            </>
          )}
        </div>
      </div>

      {/* Template selection modal */}
      {templateModal && (
        <TemplateModal
          onSelect={handleTemplateSelect}
          onClose={() => setTemplateModal(false)}
        />
      )}
    </div>
  );
}
