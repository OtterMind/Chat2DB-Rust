export default `-- 创建编辑器设置表
CREATE TABLE EditorSettings (
    UserID INT PRIMARY KEY,
    ThemeName VARCHAR(50),
    FontName VARCHAR(50),
    FontSize INT
);

-- 插入一条示例数据
INSERT INTO EditorSettings (UserID, ThemeName, FontName, FontSize) 
VALUES (1, 'Dark', 'Consolas', 14);


-- 插入一条示例数据
-- 查询access_token表
SELECT *
FROM
binary_string_table;
`;
