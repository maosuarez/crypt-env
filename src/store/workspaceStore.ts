import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';
import type { Workspace } from '../types';

interface WorkspaceStore {
  workspaces:   Workspace[];
  selected:     Workspace | null;
  loading:      boolean;
  error:        string | null;

  load:         () => Promise<void>;
  select:       (ws: Workspace | null) => void;
  save:         (ws: Omit<Workspace, 'id' | 'created' | 'updated'> & { id?: number; }) => Promise<number>;
  remove:       (id: number) => Promise<void>;
  inject:       (id: number) => Promise<{ path: string; written: string[] }>;
  clearError:   () => void;
}

export const useWorkspaceStore = create<WorkspaceStore>((set, get) => ({
  workspaces: [],
  selected:   null,
  loading:    false,
  error:      null,

  load: async () => {
    set({ loading: true, error: null });
    try {
      const workspaces = await invoke<Workspace[]>('workspace_list');
      set({ workspaces, loading: false });
    } catch (e) {
      set({ loading: false, error: String(e) });
    }
  },

  select: (ws) => set({ selected: ws }),

  save: async (ws) => {
    const now = new Date().toISOString();
    const payload = {
      workspace: {
        id:          ws.id ?? 0,
        name:        ws.name,
        description: ws.description ?? null,
        path:        ws.path ?? null,
        template:    ws.template,
        vars:        ws.vars,
        created:     now,
        updated:     now,
      },
    };
    const id = await invoke<number>('workspace_save', payload);
    await get().load();
    const updated = get().workspaces.find((w) => w.id === id) ?? null;
    set({ selected: updated });
    return id;
  },

  remove: async (id) => {
    await invoke('workspace_delete', { id });
    set((s) => ({
      workspaces: s.workspaces.filter((w) => w.id !== id),
      selected:   s.selected?.id === id ? null : s.selected,
    }));
  },

  inject: async (id) => {
    const result = await invoke<{ path: string; written: string[] }>('workspace_inject', { id });
    return result;
  },

  clearError: () => set({ error: null }),
}));
