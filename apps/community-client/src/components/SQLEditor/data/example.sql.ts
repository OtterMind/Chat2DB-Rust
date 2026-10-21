export default `-- Create Editor Settings Table
CREATE TABLE EditorSettings (
    UserID INT PRIMARY KEY,
    ThemeName VARCHAR(50),
    FontName VARCHAR(50),
    FontSize INT
);

-- Insert Example Data
INSERT INTO EditorSettings (UserID, ThemeName, FontName, FontSize) 
VALUES (1, 'Dark', 'Consolas', 14);

-- Update User Theme
UPDATE EditorSettings
SET ThemeName = 'Monokai'
WHERE UserID = 1;

-- Query User Editor Settings
SELECT * FROM EditorSettings WHERE UserID = 1;

-- Create a stored procedure to update user's all editor settings
DELIMITER //
CREATE PROCEDURE UpdateEditorSettings(
    IN p_UserID INT,
    IN p_ThemeName VARCHAR(50),
    IN p_FontName VARCHAR(50),
    IN p_FontSize INT
)
BEGIN
    UPDATE EditorSettings
    SET ThemeName = p_ThemeName,
        FontName = p_FontName,
        FontSize = p_FontSize
    WHERE UserID = p_UserID;
END //


DELIMITER ;

-- Call stored procedure to update user settings
CALL UpdateEditorSettings(1, 'Light', 'Fira Code', 12);

CREATE TABLE Users (
  id bigint unsigned NOT NULL AUTO_INCREMENT COMMENT '主键',

)
`;
