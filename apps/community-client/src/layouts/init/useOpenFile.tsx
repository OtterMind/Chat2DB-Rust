import { useEffect, useState } from 'react';
import { useWorkspaceStore } from '@/store/workspace';
import { JcefEventBus, JavaPushActionType } from '@/jcef/eventBus';

const useOpenFile = () => {
  const { readFile, consoleList } = useWorkspaceStore((state) => ({
    readFile: state.readFile,
    consoleList: state.consoleList,
  }));

  const [filePath, setFilePath] = useState<string>();

  // Read file contents
  const handleReadFileContent = (data) => {
    setFilePath(data.data);
  };

  // Wait for consoleList to render before opening the file;
  // otherwise consoleList overwrites the workspace tab.
  // Or should we change this to: only data is stored here, and the tab logic is handled externally?
  useEffect(() => {
    if (consoleList && filePath) {
      readFile(filePath);
      setFilePath(undefined); // Clear filePath to avoid repeated reading
    }
  }, [consoleList, filePath]);

  useEffect(() => {
    JcefEventBus.on(JavaPushActionType.OPEN_FILE, handleReadFileContent);
    return () => {
      JcefEventBus.off(JavaPushActionType.OPEN_FILE);
    };
  }, []);
};

export default useOpenFile;
