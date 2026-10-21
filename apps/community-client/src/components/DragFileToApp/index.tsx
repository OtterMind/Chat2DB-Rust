import React, { memo, useState } from 'react';
import { useStyles } from './style';
import { isDesktop } from '@/utils/env';

interface IProps {
  className?: string;
  children?: React.ReactNode;
  onDropCallback?: (files: FileList) => void;
  fileType?: string;
}

export default memo<IProps>((props) => {
  const { className, children, onDropCallback } = props;
  const { styles, cx } = useStyles();
  const [dragging, setDragging] = useState(false);

  const handleDragEnter = (e) => {
    // Check whether the current element is the direct drop target.
    if (!e.currentTarget.contains(e.target as Node)) {
      return;
    }
    e.preventDefault();
    e.stopPropagation();
    if (e.dataTransfer.items && e.dataTransfer.items.length > 0) {
      setDragging(true);
    }
  };

  const handleDragLeave = (e) => {
    // Check whether the current element is the direct drop target.
    if (!e.currentTarget.contains(e.target as Node)) {
      return;
    }
    e.preventDefault();
    e.stopPropagation();
  };

  const handleDragOver = (e) => {
    // Check whether the current element is the direct drop target.
    if (!e.currentTarget.contains(e.target as Node)) {
      return;
    }
    e.preventDefault();
    e.stopPropagation();
  };

  const handleDrop = (e) => {
    // Check whether the current element is the direct drop target.
    if (!e.currentTarget.contains(e.target as Node)) {
      return;
    }
    e.preventDefault();
    e.stopPropagation();
    setDragging(false);

    const droppedFiles = e.dataTransfer.files;
    if (props.fileType) {
      for (let i = 0; i < droppedFiles.length; i++) {
        const fileExtension = droppedFiles[i].name.split('.').pop();
        if (fileExtension !== props.fileType) {
          // TODO: Dragging a SQL file into an upload area also reaches this branch, so refine this component's checks.
          // message.error('Unsupported file type: ' + droppedFiles[i].type);
          return;
        }
      }
    }
    onDropCallback && onDropCallback(droppedFiles);
  };

  if (!isDesktop) {
    return <div className={cx(styles.dragBox, className)}>{children}</div>;
  }

  return (
    <div
      className={cx(styles.dragBox, className, { [styles.draging]: dragging })}
      onDragEnter={handleDragEnter}
      onDragLeave={handleDragLeave}
      onDragOver={handleDragOver}
      onDrop={handleDrop}
    >
      {children}
    </div>
  );
});
