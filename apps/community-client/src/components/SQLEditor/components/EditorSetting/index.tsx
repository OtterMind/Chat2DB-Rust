import { FC } from 'react';
import { Flex, Select, InputNumber } from 'antd';
import { editorThemes } from '../../config';
import { useGlobalStore } from '@/store/global';
import { IEditorTheme } from '../../type';

interface EditorSettingProps {}

const EditorSetting: FC<EditorSettingProps> = () => {
  const { updateEditorConfig, editorSettings } = useGlobalStore((s) => ({
    editorSettings: s.editorSettings,
    updateEditorConfig: s.updateEditorSettings,
  }));
  const { theme, fontSize } = editorSettings;

  return (
    <Flex gap={8}>
      <Select
        options={Object.entries(editorThemes).map((i) => ({ label: i[0], value: i[0] }))}
        value={theme}
        onChange={(t) => updateEditorConfig({ ...editorSettings, theme: t as IEditorTheme })}
        popupMatchSelectWidth
      />

      <InputNumber
        value={fontSize}
        onChange={(f) => updateEditorConfig({ ...editorSettings, fontSize: f as number })}
        min={12}
        max={24}
        step={1}
        style={{ width: 90 }}
        addonAfter="px"
      />
    </Flex>
  );
};

export default EditorSetting;
