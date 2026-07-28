export type ItemType = 'secret' | 'credential' | 'link' | 'command' | 'note';
export type Shell    = 'bash' | 'zsh' | 'fish' | 'PowerShell' | 'cmd';
export type Screen   = 'lock' | 'vault' | 'edit' | 'categories' | 'settings' | 'projects';

export interface Category {
  id:    string;
  name:  string;
  color: string;
}

interface BaseItem {
  id:         number;
  type:       ItemType;
  categories: string[];
  notes?:     string;
  created:    string;
  /** Reusable across projects/environments — only global items are offered
   *  as vault-item references when linking an environment variable. */
  isGlobal?:  boolean;
}

export interface SecretItem extends BaseItem {
  type:  'secret';
  name:  string;
  value: string;
}

export interface CredentialItem extends BaseItem {
  type:     'credential';
  name:     string;
  url?:     string;
  username: string;
  password: string;
}

export interface LinkItem extends BaseItem {
  type:         'link';
  title:        string;
  url:          string;
  description?: string;
}

export interface CommandItem extends BaseItem {
  type:         'command';
  name:         string;
  command:      string;
  description?: string;
  shell:        Shell;
}

export interface NoteItem extends BaseItem {
  type:    'note';
  title:   string;
  content: string;
}

export type VaultItem = SecretItem | CredentialItem | LinkItem | CommandItem | NoteItem;

// ─── Project / Environment types ───────────────────────────────────────────────

export interface EnvironmentVar {
  id:     number;
  key:    string;
  itemId: number;
}

export interface Environment {
  id:        number;
  projectId: number;
  name:      string;
  isDefault: boolean;
  paths:     string[];
  vars:      EnvironmentVar[];
  created:   string;
  updated:   string;
}

export interface Project {
  id:           number;
  name:         string;
  description?: string;
  template:     string;
  created:      string;
  updated:      string;
  environments: Environment[];
  /** Category names — same convention as VaultItem.categories. Language is
   *  just another tag value here (e.g. "Python"). */
  categories:   string[];
}

export type ProjectTemplate =
  | 'generic' | 'node' | 'postgres' | 'mongo' | 'docker' | 'python';

export interface InjectResult {
  paths:   string[];
  written: string[];
}

export interface ProjectDeleteImpact {
  environments:   number;
  itemsDeleted:   number;
  itemsOrphaned:  number;
}

export interface ItemOwner {
  projectId:   number;
  projectName: string;
}

export interface GlobalToggleResult {
  updated: VaultItem | null;
  forked:  VaultItem[];
}

export interface ContextMenuItemDef {
  label?:   string;
  icon?:    string;
  onClick?: () => void;
  danger?:  boolean;
  divider?: boolean;
  sub?:     string;
}

export interface MenuState {
  x:     number;
  y:     number;
  items: ContextMenuItemDef[];
}

export type IconName =
  | 'lock'        | 'unlock'   | 'eye'      | 'eyeOff'  | 'copy'    | 'check'
  | 'plus'        | 'search'   | 'settings' | 'trash'   | 'edit'    | 'close'
  | 'back'        | 'shield'   | 'key'      | 'kbd'     | 'timer'   | 'person'
  | 'globe'       | 'terminal' | 'more'     | 'tag'     | 'drag'    | 'external'
  | 'export'      | 'rename'   | 'note'     | 'fingerprint' | 'refresh' | 'funnel';
