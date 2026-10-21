import { RefObject } from 'react';
import * as VTable from '@visactor/vtable';
import { ContextMenuRef } from '@/components/ContextMenu';

// Vtable instance
export type ITableInstance = VTable.ListTable  & {
  contextMenuRef?: IContextMenuRef;
};

// selected cell
export type ISelectEvent = VTable.TYPES.MousePointerMultiCellEvent;

// ref of right-click menu
export type IContextMenuRef = RefObject<ContextMenuRef>;
