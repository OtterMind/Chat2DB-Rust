// import * as ContextMenu from '@radix-ui/react-dropdown-menu';
import { CSSProperties, ReactNode } from 'react';
import * as DropdownMenu from '@radix-ui/react-dropdown-menu';
import { useStyles } from './style';
import { QuestionType } from '@/constants/chat';
import { SvgIcons } from '@/components/SvgIcons';

export interface IMenuItem {
  icon: string;
  key: QuestionType;
  label: string;
  shortcutKeyLabel: string;
  shortcutKeyValue: string;
  onClick: (event: Event, editorSelectContent: string) => void;
}

interface IProps {
  trigger?: ReactNode;
  triggerStyle?: CSSProperties;
  open: boolean;
  onCloseContextMenu: () => void;
  getCurrentSelectContent: () => string;
  contextMenu: {
    menu: IMenuItem[];
  };
}

const MyContextMenu = (props: IProps) => {
  const { open, triggerStyle, onCloseContextMenu, contextMenu, getCurrentSelectContent } = props;
  const { styles } = useStyles();

  const handleKeyDown = (event) => {
    if (event.ctrlKey || event.metaKey) {
      event.preventDefault();
      const curKey = event.key.toLowerCase();
      const findMenu = contextMenu.menu.find((i) => i.shortcutKeyValue === curKey);
      if (findMenu) {
        onCloseContextMenu && onCloseContextMenu();
        findMenu.onClick(event, getCurrentSelectContent());
      }
    }
  };

  return (
    <>
      <DropdownMenu.Root open={open}>
        <DropdownMenu.Trigger asChild>
          <div style={{ ...triggerStyle }}>{props?.trigger}</div>
        </DropdownMenu.Trigger>
        <DropdownMenu.Portal>
          <DropdownMenu.Content
            className={styles.ContextMenuContent}
            align="start"
            onPointerDownOutside={() => {
              onCloseContextMenu && onCloseContextMenu();
            }}
            onEscapeKeyDown={() => {
              onCloseContextMenu && onCloseContextMenu();
            }}
            onKeyDown={handleKeyDown}
          >
            {contextMenu.menu.map((m) => (
              <DropdownMenu.Item
                key={m.key}
                className={styles.ContextMenuItem}
                onSelect={(event) => {
                  onCloseContextMenu && onCloseContextMenu();
                  m.onClick(event, getCurrentSelectContent());
                }}
              >
                {SvgIcons[m.icon]()}
                {m.label}
                {/* <div className={styles.RightSlot}>{m.shortcutKeyLabel}</div> */}
              </DropdownMenu.Item>
            ))}
          </DropdownMenu.Content>
        </DropdownMenu.Portal>
      </DropdownMenu.Root>
    </>
  );
};

export default MyContextMenu;
