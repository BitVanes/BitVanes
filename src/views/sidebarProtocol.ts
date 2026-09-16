export type SidebarStartMode = 'diff' | 'staged' | 'selection' | 'instant';

export interface SidebarHandlers {
  next(): void;
  prev(): void;
  jump(index: number): void;
  toggleAutoplay(): void;
  exit(): void;
  browse(): void;
  start(mode: SidebarStartMode): void;
  setStyle(style: string): void;
}

export interface InboundMessage {
  t: string;
  i?: number;
  mode?: SidebarStartMode;
  s?: string;
}
