import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';
import type { VaultItem, Category, Screen, MenuState } from '../types';

export const CAT_COLORS_PRESET = [
  '#FF9900', '#10a37f', '#635bff', '#c9d1d9',
  'oklch(0.62 0.16 280)', 'oklch(0.70 0.17 162)',
  'oklch(0.70 0.17 220)', 'oklch(0.62 0.20 22)',
];

interface ToastState {
  msg: string;
  type: 'success' | 'error';
}

interface VaultStore {
  screen:      Screen;
  items:       VaultItem[];
  cats:        Category[];
  editTarget:  VaultItem | null;
  menu:        MenuState | null;
  toast:       ToastState | null;
  placeholder: VaultItem | null;
  lockTimeout: number;   // minutes; 0 = never
  hotkey:      string;

  go:             (screen: Screen) => void;
  setEditTarget:  (item: VaultItem | null) => void;
  openMenu:       (menu: MenuState) => void;
  closeMenu:      () => void;
  showToast:      (msg: string, type?: 'success' | 'error') => void;
  setPlaceholder: (item: VaultItem | null) => void;
  setLockTimeout: (mins: number) => void;
  setHotkey:      (key: string) => void;
  unlock:              (password: string) => Promise<void>;
  unlockWithPayload:   (payload: { items: VaultItem[]; categories: Category[] }) => Promise<void>;
  lock:                () => Promise<void>;
  wipe:           () => Promise<void>;
  saveItem:       (form: Omit<VaultItem, 'id' | 'created'>) => Promise<void>;
  deleteItem:     (id: number) => Promise<void>;
  saveCats:       (cats: Category[]) => Promise<void>;
}

let toastTimer: ReturnType<typeof setTimeout>;

export const useVaultStore = create<VaultStore>((set, get) => ({
  screen:      'lock',
  items:       [],
  cats:        [],
  editTarget:  null,
  menu:        null,
  toast:       null,
  placeholder: null,
  lockTimeout: 5,
  hotkey:      'Ctrl+Alt+Z',

  go: (screen) => set({ screen }),

  setEditTarget: (editTarget) => set({ editTarget }),

  openMenu: (menu) => set({ menu }),

  closeMenu: () => set({ menu: null }),

  showToast: (msg, type = 'success') => {
    set({ toast: { msg, type } });
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => set({ toast: null }), 2200);
  },

  setPlaceholder: (placeholder) => set({ placeholder }),

  setLockTimeout: (lockTimeout) => set({ lockTimeout }),

  setHotkey: (hotkey) => set({ hotkey }),

  unlock: async (password) => {
    const [result, settings] = await Promise.all([
      invoke<{ items: VaultItem[]; categories: Category[] }>('vault_unlock', { password }),
      invoke<{ autoLockTimeout: number; hotkey: string }>('vault_get_settings'),
    ]);
    set({
      items:       result.items,
      cats:        result.categories,
      screen:      'vault',
      editTarget:  null,
      lockTimeout: settings.autoLockTimeout,
      hotkey:      settings.hotkey,
    });
  },

  unlockWithPayload: async (payload) => {
    const settings = await invoke<{ autoLockTimeout: number; hotkey: string }>('vault_get_settings');
    set({
      items:       payload.items,
      cats:        payload.categories,
      screen:      'vault',
      editTarget:  null,
      lockTimeout: settings.autoLockTimeout,
      hotkey:      settings.hotkey,
    });
  },

  lock: async () => {
    try {
      await invoke('vault_lock');
    } catch {}
    set({ screen: 'lock', items: [], cats: [], editTarget: null, menu: null });
  },

  wipe: async () => {
    await invoke('vault_wipe');
    set({ screen: 'lock', items: [], cats: [], editTarget: null, menu: null });
  },

  saveItem: async (form) => {
    const { editTarget } = get();
    const itemToSave: VaultItem = {
      ...form,
      id:      editTarget?.id ?? 0,
      created: editTarget?.created ?? new Date().toISOString().slice(0, 10),
    } as VaultItem;

    const saved = await invoke<VaultItem>('vault_save_item', { item: itemToSave });

    set((s) => ({
      items: editTarget
        ? s.items.map((i) => (i.id === saved.id ? saved : i))
        : [...s.items, saved],
      editTarget: null,
      screen: 'vault',
    }));
  },

  deleteItem: async (id) => {
    await invoke('vault_delete_item', { id });
    set((s) => ({ items: s.items.filter((i) => i.id !== id), screen: 'vault' }));
  },

  saveCats: async (cats) => {
    await invoke('vault_save_categories', { cats });
    set({ cats });
  },
}));
