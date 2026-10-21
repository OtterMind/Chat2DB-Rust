import { useState } from 'react';
import { Flex, Button, Input } from 'antd';
import jcefApi from '@/jcef';
import feedback from '@/utils/feedback';

const Demo = () => {
  const [openFileUrl, setOpenFileUrl] = useState('/Users');
  const [localFilePath, setLocalFilePath] = useState('/Users');
  const [saveFileName, setSaveFileName] = useState('test.txt');
  const [saveFileContent, setSaveFileContent] = useState('Hello World');
  const [updateFilePath, setUpdateFilePath] = useState('');
  const [updateFileContent, setUpdateFileContent] = useState('');

  return (
    <Flex vertical gap="20px" style={{ padding: '20px 40px' }}>
      {/* File operations. */}
      <Flex gap="20px">
        <Input placeholder="文件路径" value={openFileUrl} onChange={(e) => setOpenFileUrl(e.target.value)} />
      </Flex>

      <Flex gap="20px">
        <Input placeholder="本地文件路径" value={localFilePath} onChange={(e) => setLocalFilePath(e.target.value)} />
        <Button onClick={() => jcefApi.readFile(localFilePath)}>打开本地文件</Button>
      </Flex>

      <Flex gap="20px">
        <Button onClick={() => jcefApi.selectDirectory().then((res) => feedback.info(res))}>选择目录</Button>
      </Flex>

      {/* Save files. */}
      <Flex gap="20px">
        <Input placeholder="文件名" value={saveFileName} onChange={(e) => setSaveFileName(e.target.value)} />
        <Input placeholder="文件内容" value={saveFileContent} onChange={(e) => setSaveFileContent(e.target.value)} />
        <Button
          onClick={() =>
            jcefApi.saveFile({
              fileName: saveFileName,
              fileContent: saveFileContent,
              fileType: 'txt',
            })
          }
        >
          保存文件
        </Button>
      </Flex>

      {/* Update file contents. */}
      <Flex gap="20px">
        <Input
          placeholder="要更新的文件路径"
          value={updateFilePath}
          onChange={(e) => setUpdateFilePath(e.target.value)}
        />
        <Input
          placeholder="新文件内容"
          value={updateFileContent}
          onChange={(e) => setUpdateFileContent(e.target.value)}
        />
        <Button
          onClick={() =>
            jcefApi.updateFileContent({
              filePath: updateFilePath,
              fileContent: updateFileContent,
            })
          }
        >
          更新文件内容
        </Button>
      </Flex>

      {/* Window operations. */}
      <Flex gap="20px">
        <Button onClick={() => jcefApi.minimizeWindow()}>最小化</Button>
        <Button onClick={() => jcefApi.maximizeWindow()}>最大化</Button>
        <Button onClick={() => jcefApi.closeWindow()}>关闭窗口</Button>
        <Button onClick={() => jcefApi.isWindowMaximized().then((res) => alert(res))}>检查是否最大化</Button>
      </Flex>

      {/* Zoom operations. */}
      <Flex gap="20px">
        <Button onClick={() => jcefApi.webFrameSetZoom({ action: 'zoomIn' })}>放大</Button>
        <Button onClick={() => jcefApi.webFrameSetZoom({ action: 'zoomOut' })}>缩小</Button>
        <Button onClick={() => jcefApi.webFrameSetZoom({ action: 'zoomReset' })}>重置缩放</Button>
      </Flex>

      {/* Application operations. */}
      <Flex gap="20px">
        <Button onClick={() => jcefApi.appCheckUpdate().then((res) => alert(res))}>检查更新</Button>
      </Flex>

      {/* System information. */}
      <Flex gap="20px">
        <Button onClick={() => jcefApi.getMacAddress().then((res) => alert(res))}>获取MAC地址</Button>
      </Flex>

      {/* Developer tools. */}
      <Flex gap="20px">
        <Button onClick={() => jcefApi.openLog()}>打开日志</Button>
        <Button onClick={() => jcefApi.openDevTools()}>打开开发者工具</Button>
      </Flex>
    </Flex>
  );
};

export default Demo;
