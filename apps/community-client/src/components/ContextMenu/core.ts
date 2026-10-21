import type React from 'react';
import type { ShortcutAction } from '@/constants/shortcut';

export interface ContextMenuPoint {
  x: number;
  y: number;
}

export interface ContextMenuIntent<TTargetSnapshot = unknown, TSelectionSnapshot = unknown> {
  surface: string;
  pointer: ContextMenuPoint;
  targetSnapshot: TTargetSnapshot;
  selectionSnapshot?: TSelectionSnapshot;
  capabilities?: Record<string, boolean>;
  version?: string | number;
  sourceEventMeta?: Record<string, unknown>;
}

export interface ContextMenuAction<TIntent extends ContextMenuIntent = ContextMenuIntent> {
  id: string;
  owner?: string;
  label: React.ReactNode;
  icon?: React.ReactNode;
  shortcutAction?: ShortcutAction;
  danger?: boolean;
  disabled?: boolean;
  disabledReason?: string;
  children?: ContextMenuAction<TIntent>[];
  validateBeforeExecute?: (intent: TIntent) => boolean;
  execute?: (intent: TIntent) => void;
}

export type ContextMenuEntry<TIntent extends ContextMenuIntent = ContextMenuIntent> =
  | ContextMenuAction<TIntent>
  | { type: 'divider'; id?: string };

export function isContextMenuAction<TIntent extends ContextMenuIntent>(
  entry: ContextMenuEntry<TIntent>,
): entry is ContextMenuAction<TIntent> {
  return !('type' in entry);
}

export function executeContextMenuAction<TIntent extends ContextMenuIntent>(
  action: ContextMenuAction<TIntent>,
  intent: TIntent,
) {
  if (action.disabled) {
    return false;
  }

  if (action.validateBeforeExecute && !action.validateBeforeExecute(intent)) {
    return false;
  }

  action.execute?.(intent);
  return true;
}
