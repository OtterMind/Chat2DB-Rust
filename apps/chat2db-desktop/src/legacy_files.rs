use std::{
    collections::HashMap,
    ffi::{OsStr, OsString},
    fs,
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
};

use cap_std::{
    ambient_authority,
    fs::{Dir, OpenOptions},
};
use encoding_rs::Encoding;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const MAX_REGISTERED_ROOTS: usize = 64;
const MAX_RELATIVE_PATH_BYTES: usize = 4 * 1024;
const MAX_PATH_COMPONENTS: usize = 128;
const MAX_CHILDREN: usize = 1_000;
const MAX_FILE_NAME_BYTES: usize = 240;
const MAX_FILE_TYPE_BYTES: usize = 32;
const MAX_TEXT_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_TEXT_CONTENT_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_field_names)]
pub(crate) struct LegacySaveFileRequest {
    pub file_name: String,
    pub file_content: String,
    pub file_type: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LegacyUpdateFileRequest {
    pub file_path: String,
    pub file_content: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LegacyReadFileRequest {
    pub path: String,
    #[serde(default)]
    pub charsets: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LegacyOpenSqlDirectoryRequest {
    pub path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LegacySqlDirectoryPathRequest {
    pub root_token: String,
    #[serde(default)]
    pub relative_path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LegacyCreateSqlDirectoryChildRequest {
    pub root_token: String,
    #[serde(default)]
    pub parent_relative_path: String,
    pub name: String,
    #[serde(rename = "type")]
    pub node_type: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LegacySaveSqlDirectoryFileRequest {
    pub root_token: String,
    #[serde(default)]
    pub parent_relative_path: String,
    pub name: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LegacyRenameSqlDirectoryChildRequest {
    pub root_token: String,
    pub relative_path: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LegacySavedFile {
    pub path: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct LegacySqlTreeNode {
    pub key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root_path: Option<String>,
    pub name: String,
    pub path: String,
    pub relative_path: String,
    #[serde(rename = "type")]
    pub node_type: String,
    pub disabled: bool,
    pub sql_file: bool,
    pub text_file: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_extension: Option<String>,
    pub has_children: bool,
    pub loaded: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<Self>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LegacyCreateSqlDirectoryChildResponse {
    pub created_node: LegacySqlTreeNode,
    pub children: Vec<LegacySqlTreeNode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LegacyRenameSqlDirectoryChildResponse {
    pub renamed_node: LegacySqlTreeNode,
    pub parent_relative_path: String,
    pub children: Vec<LegacySqlTreeNode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LegacyDeleteSqlDirectoryChildResponse {
    pub parent_relative_path: String,
    pub children: Vec<LegacySqlTreeNode>,
}

#[derive(Default)]
pub(crate) struct LegacySqlDirectoryRegistry {
    roots: Mutex<HashMap<String, Arc<LegacySqlRoot>>>,
}

struct LegacySqlRoot {
    token: String,
    canonical_path: PathBuf,
    directory: Dir,
    operations: Mutex<()>,
}

impl LegacySqlDirectoryRegistry {
    pub(crate) fn register_root(&self, path: &Path) -> Result<LegacySqlTreeNode, String> {
        let canonical_path = fs::canonicalize(path)
            .map_err(|_| "The selected SQL directory is not available".to_owned())?;
        let metadata = fs::symlink_metadata(&canonical_path)
            .map_err(|_| "The selected SQL directory is not available".to_owned())?;
        if !metadata.is_dir() {
            return Err("The selected SQL path is not a directory".to_owned());
        }
        let directory = Dir::open_ambient_dir(&canonical_path, ambient_authority())
            .map_err(|_| "The selected SQL directory could not be opened safely".to_owned())?;
        let token = Uuid::new_v4().to_string();
        let root = Arc::new(LegacySqlRoot {
            token: token.clone(),
            canonical_path,
            directory,
            operations: Mutex::new(()),
        });
        let mut roots = lock(&self.roots)?;
        if roots.len() >= MAX_REGISTERED_ROOTS {
            return Err("Too many SQL directories are open".to_owned());
        }
        roots.insert(token, Arc::clone(&root));
        drop(roots);
        root_node(&root)
    }

    pub(crate) fn list_children(
        &self,
        request: &LegacySqlDirectoryPathRequest,
    ) -> Result<Vec<LegacySqlTreeNode>, String> {
        let root = self.root(&request.root_token)?;
        let _operation = lock(&root.operations)?;
        list_children_locked(&root, &request.relative_path)
    }

    pub(crate) fn create_child(
        &self,
        request: &LegacyCreateSqlDirectoryChildRequest,
    ) -> Result<LegacyCreateSqlDirectoryChildResponse, String> {
        let root = self.root(&request.root_token)?;
        let _operation = lock(&root.operations)?;
        let parent = existing_relative_path(&root, &request.parent_relative_path)?;
        require_directory(&root, &parent)?;
        let is_directory = match request.node_type.as_str() {
            "directory" => true,
            "file" => false,
            _ => return Err("SQL directory child type must be file or directory".to_owned()),
        };
        let name = if is_directory {
            validate_leaf_name(&request.name)?.to_owned()
        } else {
            normalize_text_file_name(&request.name, "sql")?
        };
        let target = parent.join(&name);
        require_missing(&root, &target)?;
        if is_directory {
            root.directory
                .create_dir(&target)
                .map_err(|_| "The SQL directory could not be created".to_owned())?;
        } else {
            atomic_write_in_root(&root, &target, b"", false)?;
        }
        sync_root_directory(&root)?;
        Ok(LegacyCreateSqlDirectoryChildResponse {
            created_node: tree_node(&root, &target)?,
            children: list_children_relative(&root, &parent)?,
        })
    }

    pub(crate) fn save_file(
        &self,
        request: &LegacySaveSqlDirectoryFileRequest,
    ) -> Result<LegacyCreateSqlDirectoryChildResponse, String> {
        validate_content_size(&request.content)?;
        let root = self.root(&request.root_token)?;
        let _operation = lock(&root.operations)?;
        let parent = existing_relative_path(&root, &request.parent_relative_path)?;
        require_directory(&root, &parent)?;
        let name = normalize_text_file_name(&request.name, "sql")?;
        let target = available_file_path(&root, &parent, &name)?;
        atomic_write_in_root(&root, &target, request.content.as_bytes(), false)?;
        sync_root_directory(&root)?;
        Ok(LegacyCreateSqlDirectoryChildResponse {
            created_node: tree_node(&root, &target)?,
            children: list_children_relative(&root, &parent)?,
        })
    }

    pub(crate) fn rename_child(
        &self,
        request: &LegacyRenameSqlDirectoryChildRequest,
    ) -> Result<LegacyRenameSqlDirectoryChildResponse, String> {
        let root = self.root(&request.root_token)?;
        let _operation = lock(&root.operations)?;
        let source = existing_relative_path(&root, &request.relative_path)?;
        if source.as_os_str().is_empty() {
            return Err("The selected SQL directory root cannot be renamed".to_owned());
        }
        let metadata = root
            .directory
            .symlink_metadata(&source)
            .map_err(|_| "The selected SQL path is not available".to_owned())?;
        let name = if metadata.is_dir() {
            validate_leaf_name(&request.name)?.to_owned()
        } else if metadata.is_file() {
            let fallback = file_extension(
                source
                    .file_name()
                    .and_then(OsStr::to_str)
                    .ok_or_else(|| "The SQL file name is not valid UTF-8".to_owned())?,
            );
            normalize_text_file_name(
                &request.name,
                if fallback.is_empty() {
                    "sql"
                } else {
                    &fallback
                },
            )?
        } else {
            return Err("The selected SQL path is not available".to_owned());
        };
        let parent = source.parent().map_or_else(PathBuf::new, Path::to_path_buf);
        let target = parent.join(name);
        if target != source {
            require_missing(&root, &target)?;
            root.directory
                .rename(&source, &root.directory, &target)
                .map_err(|_| "The SQL file or directory could not be renamed".to_owned())?;
            sync_root_directory(&root)?;
        }
        Ok(LegacyRenameSqlDirectoryChildResponse {
            renamed_node: tree_node(&root, &target)?,
            parent_relative_path: path_text(&parent)?,
            children: list_children_relative(&root, &parent)?,
        })
    }

    pub(crate) fn delete_child(
        &self,
        request: &LegacySqlDirectoryPathRequest,
    ) -> Result<LegacyDeleteSqlDirectoryChildResponse, String> {
        self.delete_child_with(request, |path| {
            trash::delete(path)
                .map_err(|_| "The SQL file or directory could not be moved to Trash".to_owned())
        })
    }

    fn delete_child_with<F>(
        &self,
        request: &LegacySqlDirectoryPathRequest,
        move_to_trash: F,
    ) -> Result<LegacyDeleteSqlDirectoryChildResponse, String>
    where
        F: FnOnce(&Path) -> Result<(), String>,
    {
        let root = self.root(&request.root_token)?;
        let _operation = lock(&root.operations)?;
        let target = existing_relative_path(&root, &request.relative_path)?;
        if target.as_os_str().is_empty() {
            return Err("The selected SQL directory root cannot be deleted".to_owned());
        }
        let metadata = root
            .directory
            .symlink_metadata(&target)
            .map_err(|_| "The selected SQL path is not available".to_owned())?;
        if !metadata.is_dir() && !metadata.is_file() {
            return Err("The selected SQL path is not available".to_owned());
        }
        if metadata.is_file() {
            let name = target
                .file_name()
                .and_then(OsStr::to_str)
                .ok_or_else(|| "The SQL file name is not valid UTF-8".to_owned())?;
            if !is_supported_text_file(name) {
                return Err("Only supported text files can be deleted".to_owned());
            }
        }
        let parent = target.parent().map_or_else(PathBuf::new, Path::to_path_buf);
        let original_name = target
            .file_name()
            .ok_or_else(|| "The selected SQL path is not available".to_owned())?;
        let staging = parent.join(format!(".chat2db-trash-{}", Uuid::new_v4()));
        root.directory
            .create_dir(&staging)
            .map_err(|_| "The SQL Trash staging directory could not be created".to_owned())?;
        let staged_target = staging.join(original_name);
        if let Err(error) = root
            .directory
            .rename(&target, &root.directory, &staged_target)
        {
            let _ = root.directory.remove_dir(&staging);
            return Err(format!(
                "The SQL path could not be staged for Trash: {error}"
            ));
        }
        sync_root_directory(&root)?;
        let staged_absolute = root.canonical_path.join(&staged_target);
        if let Err(error) = move_to_trash(&staged_absolute) {
            let _ = root
                .directory
                .rename(&staged_target, &root.directory, &target);
            let _ = root.directory.remove_dir(&staging);
            let _ = sync_root_directory(&root);
            return Err(error);
        }
        root.directory
            .remove_dir(&staging)
            .map_err(|_| "The SQL Trash staging directory could not be removed".to_owned())?;
        sync_root_directory(&root)?;
        Ok(LegacyDeleteSqlDirectoryChildResponse {
            parent_relative_path: path_text(&parent)?,
            children: list_children_relative(&root, &parent)?,
        })
    }

    pub(crate) fn terminal_directory(
        &self,
        request: &LegacySqlDirectoryPathRequest,
    ) -> Result<PathBuf, String> {
        let root = self.root(&request.root_token)?;
        let _operation = lock(&root.operations)?;
        let target = existing_relative_path(&root, &request.relative_path)?;
        let metadata = if target.as_os_str().is_empty() {
            root.directory
                .dir_metadata()
                .map_err(|_| "The selected SQL directory is not available".to_owned())?
        } else {
            root.directory
                .symlink_metadata(&target)
                .map_err(|_| "The selected SQL path is not available".to_owned())?
        };
        let directory = if metadata.is_dir() {
            target
        } else if metadata.is_file() {
            target.parent().map_or_else(PathBuf::new, Path::to_path_buf)
        } else {
            return Err("The selected SQL path is not available".to_owned());
        };
        Ok(root.canonical_path.join(directory))
    }

    fn root(&self, token: &str) -> Result<Arc<LegacySqlRoot>, String> {
        if token.is_empty() || Uuid::parse_str(token).is_err() {
            return Err("The SQL directory root token is invalid".to_owned());
        }
        lock(&self.roots)?
            .get(token)
            .cloned()
            .ok_or_else(|| "The SQL directory root is not available".to_owned())
    }
}

pub(crate) fn save_dialog_file_name(request: &LegacySaveFileRequest) -> Result<String, String> {
    validate_content_size(&request.file_content)?;
    let file_type = normalize_file_type(&request.file_type)?;
    let name = validate_leaf_name(&request.file_name)?;
    if file_extension(name).eq_ignore_ascii_case(&file_type) {
        Ok(name.to_owned())
    } else if file_extension(name).is_empty() {
        let completed = format!("{name}.{file_type}");
        validate_leaf_name(&completed)?;
        Ok(completed)
    } else {
        Err("The save file name does not match the requested file type".to_owned())
    }
}

pub(crate) fn save_dialog_file_type(request: &LegacySaveFileRequest) -> Result<String, String> {
    normalize_file_type(&request.file_type)
}

pub(crate) fn save_text_file(
    selected_path: &Path,
    request: &LegacySaveFileRequest,
) -> Result<LegacySavedFile, String> {
    validate_content_size(&request.file_content)?;
    let file_type = normalize_file_type(&request.file_type)?;
    let selected_name = selected_path
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| "The selected save file name is not valid UTF-8".to_owned())?;
    validate_leaf_name(selected_name)?;
    if !file_extension(selected_name).eq_ignore_ascii_case(&file_type) {
        return Err("The selected save file does not match the requested file type".to_owned());
    }
    atomic_write_absolute(selected_path, request.file_content.as_bytes(), false)?;
    Ok(LegacySavedFile {
        path: absolute_path_text(selected_path)?,
        size: u64::try_from(request.file_content.len()).unwrap_or(u64::MAX),
    })
}

pub(crate) fn update_text_file(request: &LegacyUpdateFileRequest) -> Result<bool, String> {
    validate_content_size(&request.file_content)?;
    let path = validated_absolute_text_path(&request.file_path)?;
    atomic_write_absolute(&path, request.file_content.as_bytes(), true)?;
    Ok(true)
}

pub(crate) fn read_text_file(request: &LegacyReadFileRequest) -> Result<String, String> {
    let path = validated_absolute_text_path(&request.path)?;
    let parent = path
        .parent()
        .ok_or_else(|| "The selected text file has no parent directory".to_owned())?;
    let file_name = path
        .file_name()
        .ok_or_else(|| "The selected text file name is invalid".to_owned())?;
    let canonical_parent = fs::canonicalize(parent)
        .map_err(|_| "The selected text file directory is not available".to_owned())?;
    let directory = Dir::open_ambient_dir(canonical_parent, ambient_authority())
        .map_err(|_| "The selected text file directory could not be opened safely".to_owned())?;
    let metadata = directory
        .symlink_metadata(file_name)
        .map_err(|_| "The selected text file is not available".to_owned())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("The selected text path is not a regular file".to_owned());
    }
    if metadata.len() > MAX_TEXT_FILE_BYTES {
        return Err("The selected text file exceeds the 16 MiB limit".to_owned());
    }
    let mut file = directory
        .open(file_name)
        .map_err(|_| "The selected text file could not be opened safely".to_owned())?;
    let mut bytes = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or_default());
    Read::by_ref(&mut file)
        .take(MAX_TEXT_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "The selected text file could not be read".to_owned())?;
    if bytes.len() > MAX_TEXT_CONTENT_BYTES {
        return Err("The selected text file exceeds the 16 MiB limit".to_owned());
    }
    decode_text(&bytes, request.charsets.as_deref())
}

pub(crate) fn open_terminal(directory: &Path) -> Result<(), String> {
    if !directory.is_absolute() || !directory.is_dir() {
        return Err("The terminal working directory is not available".to_owned());
    }
    spawn_terminal(directory)
}

fn root_node(root: &Arc<LegacySqlRoot>) -> Result<LegacySqlTreeNode, String> {
    let _operation = lock(&root.operations)?;
    let children = list_children_relative(root, Path::new(""))?;
    let name = root
        .canonical_path
        .file_name()
        .and_then(OsStr::to_str)
        .map_or_else(
            || path_text(&root.canonical_path),
            |name| Ok(name.to_owned()),
        )?;
    Ok(LegacySqlTreeNode {
        key: format!("{}:", root.token),
        root_token: Some(root.token.clone()),
        root_path: Some(path_text(&root.canonical_path)?),
        name,
        path: path_text(&root.canonical_path)?,
        relative_path: String::new(),
        node_type: "directory".to_owned(),
        disabled: false,
        sql_file: false,
        text_file: false,
        file_extension: None,
        has_children: true,
        loaded: true,
        children: Some(children),
    })
}

fn list_children_locked(
    root: &LegacySqlRoot,
    relative_path: &str,
) -> Result<Vec<LegacySqlTreeNode>, String> {
    let relative = existing_relative_path(root, relative_path)?;
    require_directory(root, &relative)?;
    list_children_relative(root, &relative)
}

fn list_children_relative(
    root: &LegacySqlRoot,
    relative: &Path,
) -> Result<Vec<LegacySqlTreeNode>, String> {
    let directory_path = if relative.as_os_str().is_empty() {
        Path::new(".")
    } else {
        relative
    };
    let entries = root
        .directory
        .read_dir(directory_path)
        .map_err(|_| "The SQL directory children could not be read".to_owned())?;
    let mut children = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_| "A SQL directory child could not be read".to_owned())?;
        let file_type = entry
            .file_type()
            .map_err(|_| "A SQL directory child could not be inspected".to_owned())?;
        if file_type.is_symlink() {
            continue;
        }
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if !file_type.is_dir() && (!file_type.is_file() || !is_supported_text_file(&name)) {
            continue;
        }
        children.push((file_type.is_dir(), name));
        if children.len() > MAX_CHILDREN {
            break;
        }
    }
    children.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| left.1.to_lowercase().cmp(&right.1.to_lowercase()))
            .then_with(|| left.1.cmp(&right.1))
    });
    let overflow = children.len() > MAX_CHILDREN;
    children.truncate(MAX_CHILDREN);
    let mut nodes = children
        .into_iter()
        .map(|(_, name)| tree_node(root, &relative.join(name)))
        .collect::<Result<Vec<_>, _>>()?;
    if overflow {
        nodes.push(LegacySqlTreeNode {
            key: format!("{}:overflow:{}", root.token, path_text(relative)?),
            root_token: None,
            root_path: None,
            name: format!("Only first {MAX_CHILDREN} entries are shown"),
            path: String::new(),
            relative_path: path_text(relative)?,
            node_type: "file".to_owned(),
            disabled: true,
            sql_file: false,
            text_file: false,
            file_extension: None,
            has_children: false,
            loaded: true,
            children: None,
        });
    }
    Ok(nodes)
}

fn tree_node(root: &LegacySqlRoot, relative: &Path) -> Result<LegacySqlTreeNode, String> {
    ensure_no_symlink_components(root, relative, false)?;
    let metadata = root
        .directory
        .symlink_metadata(relative)
        .map_err(|_| "The SQL path is not available".to_owned())?;
    if metadata.file_type().is_symlink() {
        return Err("Symbolic links are not supported in SQL directories".to_owned());
    }
    let name = relative
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| "The SQL path name is not valid UTF-8".to_owned())?;
    let is_directory = metadata.is_dir();
    let is_file = metadata.is_file();
    if !is_directory && !is_file {
        return Err("The SQL path is not a file or directory".to_owned());
    }
    let extension = if is_file {
        file_extension(name)
    } else {
        String::new()
    };
    let relative_text = path_text(relative)?;
    Ok(LegacySqlTreeNode {
        key: format!("{}:{relative_text}", root.token),
        root_token: Some(root.token.clone()),
        root_path: None,
        name: name.to_owned(),
        path: path_text(&root.canonical_path.join(relative))?,
        relative_path: relative_text,
        node_type: if is_directory { "directory" } else { "file" }.to_owned(),
        disabled: false,
        sql_file: is_file && extension == "sql",
        text_file: is_file && is_supported_text_file(name),
        file_extension: Some(extension),
        has_children: is_directory,
        loaded: !is_directory,
        children: None,
    })
}

fn existing_relative_path(root: &LegacySqlRoot, value: &str) -> Result<PathBuf, String> {
    let relative = validate_relative_path(value)?;
    ensure_no_symlink_components(root, &relative, false)?;
    if relative.as_os_str().is_empty() {
        return Ok(relative);
    }
    let canonical = root
        .directory
        .canonicalize(&relative)
        .map_err(|_| "The SQL path is not available".to_owned())?;
    validate_relative_path_os(&canonical)?;
    Ok(canonical)
}

fn validate_relative_path(value: &str) -> Result<PathBuf, String> {
    if value.len() > MAX_RELATIVE_PATH_BYTES {
        return Err("The SQL relative path is too long".to_owned());
    }
    validate_relative_path_os(Path::new(value))
}

fn validate_relative_path_os(path: &Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        return Err("The SQL path must be relative to its registered root".to_owned());
    }
    let mut normalized = PathBuf::new();
    let mut count = 0_usize;
    for component in path.components() {
        count = count.saturating_add(1);
        if count > MAX_PATH_COMPONENTS {
            return Err("The SQL relative path has too many components".to_owned());
        }
        match component {
            Component::Normal(value) => normalized.push(value),
            Component::CurDir if path.as_os_str().is_empty() => {}
            Component::CurDir => {
                return Err("The SQL relative path cannot contain dot components".to_owned());
            }
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err("The SQL relative path cannot escape its registered root".to_owned());
            }
        }
    }
    Ok(normalized)
}

fn ensure_no_symlink_components(
    root: &LegacySqlRoot,
    relative: &Path,
    allow_missing_last: bool,
) -> Result<(), String> {
    let mut current = PathBuf::new();
    let components = relative.components().collect::<Vec<_>>();
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(value) = component else {
            return Err("The SQL relative path is invalid".to_owned());
        };
        current.push(value);
        match root.directory.symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err("Symbolic links are not supported in SQL directories".to_owned());
            }
            Ok(_) => {}
            Err(error)
                if allow_missing_last
                    && index + 1 == components.len()
                    && error.kind() == io::ErrorKind::NotFound => {}
            Err(_) => return Err("The SQL path is not available".to_owned()),
        }
    }
    Ok(())
}

fn require_directory(root: &LegacySqlRoot, relative: &Path) -> Result<(), String> {
    let metadata = if relative.as_os_str().is_empty() {
        root.directory.dir_metadata()
    } else {
        root.directory.symlink_metadata(relative)
    }
    .map_err(|_| "The SQL directory is not available".to_owned())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("The SQL path is not a directory".to_owned());
    }
    Ok(())
}

fn require_missing(root: &LegacySqlRoot, relative: &Path) -> Result<(), String> {
    ensure_no_symlink_components(root, relative, true)?;
    match root.directory.symlink_metadata(relative) {
        Ok(_) => Err("The SQL file or directory already exists".to_owned()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("The SQL target could not be inspected".to_owned()),
    }
}

fn available_file_path(root: &LegacySqlRoot, parent: &Path, name: &str) -> Result<PathBuf, String> {
    let extension = Path::new(name)
        .extension()
        .and_then(OsStr::to_str)
        .unwrap_or_default();
    let stem = Path::new(name)
        .file_stem()
        .and_then(OsStr::to_str)
        .ok_or_else(|| "The SQL file name is invalid".to_owned())?;
    for index in 0..=MAX_CHILDREN {
        let candidate = if index == 0 {
            name.to_owned()
        } else {
            format!("{stem}-{index}.{extension}")
        };
        let target = parent.join(candidate);
        match root.directory.symlink_metadata(&target) {
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(target),
            Err(_) => return Err("The SQL target could not be inspected".to_owned()),
        }
    }
    Err("No available SQL file name remains in this directory".to_owned())
}

fn atomic_write_in_root(
    root: &LegacySqlRoot,
    target: &Path,
    contents: &[u8],
    replace: bool,
) -> Result<(), String> {
    if contents.len() > MAX_TEXT_CONTENT_BYTES {
        return Err("The text content exceeds the 16 MiB limit".to_owned());
    }
    let parent = target.parent().map_or_else(PathBuf::new, Path::to_path_buf);
    require_directory(root, &parent)?;
    if !replace {
        require_missing(root, target)?;
    }
    let temporary = parent.join(format!(".chat2db-write-{}.tmp", Uuid::new_v4()));
    let mut file = root
        .directory
        .open_with(&temporary, OpenOptions::new().write(true).create_new(true))
        .map_err(|_| "The temporary SQL file could not be created".to_owned())?;
    let write_result = (|| {
        file.write_all(contents)?;
        file.sync_all()?;
        if replace {
            let metadata = root.directory.symlink_metadata(target)?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(io::Error::other("target is not a regular file"));
            }
        } else if root.directory.symlink_metadata(target).is_ok() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "target already exists",
            ));
        }
        root.directory.rename(&temporary, &root.directory, target)
    })();
    if let Err(error) = write_result {
        let _ = root.directory.remove_file(&temporary);
        return Err(format!(
            "The SQL file could not be written atomically: {error}"
        ));
    }
    sync_root_directory(root)
}

fn atomic_write_absolute(
    path: &Path,
    contents: &[u8],
    require_existing: bool,
) -> Result<(), String> {
    if contents.len() > MAX_TEXT_CONTENT_BYTES {
        return Err("The text content exceeds the 16 MiB limit".to_owned());
    }
    if !path.is_absolute() {
        return Err("The selected file path must be absolute".to_owned());
    }
    let parent = path
        .parent()
        .ok_or_else(|| "The selected file path has no parent directory".to_owned())?;
    let file_name = path
        .file_name()
        .ok_or_else(|| "The selected file name is invalid".to_owned())?;
    let canonical_parent = fs::canonicalize(parent)
        .map_err(|_| "The selected file directory is not available".to_owned())?;
    let directory = Dir::open_ambient_dir(canonical_parent, ambient_authority())
        .map_err(|_| "The selected file directory could not be opened safely".to_owned())?;
    let existing_permissions = match directory.symlink_metadata(file_name) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err("The selected file target is not a regular file".to_owned());
        }
        Ok(metadata) => Some(metadata.permissions()),
        Err(error) if error.kind() == io::ErrorKind::NotFound && !require_existing => None,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err("The selected file does not exist".to_owned());
        }
        Err(_) => return Err("The selected file target could not be inspected".to_owned()),
    };
    let temporary_name = OsString::from(format!(".chat2db-write-{}.tmp", Uuid::new_v4()));
    let mut file = directory
        .open_with(
            &temporary_name,
            OpenOptions::new().write(true).create_new(true),
        )
        .map_err(|_| "The temporary file could not be created".to_owned())?;
    let write_result = (|| {
        file.write_all(contents)?;
        if let Some(permissions) = existing_permissions {
            directory.set_permissions(&temporary_name, permissions)?;
        }
        file.sync_all()?;
        if require_existing {
            let metadata = directory.symlink_metadata(file_name)?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(io::Error::other("target is not a regular file"));
            }
        }
        directory.rename(&temporary_name, &directory, file_name)
    })();
    if let Err(error) = write_result {
        let _ = directory.remove_file(&temporary_name);
        return Err(format!("The file could not be written atomically: {error}"));
    }
    sync_directory(&directory)
        .map_err(|_| "The file directory could not be synchronized".to_owned())
}

fn sync_root_directory(root: &LegacySqlRoot) -> Result<(), String> {
    sync_directory(&root.directory)
        .map_err(|_| "The SQL directory could not be synchronized".to_owned())
}

fn sync_directory(directory: &Dir) -> io::Result<()> {
    let file = directory.try_clone()?.into_std_file();
    match file.sync_all() {
        Ok(()) => Ok(()),
        #[cfg(windows)]
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::InvalidInput
                    | io::ErrorKind::PermissionDenied
                    | io::ErrorKind::Unsupported
            ) =>
        {
            Ok(())
        }
        Err(error) => Err(error),
    }
}

fn validated_absolute_text_path(value: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(value.trim());
    if value.trim().is_empty() || !path.is_absolute() {
        return Err("The selected text file path must be absolute".to_owned());
    }
    let file_name = path
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| "The selected text file name is not valid UTF-8".to_owned())?;
    validate_leaf_name(file_name)?;
    if !is_supported_text_file(file_name) {
        return Err("Only supported text files can be opened or updated".to_owned());
    }
    let metadata = fs::symlink_metadata(&path)
        .map_err(|_| "The selected text file is not available".to_owned())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("The selected text path is not a regular file".to_owned());
    }
    Ok(path)
}

fn decode_text(bytes: &[u8], charset: Option<&str>) -> Result<String, String> {
    let label = charset
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("utf-8");
    let encoding = Encoding::for_label(label.as_bytes())
        .ok_or_else(|| "The requested text charset is not supported".to_owned())?;
    let (decoded, _, had_errors) = encoding.decode(bytes);
    if had_errors {
        return Err(format!(
            "The selected text file is not valid {}",
            encoding.name()
        ));
    }
    let decoded = decoded.strip_prefix('\u{feff}').unwrap_or(decoded.as_ref());
    Ok(decoded.to_owned())
}

fn normalize_file_type(value: &str) -> Result<String, String> {
    let value = value.trim().trim_start_matches('.').to_ascii_lowercase();
    if value.is_empty()
        || value.len() > MAX_FILE_TYPE_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err("The requested file type is invalid".to_owned());
    }
    Ok(value)
}

fn normalize_text_file_name(value: &str, fallback_extension: &str) -> Result<String, String> {
    let value = validate_leaf_name(value)?;
    if is_supported_text_file(value) {
        return Ok(value.to_owned());
    }
    if !file_extension(value).is_empty() {
        return Err("Only supported text file extensions are allowed".to_owned());
    }
    let completed = format!("{value}.{fallback_extension}");
    validate_leaf_name(&completed)?;
    Ok(completed)
}

fn validate_leaf_name(value: &str) -> Result<&str, String> {
    let value = value.trim();
    if value.is_empty()
        || matches!(value, "." | "..")
        || value.len() > MAX_FILE_NAME_BYTES
        || value.ends_with(['.', ' '])
        || value.chars().any(|character| {
            character.is_control()
                || matches!(
                    character,
                    '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
                )
        })
    {
        return Err("The file or directory name is invalid".to_owned());
    }
    let stem = value
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || stem.strip_prefix("COM").is_some_and(|suffix| {
            matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
        })
        || stem.strip_prefix("LPT").is_some_and(|suffix| {
            matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
        })
    {
        return Err("The file or directory name is reserved".to_owned());
    }
    Ok(value)
}

fn validate_content_size(value: &str) -> Result<(), String> {
    if value.len() > MAX_TEXT_CONTENT_BYTES {
        return Err("The text content exceeds the 16 MiB limit".to_owned());
    }
    Ok(())
}

fn is_supported_text_file(name: &str) -> bool {
    matches!(
        file_extension(name).as_str(),
        "sql"
            | "txt"
            | "md"
            | "markdown"
            | "json"
            | "jsonl"
            | "yaml"
            | "yml"
            | "csv"
            | "tsv"
            | "xml"
            | "log"
            | "env"
            | "ini"
            | "conf"
            | "config"
            | "properties"
            | "toml"
    )
}

fn file_extension(name: &str) -> String {
    name.rsplit_once('.')
        .filter(|(stem, extension)| !stem.is_empty() || !extension.is_empty())
        .map_or_else(String::new, |(_, extension)| extension.to_ascii_lowercase())
}

fn absolute_path_text(path: &Path) -> Result<String, String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env_current_dir()?.join(path)
    };
    path_text(&absolute)
}

fn env_current_dir() -> Result<PathBuf, String> {
    std::env::current_dir().map_err(|_| "The current directory is not available".to_owned())
}

fn path_text(path: &Path) -> Result<String, String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| "The local path is not valid UTF-8".to_owned())
}

fn lock<T>(mutex: &Mutex<T>) -> Result<std::sync::MutexGuard<'_, T>, String> {
    mutex
        .lock()
        .map_err(|_| "The desktop file registry is unavailable".to_owned())
}

#[cfg(target_os = "macos")]
fn spawn_terminal(directory: &Path) -> Result<(), String> {
    Command::new("/usr/bin/open")
        .arg("-a")
        .arg("Terminal")
        .arg("--")
        .arg(directory)
        .spawn()
        .map(|_| ())
        .map_err(|_| "The terminal could not be opened".to_owned())
}

#[cfg(target_os = "windows")]
fn spawn_terminal(directory: &Path) -> Result<(), String> {
    Command::new("cmd.exe")
        .current_dir(directory)
        .spawn()
        .map(|_| ())
        .map_err(|_| "The terminal could not be opened".to_owned())
}

#[cfg(target_os = "linux")]
fn spawn_terminal(directory: &Path) -> Result<(), String> {
    let candidates: [(&str, &[&str]); 4] = [
        ("x-terminal-emulator", &["--working-directory"]),
        ("gnome-terminal", &["--working-directory"]),
        ("konsole", &["--workdir"]),
        ("xfce4-terminal", &["--working-directory"]),
    ];
    for (program, arguments) in candidates {
        let mut command = Command::new(program);
        command.args(arguments).arg(directory);
        match command.spawn() {
            Ok(_) => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(_) => return Err("The terminal could not be opened".to_owned()),
        }
    }
    Err("No supported terminal application is installed".to_owned())
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
fn spawn_terminal(_directory: &Path) -> Result<(), String> {
    Err("Opening a terminal is not supported on this platform".to_owned())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{
        LegacyCreateSqlDirectoryChildRequest, LegacyReadFileRequest,
        LegacyRenameSqlDirectoryChildRequest, LegacySaveFileRequest,
        LegacySaveSqlDirectoryFileRequest, LegacySqlDirectoryPathRequest,
        LegacySqlDirectoryRegistry, LegacyUpdateFileRequest, read_text_file, save_dialog_file_name,
        save_text_file, update_text_file,
    };

    fn registry_fixture() -> (tempfile::TempDir, LegacySqlDirectoryRegistry, String) {
        let directory = tempfile::tempdir().expect("temporary SQL root");
        fs::write(directory.path().join("inventory.sql"), "SELECT 1;").expect("SQL fixture");
        fs::create_dir(directory.path().join("nested")).expect("nested fixture");
        let registry = LegacySqlDirectoryRegistry::default();
        let root = registry
            .register_root(directory.path())
            .expect("root registration");
        let token = root.root_token.expect("root token");
        (directory, registry, token)
    }

    #[test]
    fn sql_root_and_children_match_the_retained_frontend_shape() {
        let (directory, registry, token) = registry_fixture();
        let root = registry
            .register_root(directory.path())
            .expect("second root registration");
        let value = serde_json::to_value(root).expect("root serialization");
        assert_eq!(value["type"], "directory");
        assert_eq!(value["relativePath"], "");
        assert_eq!(value["loaded"], true);
        assert!(value["rootToken"].as_str().is_some());
        assert!(value["rootPath"].as_str().is_some());
        assert!(value["children"].as_array().is_some());

        let children = registry
            .list_children(&LegacySqlDirectoryPathRequest {
                root_token: token,
                relative_path: String::new(),
            })
            .expect("children");
        assert_eq!(children[0].name, "nested");
        assert_eq!(children[0].node_type, "directory");
        assert_eq!(children[1].name, "inventory.sql");
        assert!(children[1].sql_file);
    }

    #[test]
    fn sql_root_tokens_and_relative_paths_fail_closed() {
        let (directory, registry, token) = registry_fixture();
        let unknown = LegacySqlDirectoryPathRequest {
            root_token: uuid::Uuid::new_v4().to_string(),
            relative_path: String::new(),
        };
        assert!(registry.list_children(&unknown).is_err());
        for path in ["../outside", "/tmp", "nested/../outside", "./nested"] {
            assert!(
                registry
                    .list_children(&LegacySqlDirectoryPathRequest {
                        root_token: token.clone(),
                        relative_path: path.to_owned(),
                    })
                    .is_err(),
                "path must fail: {path}"
            );
        }
        assert!(directory.path().join("inventory.sql").is_file());
    }

    #[cfg(unix)]
    #[test]
    fn sql_directory_operations_reject_symbolic_links() {
        use std::os::unix::fs::symlink;

        let (directory, registry, token) = registry_fixture();
        let outside = tempfile::tempdir().expect("outside directory");
        symlink(outside.path(), directory.path().join("outside-link")).expect("directory link");
        symlink(
            directory.path().join("inventory.sql"),
            directory.path().join("file-link.sql"),
        )
        .expect("file link");

        let children = registry
            .list_children(&LegacySqlDirectoryPathRequest {
                root_token: token.clone(),
                relative_path: String::new(),
            })
            .expect("children");
        assert!(!children.iter().any(|node| node.name.contains("link")));
        assert!(
            registry
                .list_children(&LegacySqlDirectoryPathRequest {
                    root_token: token,
                    relative_path: "outside-link".to_owned(),
                })
                .is_err()
        );
    }

    #[test]
    fn create_save_and_rename_return_exact_tree_refresh_shapes() {
        let (directory, registry, token) = registry_fixture();
        let created = registry
            .create_child(&LegacyCreateSqlDirectoryChildRequest {
                root_token: token.clone(),
                parent_relative_path: "nested".to_owned(),
                name: "query".to_owned(),
                node_type: "file".to_owned(),
            })
            .expect("create file");
        assert_eq!(created.created_node.name, "query.sql");
        assert!(directory.path().join("nested/query.sql").is_file());

        let saved = registry
            .save_file(&LegacySaveSqlDirectoryFileRequest {
                root_token: token.clone(),
                parent_relative_path: "nested".to_owned(),
                name: "query.sql".to_owned(),
                content: "SELECT 2;".to_owned(),
            })
            .expect("save collision");
        assert_eq!(saved.created_node.name, "query-1.sql");
        assert_eq!(
            fs::read_to_string(directory.path().join("nested/query-1.sql")).expect("saved content"),
            "SELECT 2;"
        );

        let renamed = registry
            .rename_child(&LegacyRenameSqlDirectoryChildRequest {
                root_token: token,
                relative_path: "nested/query-1.sql".to_owned(),
                name: "renamed".to_owned(),
            })
            .expect("rename file");
        assert_eq!(renamed.parent_relative_path, "nested");
        assert_eq!(renamed.renamed_node.name, "renamed.sql");
        assert!(directory.path().join("nested/renamed.sql").is_file());
        assert!(!directory.path().join("nested/query-1.sql").exists());
    }

    #[test]
    fn delete_stages_safely_and_returns_refreshed_children() {
        let (directory, registry, token) = registry_fixture();
        let request = LegacySqlDirectoryPathRequest {
            root_token: token,
            relative_path: "inventory.sql".to_owned(),
        };
        let deleted = registry
            .delete_child_with(&request, |staged| {
                assert_eq!(
                    staged.file_name().and_then(std::ffi::OsStr::to_str),
                    Some("inventory.sql")
                );
                fs::remove_file(staged).map_err(|error| error.to_string())
            })
            .expect("delete fixture");
        assert_eq!(deleted.parent_relative_path, "");
        assert!(!directory.path().join("inventory.sql").exists());
        assert!(
            !deleted
                .children
                .iter()
                .any(|node| node.name == "inventory.sql")
        );
        assert!(
            fs::read_dir(directory.path())
                .expect("root entries")
                .all(|entry| !entry
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".chat2db-trash-"))
        );
    }

    #[test]
    fn absolute_file_writes_are_atomic_bounded_and_utf8_explicit() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("query.sql");
        fs::write(&path, "SELECT 1;").expect("file fixture");
        update_text_file(&LegacyUpdateFileRequest {
            file_path: path.to_string_lossy().into_owned(),
            file_content: "SELECT 2;".to_owned(),
        })
        .expect("atomic update");
        assert_eq!(
            fs::read_to_string(&path).expect("updated file"),
            "SELECT 2;"
        );
        assert!(
            fs::read_dir(directory.path())
                .expect("directory entries")
                .all(|entry| !entry
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".chat2db-write-"))
        );

        let read = read_text_file(&LegacyReadFileRequest {
            path: path.to_string_lossy().into_owned(),
            charsets: Some("UTF-8".to_owned()),
        })
        .expect("UTF-8 read");
        assert_eq!(read, "SELECT 2;");
        fs::write(&path, [0xff, 0xfe, 0xfd]).expect("invalid UTF-8 fixture");
        assert!(
            read_text_file(&LegacyReadFileRequest {
                path: path.to_string_lossy().into_owned(),
                charsets: None,
            })
            .is_err()
        );
    }

    #[test]
    fn save_file_contract_normalizes_names_and_reports_size() {
        let request = LegacySaveFileRequest {
            file_name: "connections".to_owned(),
            file_content: "{}".to_owned(),
            file_type: ".json".to_owned(),
        };
        assert_eq!(
            save_dialog_file_name(&request).expect("dialog name"),
            "connections.json"
        );
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("connections.json");
        let saved = save_text_file(&path, &request).expect("saved file");
        assert_eq!(saved.path, path.to_string_lossy());
        assert_eq!(saved.size, 2);
        assert_eq!(fs::read_to_string(path).expect("saved content"), "{}");
        assert!(
            save_dialog_file_name(&LegacySaveFileRequest {
                file_name: "../secret".to_owned(),
                file_content: String::new(),
                file_type: "sql".to_owned(),
            })
            .is_err()
        );
    }
}
