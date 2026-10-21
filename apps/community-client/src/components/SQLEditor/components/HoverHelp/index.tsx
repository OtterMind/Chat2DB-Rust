import * as Popover from '@radix-ui/react-popover';
import { CSSProperties, memo } from 'react';
import { useStyles } from './style';
import { IHoverInfo } from '@/typings/sqlParser';
import SQLPreview from '@/components/SQLPreview';
import { Flex } from 'antd';
import * as monaco from 'monaco-editor';

export interface HoverHelperInfo {
  open: boolean;
  position: CSSProperties;
  hoverInfo: IHoverInfo | null;
  editor: monaco.editor.IStandaloneCodeEditor | null;
  mouse: monaco.editor.IEditorMouseEvent | null;
}

interface IProps {
  hoverHelperInfo: HoverHelperInfo;
  onClose: () => void;
  canShow: () => boolean;
}
const HoverHelp = memo(({ hoverHelperInfo, onClose, canShow }: IProps) => {
  const { open, position, hoverInfo } = hoverHelperInfo;
  const { styles } = useStyles();

  const { datasourceName, databaseName, schemaName, tableName, ddl, comment, columnName, dataType } = hoverInfo || {};

  // console.log('hoverInfo', hoverInfo);
  if (!hoverInfo || JSON.stringify(hoverInfo) === '{}' || !canShow()) return null;

  return (
    <Popover.Root open={open}>
      <Popover.Anchor style={{ position: 'fixed', ...position }} />
      <Popover.Portal>
        <Popover.Content
          className={styles.HoverHelpContent}
          onPointerDownOutside={onClose}
          onEscapeKeyDown={onClose}
          align="start"
          sideOffset={10}
          collisionPadding={10}
          onOpenAutoFocus={(event) => event.preventDefault()}
        >
          <Flex vertical gap={8}>
            {datasourceName && (
              <Flex gap={8}>
                <div className={styles.HoverHelpTitle}>Datasource:</div>
                <div className={styles.HoverHelpValue}>{datasourceName}</div>
              </Flex>
            )}
            {databaseName && (
              <Flex gap={8}>
                <div className={styles.HoverHelpTitle}>Database:</div>
                <div className={styles.HoverHelpValue}>{databaseName}</div>
              </Flex>
            )}
            {schemaName && (
              <Flex gap={8}>
                <div className={styles.HoverHelpTitle}>Schema:</div>
                <div className={styles.HoverHelpValue}>{schemaName}</div>
              </Flex>
            )}
            {tableName && (
              <Flex gap={8}>
                <div className={styles.HoverHelpTitle}>Table:</div>
                <div className={styles.HoverHelpValue}>{tableName}</div>
              </Flex>
            )}
            {columnName && (
              <Flex gap={8}>
                <div className={styles.HoverHelpTitle}>Column:</div>
                <div className={styles.HoverHelpValue}>
                  {columnName}
                  {dataType && ` (${dataType})`}
                </div>
              </Flex>
            )}

            {comment && (
              <Flex gap={8}>
                <div className={styles.HoverHelpTitle}>Comment:</div>
                <div className={styles.HoverHelpValue}>{comment}</div>
              </Flex>
            )}
            {ddl && (
              <Flex vertical gap={4}>
                <div className={styles.HoverHelpTitle}>DDL</div>
                <SQLPreview sql={ddl} source="sql-editor-hover-ddl" foldable={false} />
              </Flex>
            )}
          </Flex>
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
});

export default HoverHelp;
