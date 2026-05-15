export type ItemType = 'secret' | 'credential' | 'link' | 'command' | 'note';
export type Shell    = 'bash' | 'zsh' | 'fish' | 'PowerShell' | 'cmd';
export type Screen   = 'lock' | 'vault' | 'edit' | 'categories' | 'settings';

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
