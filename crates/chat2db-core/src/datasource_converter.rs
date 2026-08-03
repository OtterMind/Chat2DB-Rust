//! Strict importers for Community datasource exchange formats.

use std::{
    collections::{HashMap, HashSet},
    io::{Cursor, Read},
};

use aes::Aes128;
use blowfish::{
    Blowfish,
    cipher::{BlockDecrypt, BlockEncrypt, KeyInit, generic_array::GenericArray},
};
use cbc::cipher::{BlockDecryptMut, KeyIvInit, block_padding::Pkcs7};
use chat2db_contract::{
    CommunityDatasourceFileImportRequest, CommunityDatasourceFileImportResult,
    CommunityDatasourceImportFormat, CreateDatasourceRequest, DatasourceConnection,
    DatasourceConnectionProperty,
};
use quick_xml::{Reader, events::Event};
use serde_json::Value;
use sha1::{Digest, Sha1};
use url::Url;
use zip::ZipArchive;

use crate::{AppError, Application, datasource_edit::is_sensitive_key};

const MAX_IMPORT_BYTES: usize = 16 * 1024 * 1024;
const MAX_DATASOURCES: usize = 1_000;
const MAX_ZIP_ENTRIES: usize = 256;
const MAX_ZIP_ENTRY_BYTES: u64 = 8 * 1024 * 1024;
const MAX_ZIP_EXPANDED_BYTES: u64 = 32 * 1024 * 1024;
const MAX_XML_DEPTH: usize = 64;
const MAX_NAME_BYTES: usize = 512;
const MAX_URL_BYTES: usize = 8 * 1024;
const MAX_USERNAME_BYTES: usize = 1_024;
const MAX_PROPERTIES: usize = 128;
const MAX_PROPERTY_KEY_BYTES: usize = 255;
const MAX_PROPERTY_VALUE_BYTES: usize = 8 * 1024;

type Aes128CbcDec = cbc::Decryptor<Aes128>;

struct ParsedImport {
    datasources: Vec<PreparedDatasource>,
    skipped: usize,
}

struct PreparedDatasource {
    name: String,
    connection: Option<DatasourceConnection>,
}

impl Application {
    /// Parses and imports one Community datasource exchange file.
    ///
    /// Every entry is parsed and validated before the first datasource record is created. Only
    /// native `MySQL` entries are accepted; unsupported database types are counted as skipped.
    ///
    /// # Errors
    ///
    /// Returns format, size, validation, driver, vault, or durable-storage failures.
    pub async fn import_community_datasource_file(
        &self,
        request: CommunityDatasourceFileImportRequest,
    ) -> Result<CommunityDatasourceFileImportResult, AppError> {
        if request.content.is_empty() {
            return Err(invalid_file());
        }
        if request.content.len() > MAX_IMPORT_BYTES {
            return Err(import_limit("the datasource import file is too large"));
        }
        self.require_managed_driver("mysql")?;
        let parsed = tokio::task::spawn_blocking(move || parse_import(&request))
            .await
            .map_err(|_| AppError::internal())??;

        let mut created = Vec::with_capacity(parsed.datasources.len());
        for datasource in parsed.datasources {
            created.push(
                self.create_datasource(CreateDatasourceRequest {
                    name: datasource.name,
                    driver_id: "mysql".to_owned(),
                    connection: datasource.connection,
                })
                .await?,
            );
        }
        Ok(CommunityDatasourceFileImportResult {
            count: u32::try_from(created.len()).map_err(|_| AppError::internal())?,
            created,
            skipped: u32::try_from(parsed.skipped).map_err(|_| AppError::internal())?,
        })
    }
}

fn parse_import(request: &CommunityDatasourceFileImportRequest) -> Result<ParsedImport, AppError> {
    let mut parsed = match request.format {
        CommunityDatasourceImportFormat::Chat2dbJson => parse_chat2db_json(&request.content)?,
        CommunityDatasourceImportFormat::NavicatNcx => parse_navicat_ncx(&request.content)?,
        CommunityDatasourceImportFormat::DbeaverDbp => parse_dbeaver_dbp(&request.content)?,
        CommunityDatasourceImportFormat::DatagripText => parse_datagrip_text(&request.content)?,
    };
    if parsed.datasources.len() > MAX_DATASOURCES {
        return Err(import_limit(
            "at most 1000 datasources can be imported at once",
        ));
    }
    for (index, datasource) in parsed.datasources.iter_mut().enumerate() {
        validate_datasource(datasource, index)?;
    }
    Ok(parsed)
}

fn validate_datasource(datasource: &mut PreparedDatasource, index: usize) -> Result<(), AppError> {
    datasource.name = datasource.name.trim().to_owned();
    if datasource.name.is_empty() {
        datasource.name = format!("Imported MySQL {}", index + 1);
    }
    if datasource.name.len() > MAX_NAME_BYTES || datasource.name.contains('\0') {
        return Err(invalid_file());
    }
    let Some(connection) = datasource.connection.as_mut() else {
        return Ok(());
    };
    connection.jdbc_url = connection.jdbc_url.trim().to_owned();
    if connection.jdbc_url.is_empty()
        || connection.jdbc_url.len() > MAX_URL_BYTES
        || connection.jdbc_url.contains('\0')
        || !is_mysql_url(&connection.jdbc_url)
    {
        return Err(invalid_file());
    }
    if connection.properties.len() > MAX_PROPERTIES {
        return Err(import_limit(
            "a datasource contains too many connection properties",
        ));
    }
    let mut keys = HashSet::with_capacity(connection.properties.len());
    for property in &mut connection.properties {
        property.key = property.key.trim().to_owned();
        if property.key.is_empty()
            || property.key.len() > MAX_PROPERTY_KEY_BYTES
            || property.value.len() > MAX_PROPERTY_VALUE_BYTES
            || property.key.contains('\0')
            || property.value.contains('\0')
            || !keys.insert(property.key.to_ascii_lowercase())
        {
            return Err(invalid_file());
        }
        if is_sensitive_key(&property.key) {
            property.sensitive = true;
        }
        if is_username_key(&property.key) && property.value.len() > MAX_USERNAME_BYTES {
            return Err(invalid_file());
        }
    }
    Ok(())
}

fn invalid_file() -> AppError {
    AppError::invalid(
        "invalid_datasource_import_file",
        "The datasource import file is invalid",
    )
}

fn import_limit(message: &'static str) -> AppError {
    AppError::invalid("datasource_import_limit_exceeded", message)
}

fn is_mysql_url(jdbc_url: &str) -> bool {
    jdbc_url
        .strip_prefix("jdbc:")
        .and_then(|raw_url| Url::parse(raw_url).ok())
        .is_some_and(|url| url.scheme().eq_ignore_ascii_case("mysql"))
}

fn is_username_key(key: &str) -> bool {
    matches!(
        key.trim().to_ascii_lowercase().as_str(),
        "user" | "username" | "user_name"
    )
}

fn push_property(
    properties: &mut Vec<DatasourceConnectionProperty>,
    key: impl Into<String>,
    value: impl Into<String>,
    sensitive: bool,
) {
    let key = key.into();
    let value = value.into();
    if key.trim().is_empty() || value.is_empty() {
        return;
    }
    if let Some(existing) = properties
        .iter_mut()
        .find(|property| property.key.eq_ignore_ascii_case(&key))
    {
        existing.value = value;
        existing.sensitive |= sensitive || is_sensitive_key(&key);
        return;
    }
    properties.push(DatasourceConnectionProperty {
        sensitive: sensitive || is_sensitive_key(&key),
        key,
        value,
    });
}

fn connection(
    jdbc_url: String,
    username: Option<String>,
    password: Option<String>,
    mut properties: Vec<DatasourceConnectionProperty>,
    read_only: bool,
) -> DatasourceConnection {
    if let Some(username) = username {
        push_property(&mut properties, "user", username, false);
    }
    if let Some(password) = password {
        push_property(&mut properties, "password", password, true);
    }
    DatasourceConnection {
        jdbc_url,
        properties,
        read_only,
        ssh: None,
    }
}

fn value_string(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn indicates_mysql(value: &Value, jdbc_url: Option<&str>) -> bool {
    jdbc_url.is_some_and(is_mysql_url)
        || ["type", "driver", "jdbc", "driverId", "provider"]
            .iter()
            .filter_map(|key| value.get(*key).and_then(Value::as_str))
            .any(|candidate| candidate.to_ascii_lowercase().contains("mysql"))
        || value
            .get("driverConfig")
            .and_then(Value::as_object)
            .is_some_and(|driver| {
                driver
                    .values()
                    .filter_map(Value::as_str)
                    .any(|candidate| candidate.to_ascii_lowercase().contains("mysql"))
            })
}

fn parse_chat2db_json(content: &[u8]) -> Result<ParsedImport, AppError> {
    let value: Value = serde_json::from_slice(content).map_err(|_| invalid_file())?;
    let items = match &value {
        Value::Array(items) => items,
        Value::Object(object) => object
            .get("datasources")
            .and_then(Value::as_array)
            .ok_or_else(invalid_file)?,
        _ => return Err(invalid_file()),
    };
    if items.len() > MAX_DATASOURCES {
        return Err(import_limit(
            "at most 1000 datasources can be imported at once",
        ));
    }

    let mut datasources = Vec::with_capacity(items.len());
    let mut skipped = 0;
    for item in items {
        let portable_connection = item.get("connection").and_then(Value::as_object);
        let jdbc_url = portable_connection
            .and_then(|object| object.get("jdbcUrl"))
            .and_then(Value::as_str)
            .or_else(|| item.get("url").and_then(Value::as_str));
        if !indicates_mysql(item, jdbc_url) {
            skipped += 1;
            continue;
        }
        let name = value_string(item, "name")
            .or_else(|| value_string(item, "alias"))
            .unwrap_or_default();
        let Some(jdbc_url) = jdbc_url.map(str::to_owned) else {
            datasources.push(PreparedDatasource {
                name,
                connection: None,
            });
            continue;
        };
        let mut properties = Vec::new();
        let property_values = portable_connection
            .and_then(|object| object.get("properties"))
            .and_then(Value::as_array)
            .or_else(|| item.get("extendInfo").and_then(Value::as_array));
        if let Some(property_values) = property_values {
            for property in property_values {
                let Some(key) = property.get("key").and_then(Value::as_str) else {
                    continue;
                };
                if is_sensitive_key(key) {
                    continue;
                }
                let value = property
                    .get("value")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                push_property(&mut properties, key, value, false);
            }
        }
        let username = value_string(item, "user");
        let read_only = portable_connection
            .and_then(|object| object.get("readOnly"))
            .and_then(Value::as_bool)
            .or_else(|| item.get("readOnly").and_then(Value::as_bool))
            .unwrap_or(false);
        // Legacy Chat2DB exports intentionally do not restore the exported password.
        datasources.push(PreparedDatasource {
            name,
            connection: Some(connection(jdbc_url, username, None, properties, read_only)),
        });
    }
    Ok(ParsedImport {
        datasources,
        skipped,
    })
}

fn parse_navicat_ncx(content: &[u8]) -> Result<ParsedImport, AppError> {
    let mut reader = Reader::from_reader(content);
    reader.config_mut().trim_text(true);
    let mut version = None;
    let mut raw_connections = Vec::new();
    let mut depth = 0_usize;
    loop {
        match reader.read_event().map_err(|_| invalid_file())? {
            Event::Start(element) => {
                depth = depth.checked_add(1).ok_or_else(invalid_file)?;
                if depth > MAX_XML_DEPTH {
                    return Err(import_limit("the datasource XML is nested too deeply"));
                }
                collect_navicat_element(&element, &mut version, &mut raw_connections)?;
            }
            Event::Empty(element) => {
                collect_navicat_element(&element, &mut version, &mut raw_connections)?;
            }
            Event::End(_) => depth = depth.saturating_sub(1),
            Event::DocType(_) => return Err(invalid_file()),
            Event::Eof => break,
            _ => {}
        }
    }
    let version = version.ok_or_else(invalid_file)?;
    if raw_connections.len() > MAX_DATASOURCES {
        return Err(import_limit(
            "at most 1000 datasources can be imported at once",
        ));
    }

    let mut datasources = Vec::with_capacity(raw_connections.len());
    let mut skipped = 0;
    for attributes in raw_connections {
        let connection_type = attribute(&attributes, "ConnType").unwrap_or_default();
        if !connection_type.to_ascii_lowercase().contains("mysql") || navicat_uses_ssh(&attributes)
        {
            skipped += 1;
            continue;
        }
        let jdbc_url = navicat_mysql_url(&attributes)?;
        let encrypted_password = attribute(&attributes, "Password").unwrap_or_default();
        let password = (!encrypted_password.is_empty())
            .then(|| decrypt_navicat_password(version, encrypted_password))
            .transpose()?;
        datasources.push(PreparedDatasource {
            name: attribute(&attributes, "ConnectionName")
                .unwrap_or_default()
                .to_owned(),
            connection: Some(connection(
                jdbc_url,
                attribute(&attributes, "UserName").map(str::to_owned),
                password,
                Vec::new(),
                false,
            )),
        });
    }
    Ok(ParsedImport {
        datasources,
        skipped,
    })
}

fn collect_navicat_element(
    element: &quick_xml::events::BytesStart<'_>,
    version: &mut Option<f64>,
    connections: &mut Vec<HashMap<String, String>>,
) -> Result<(), AppError> {
    let name = element.name();
    if xml_local_name(name.as_ref()).eq_ignore_ascii_case(b"Connections") {
        let attributes = xml_attributes(element)?;
        let raw_version = attribute(&attributes, "Ver").ok_or_else(invalid_file)?;
        let parsed = raw_version.parse::<f64>().map_err(|_| invalid_file())?;
        if !parsed.is_finite() || parsed <= 0.0 {
            return Err(invalid_file());
        }
        *version = Some(parsed);
    } else if xml_local_name(name.as_ref()).eq_ignore_ascii_case(b"Connection") {
        connections.push(xml_attributes(element)?);
    }
    Ok(())
}

fn xml_attributes(
    element: &quick_xml::events::BytesStart<'_>,
) -> Result<HashMap<String, String>, AppError> {
    let mut values = HashMap::new();
    for raw_attribute in element.attributes().with_checks(true) {
        let raw_attribute = raw_attribute.map_err(|_| invalid_file())?;
        let key = std::str::from_utf8(raw_attribute.key.as_ref())
            .map_err(|_| invalid_file())?
            .to_owned();
        let value = raw_attribute
            .unescape_value()
            .map_err(|_| invalid_file())?
            .into_owned();
        if values.insert(key, value).is_some() {
            return Err(invalid_file());
        }
    }
    Ok(values)
}

fn attribute<'a>(attributes: &'a HashMap<String, String>, key: &str) -> Option<&'a str> {
    attributes
        .iter()
        .find(|(candidate, _)| candidate.eq_ignore_ascii_case(key))
        .map(|(_, value)| value.as_str())
}

fn navicat_uses_ssh(attributes: &HashMap<String, String>) -> bool {
    attribute(attributes, "SSH").is_some_and(|value| {
        !value.is_empty() && !value.eq_ignore_ascii_case("false") && value != "0"
    })
}

fn navicat_mysql_url(attributes: &HashMap<String, String>) -> Result<String, AppError> {
    if let Some(candidate) = ["URL", "Url", "ConnectionString"]
        .iter()
        .find_map(|key| attribute(attributes, key))
        && is_mysql_url(candidate)
    {
        return Ok(candidate.to_owned());
    }
    let host = attribute(attributes, "Host").ok_or_else(invalid_file)?;
    let port = attribute(attributes, "Port")
        .filter(|value| !value.is_empty())
        .unwrap_or("3306");
    let mut parsed = Url::parse(&format!("mysql://{host}:{port}")).map_err(|_| invalid_file())?;
    if let Some(database) = ["Database", "DatabaseName", "InitialDatabase"]
        .iter()
        .find_map(|key| attribute(attributes, key))
        .filter(|value| !value.is_empty())
    {
        parsed.set_path(database);
    }
    Ok(format!("jdbc:{parsed}"))
}

fn decrypt_navicat_password(version: f64, ciphertext: &str) -> Result<String, AppError> {
    if version <= 1.1 {
        decrypt_navicat_11(ciphertext)
    } else {
        decrypt_navicat_12(ciphertext)
    }
}

fn decrypt_navicat_12(ciphertext: &str) -> Result<String, AppError> {
    let mut bytes = hex::decode(ciphertext).map_err(|_| invalid_file())?;
    let plaintext = Aes128CbcDec::new_from_slices(b"libcckeylibcckey", b"libcciv libcciv ")
        .map_err(|_| invalid_file())?
        .decrypt_padded_mut::<Pkcs7>(&mut bytes)
        .map_err(|_| invalid_file())?;
    std::str::from_utf8(plaintext)
        .map(str::to_owned)
        .map_err(|_| invalid_file())
}

#[allow(deprecated)]
fn decrypt_navicat_11(ciphertext: &str) -> Result<String, AppError> {
    let input = hex::decode(ciphertext).map_err(|_| invalid_file())?;
    let key = Sha1::digest(b"3DC5CA39");
    let cipher: Blowfish = Blowfish::new_from_slice(&key).map_err(|_| invalid_file())?;
    let mut iv = GenericArray::clone_from_slice(&[0xff_u8; 8]);
    cipher.encrypt_block(&mut iv);
    let mut chaining_value = <[u8; 8]>::from(iv);
    let mut output = vec![0_u8; input.len()];

    let full_blocks = input.len() / 8;
    for block_index in 0..full_blocks {
        let offset = block_index * 8;
        let ciphertext_block = &input[offset..offset + 8];
        let mut block = GenericArray::clone_from_slice(ciphertext_block);
        cipher.decrypt_block(&mut block);
        for index in 0..8 {
            output[offset + index] = block[index] ^ chaining_value[index];
            chaining_value[index] ^= ciphertext_block[index];
        }
    }
    let remaining = input.len() % 8;
    if remaining != 0 {
        let offset = full_blocks * 8;
        let mut block = GenericArray::clone_from_slice(&chaining_value);
        cipher.encrypt_block(&mut block);
        for index in 0..remaining {
            output[offset + index] = input[offset + index] ^ block[index];
        }
    }
    String::from_utf8(output).map_err(|_| invalid_file())
}

fn xml_local_name(name: &[u8]) -> &[u8] {
    name.rsplit(|byte| *byte == b':').next().unwrap_or(name)
}

fn parse_dbeaver_dbp(content: &[u8]) -> Result<ParsedImport, AppError> {
    let files = read_dbeaver_archive(content)?;
    let mut datasource_paths = files
        .keys()
        .filter(|path| path.ends_with("/data-sources.json") || *path == "data-sources.json")
        .cloned()
        .collect::<Vec<_>>();
    datasource_paths.sort();
    if datasource_paths.is_empty() {
        return Err(invalid_file());
    }

    let mut datasources = Vec::new();
    let mut skipped = 0;
    let mut seen_entries = 0_usize;
    for path in datasource_paths {
        let document: Value = serde_json::from_slice(files.get(&path).ok_or_else(invalid_file)?)
            .map_err(|_| invalid_file())?;
        let credentials_path = path
            .strip_suffix("data-sources.json")
            .map(|prefix| format!("{prefix}credentials-config.json"))
            .ok_or_else(invalid_file)?;
        let credentials = files
            .get(&credentials_path)
            .map(|bytes| parse_dbeaver_credentials(bytes))
            .transpose()?;
        let connections = document
            .get("connections")
            .and_then(Value::as_object)
            .ok_or_else(invalid_file)?;
        let mut ids = connections.keys().cloned().collect::<Vec<_>>();
        ids.sort();
        for id in ids {
            seen_entries = seen_entries
                .checked_add(1)
                .ok_or_else(|| import_limit("at most 1000 datasources can be imported at once"))?;
            if seen_entries > MAX_DATASOURCES {
                return Err(import_limit(
                    "at most 1000 datasources can be imported at once",
                ));
            }
            let raw = connections.get(&id).ok_or_else(invalid_file)?;
            let configuration = raw
                .get("configuration")
                .and_then(Value::as_object)
                .ok_or_else(invalid_file)?;
            let configured_url = configuration.get("url").and_then(Value::as_str);
            if !dbeaver_is_mysql(&document, raw, configured_url) {
                skipped += 1;
                continue;
            }
            let jdbc_url = match configured_url {
                Some(url) if is_mysql_url(url) => url.to_owned(),
                Some(_) => {
                    skipped += 1;
                    continue;
                }
                None => dbeaver_mysql_url(configuration)?,
            };
            let credential = credentials
                .as_ref()
                .and_then(|document| document.get(&id))
                .and_then(|value| value.get("#connection"));
            let username = credential.and_then(|value| value_string(value, "user"));
            let password = credential.and_then(|value| value_string(value, "password"));
            let properties = dbeaver_properties(configuration)?;
            let read_only = configuration
                .get("read-only")
                .or_else(|| configuration.get("readOnly"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            datasources.push(PreparedDatasource {
                name: value_string(raw, "name").unwrap_or_default(),
                connection: Some(connection(
                    jdbc_url, username, password, properties, read_only,
                )),
            });
            if datasources.len() > MAX_DATASOURCES {
                return Err(import_limit(
                    "at most 1000 datasources can be imported at once",
                ));
            }
        }
    }
    Ok(ParsedImport {
        datasources,
        skipped,
    })
}

fn read_dbeaver_archive(content: &[u8]) -> Result<HashMap<String, Vec<u8>>, AppError> {
    let mut archive = ZipArchive::new(Cursor::new(content)).map_err(|_| invalid_file())?;
    if archive.len() > MAX_ZIP_ENTRIES {
        return Err(import_limit(
            "the DBeaver archive contains too many entries",
        ));
    }
    let mut expanded_bytes = 0_u64;
    let mut files = HashMap::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|_| invalid_file())?;
        let path = entry.enclosed_name().ok_or_else(invalid_file)?;
        let path = path.to_string_lossy().replace('\\', "/");
        if entry.is_dir() {
            continue;
        }
        if entry.size() > MAX_ZIP_ENTRY_BYTES {
            return Err(import_limit("a DBeaver archive entry is too large"));
        }
        expanded_bytes = expanded_bytes
            .checked_add(entry.size())
            .ok_or_else(|| import_limit("the DBeaver archive is too large"))?;
        if expanded_bytes > MAX_ZIP_EXPANDED_BYTES {
            return Err(import_limit("the DBeaver archive expands beyond its limit"));
        }
        let relevant = path.ends_with("/data-sources.json")
            || path == "data-sources.json"
            || path.ends_with("/credentials-config.json")
            || path == "credentials-config.json";
        if !relevant {
            continue;
        }
        let mut bytes = Vec::with_capacity(usize::try_from(entry.size()).unwrap_or(0));
        entry
            .by_ref()
            .take(MAX_ZIP_ENTRY_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| invalid_file())?;
        if u64::try_from(bytes.len()).map_err(|_| invalid_file())? > MAX_ZIP_ENTRY_BYTES {
            return Err(import_limit("a DBeaver archive entry is too large"));
        }
        if files.insert(path, bytes).is_some() {
            return Err(invalid_file());
        }
    }
    Ok(files)
}

fn parse_dbeaver_credentials(bytes: &[u8]) -> Result<Value, AppError> {
    let plaintext = if bytes
        .iter()
        .copied()
        .find(|byte| !byte.is_ascii_whitespace())
        == Some(b'{')
    {
        bytes.to_vec()
    } else {
        decrypt_dbeaver_credentials(bytes)?
    };
    serde_json::from_slice(&plaintext).map_err(|_| invalid_file())
}

fn decrypt_dbeaver_credentials(bytes: &[u8]) -> Result<Vec<u8>, AppError> {
    const KEY: [u8; 16] = [
        0xba, 0xbb, 0x4a, 0x9f, 0x77, 0x4a, 0xb8, 0x53, 0xc9, 0x6c, 0x2d, 0x65, 0x3d, 0xfe, 0x54,
        0x4a,
    ];
    if bytes.len() < 32 || !(bytes.len() - 16).is_multiple_of(16) {
        return Err(invalid_file());
    }
    let (iv, ciphertext) = bytes.split_at(16);
    let mut ciphertext = ciphertext.to_vec();
    let plaintext = Aes128CbcDec::new_from_slices(&KEY, iv)
        .map_err(|_| invalid_file())?
        .decrypt_padded_mut::<Pkcs7>(&mut ciphertext)
        .map_err(|_| invalid_file())?;
    Ok(plaintext.to_vec())
}

fn dbeaver_is_mysql(document: &Value, connection: &Value, jdbc_url: Option<&str>) -> bool {
    if jdbc_url.is_some_and(is_mysql_url) {
        return true;
    }
    let provider = connection
        .get("provider")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if provider.to_ascii_lowercase().contains("mysql") {
        return true;
    }
    if !provider.eq_ignore_ascii_case("generic") {
        return false;
    }
    let driver_id = connection
        .get("driver")
        .and_then(Value::as_str)
        .unwrap_or_default();
    document
        .get("drivers")
        .and_then(|drivers| drivers.get("generic"))
        .and_then(|drivers| drivers.get(driver_id))
        .is_some_and(|driver| {
            driver
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_ascii_lowercase()
                .contains("mysql")
        })
}

fn dbeaver_mysql_url(configuration: &serde_json::Map<String, Value>) -> Result<String, AppError> {
    let host = configuration
        .get("host")
        .and_then(Value::as_str)
        .ok_or_else(invalid_file)?;
    let port = configuration
        .get("port")
        .and_then(Value::as_str)
        .filter(|port| !port.is_empty())
        .unwrap_or("3306");
    let mut url = Url::parse(&format!("mysql://{host}:{port}")).map_err(|_| invalid_file())?;
    if let Some(database) = configuration
        .get("database")
        .and_then(Value::as_str)
        .filter(|database| !database.is_empty())
    {
        url.set_path(database);
    }
    Ok(format!("jdbc:{url}"))
}

fn dbeaver_properties(
    configuration: &serde_json::Map<String, Value>,
) -> Result<Vec<DatasourceConnectionProperty>, AppError> {
    let Some(properties) = configuration.get("properties") else {
        return Ok(Vec::new());
    };
    let properties = properties.as_object().ok_or_else(invalid_file)?;
    let mut output = Vec::with_capacity(properties.len());
    for (key, value) in properties {
        let value = match value {
            Value::String(value) => value.clone(),
            Value::Bool(value) => value.to_string(),
            Value::Number(value) => value.to_string(),
            Value::Null => continue,
            Value::Array(_) | Value::Object(_) => return Err(invalid_file()),
        };
        push_property(&mut output, key, value, is_sensitive_key(key));
    }
    Ok(output)
}

fn parse_datagrip_text(content: &[u8]) -> Result<ParsedImport, AppError> {
    let text = std::str::from_utf8(content).map_err(|_| invalid_file())?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = text.lines();
    if lines.next().map(str::trim_end) != Some("#DataSourceSettings#") {
        return Err(invalid_file());
    }
    let mut blocks = Vec::new();
    let mut current = None::<String>;
    for line in lines {
        match line.trim() {
            "#BEGIN#" => {
                if let Some(block) = current.take()
                    && !block.trim().is_empty()
                {
                    blocks.push(block);
                }
                current = Some(String::new());
            }
            "#END#" => {
                if let Some(block) = current.take()
                    && !block.trim().is_empty()
                {
                    blocks.push(block);
                }
            }
            _ => {
                if let Some(block) = current.as_mut() {
                    block.push_str(line);
                    block.push('\n');
                }
            }
        }
    }
    if let Some(block) = current
        && !block.trim().is_empty()
    {
        blocks.push(block);
    }
    if blocks.is_empty() || blocks.len() > MAX_DATASOURCES {
        return Err(invalid_file());
    }

    let mut datasources = Vec::with_capacity(blocks.len());
    let mut skipped = 0;
    for block in blocks {
        match parse_datagrip_datasource(&block)? {
            Some(datasource) => datasources.push(datasource),
            None => skipped += 1,
        }
    }
    Ok(ParsedImport {
        datasources,
        skipped,
    })
}

fn parse_datagrip_datasource(xml: &str) -> Result<Option<PreparedDatasource>, AppError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut name = String::new();
    let mut dbms = String::new();
    let mut jdbc_url = String::new();
    let mut username = String::new();
    let mut active_text = None::<Vec<u8>>;
    let mut depth = 0_usize;
    loop {
        match reader.read_event().map_err(|_| invalid_file())? {
            Event::Start(element) => {
                depth = depth.checked_add(1).ok_or_else(invalid_file)?;
                if depth > MAX_XML_DEPTH {
                    return Err(import_limit("the datasource XML is nested too deeply"));
                }
                let local_name = xml_local_name(element.name().as_ref()).to_vec();
                if depth == 1 {
                    name = xml_attribute(&element, "name")?.unwrap_or_default();
                }
                if local_name.eq_ignore_ascii_case(b"database-info") {
                    dbms = xml_attribute(&element, "dbms")?.unwrap_or_default();
                }
                if [b"jdbc-url".as_slice(), b"user-name".as_slice()]
                    .iter()
                    .any(|candidate| local_name.eq_ignore_ascii_case(candidate))
                {
                    active_text = Some(local_name);
                }
            }
            Event::Empty(element)
                if xml_local_name(element.name().as_ref())
                    .eq_ignore_ascii_case(b"database-info") =>
            {
                dbms = xml_attribute(&element, "dbms")?.unwrap_or_default();
            }
            Event::Text(text) => {
                let value = text.unescape().map_err(|_| invalid_file())?;
                match active_text.as_deref() {
                    Some(tag) if tag.eq_ignore_ascii_case(b"jdbc-url") => {
                        jdbc_url.push_str(&value);
                    }
                    Some(tag) if tag.eq_ignore_ascii_case(b"user-name") => {
                        username.push_str(&value);
                    }
                    _ => {}
                }
            }
            Event::CData(text) => {
                let value = std::str::from_utf8(text.as_ref()).map_err(|_| invalid_file())?;
                match active_text.as_deref() {
                    Some(tag) if tag.eq_ignore_ascii_case(b"jdbc-url") => {
                        jdbc_url.push_str(value);
                    }
                    Some(tag) if tag.eq_ignore_ascii_case(b"user-name") => {
                        username.push_str(value);
                    }
                    _ => {}
                }
            }
            Event::End(element) => {
                if active_text.as_deref().is_some_and(|tag| {
                    xml_local_name(element.name().as_ref()).eq_ignore_ascii_case(tag)
                }) {
                    active_text = None;
                }
                depth = depth.saturating_sub(1);
            }
            Event::DocType(_) => return Err(invalid_file()),
            Event::Eof => break,
            _ => {}
        }
    }
    if !is_mysql_url(jdbc_url.trim())
        || (!dbms.is_empty() && !dbms.to_ascii_lowercase().contains("mysql"))
    {
        return Ok(None);
    }
    Ok(Some(PreparedDatasource {
        name,
        connection: Some(connection(
            jdbc_url.trim().to_owned(),
            (!username.is_empty()).then_some(username),
            None,
            Vec::new(),
            false,
        )),
    }))
}

fn xml_attribute(
    element: &quick_xml::events::BytesStart<'_>,
    name: &str,
) -> Result<Option<String>, AppError> {
    for raw_attribute in element.attributes().with_checks(true) {
        let raw_attribute = raw_attribute.map_err(|_| invalid_file())?;
        if xml_local_name(raw_attribute.key.as_ref()).eq_ignore_ascii_case(name.as_bytes()) {
            return raw_attribute
                .unescape_value()
                .map(|value| Some(value.into_owned()))
                .map_err(|_| invalid_file());
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write};

    use cbc::cipher::{BlockEncryptMut, KeyIvInit, block_padding::Pkcs7};
    use chat2db_contract::{CommunityDatasourceFileImportRequest, CommunityDatasourceImportFormat};
    use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

    use super::{decrypt_navicat_11, parse_import};

    type Aes128CbcEnc = cbc::Encryptor<aes::Aes128>;

    fn parse(format: CommunityDatasourceImportFormat, content: Vec<u8>) -> super::ParsedImport {
        parse_import(&CommunityDatasourceFileImportRequest { format, content })
            .expect("fixture parses")
    }

    #[test]
    fn chat2db_legacy_json_import_discards_passwords_and_sensitive_properties() {
        let content = br#"[
          {
            "id": 9,
            "spaceId": 4,
            "alias": "Legacy MySQL",
            "type": "MYSQL",
            "url": "jdbc:mysql://localhost:3306/demo",
            "user": "root",
            "password": "sentinel-password",
            "extendInfo": [
              {"key": "useSSL", "value": "false"},
              {"key": "apiToken", "value": "sentinel-token"}
            ]
          },
          {"alias": "PostgreSQL", "type": "POSTGRESQL", "url": "jdbc:postgresql://localhost/db"}
        ]"#;
        let parsed = parse(
            CommunityDatasourceImportFormat::Chat2dbJson,
            content.to_vec(),
        );
        assert_eq!(parsed.datasources.len(), 1);
        assert_eq!(parsed.skipped, 1);
        let connection = parsed.datasources[0]
            .connection
            .as_ref()
            .expect("connection exists");
        assert!(
            connection
                .properties
                .iter()
                .any(|property| { property.key == "user" && property.value == "root" })
        );
        assert!(
            connection
                .properties
                .iter()
                .any(|property| { property.key == "useSSL" && property.value == "false" })
        );
        assert!(!connection.properties.iter().any(|property| {
            property.value.contains("sentinel") || property.key.eq_ignore_ascii_case("password")
        }));
    }

    #[test]
    fn navicat_11_and_12_passwords_are_compatible_with_the_java_algorithms() {
        let plaintext_11 = b"secret11";
        let encrypted_11 = encrypt_navicat_11(plaintext_11);
        assert_eq!(
            decrypt_navicat_11(&hex::encode_upper(encrypted_11)).expect("v11 decrypts"),
            "secret11"
        );

        let encrypted_12 = encrypt_aes_cbc(b"libcckeylibcckey", b"libcciv libcciv ", b"secret12");
        let ncx = format!(
            r#"<Connections Ver="1.2"><Connection ConnectionName="Navicat" ConnType="MYSQL" Host="localhost" Port="3306" UserName="root" Password="{}" SSH="false"/></Connections>"#,
            hex::encode_upper(encrypted_12)
        );
        let parsed = parse(
            CommunityDatasourceImportFormat::NavicatNcx,
            ncx.into_bytes(),
        );
        let connection = parsed.datasources[0]
            .connection
            .as_ref()
            .expect("connection exists");
        assert!(connection.properties.iter().any(|property| {
            property.key == "password" && property.value == "secret12" && property.sensitive
        }));
    }

    #[test]
    fn dbeaver_dbp_reads_encrypted_credentials_with_bounded_zip_entries() {
        let data_sources = br#"{
          "connections": {
            "mysql-1": {
              "provider": "mysql",
              "driver": "mysql8",
              "name": "DBeaver MySQL",
              "configuration": {
                "host": "localhost",
                "port": "3306",
                "database": "demo",
                "url": "jdbc:mysql://localhost:3306/demo",
                "properties": {"useSSL": false}
              }
            },
            "pg-1": {
              "provider": "postgresql",
              "name": "PostgreSQL",
              "configuration": {"url": "jdbc:postgresql://localhost/demo"}
            }
          }
        }"#;
        let credentials =
            br##"{"mysql-1":{"#connection":{"user":"root","password":"dbeaver-secret"}}}"##;
        let encrypted_credentials = encrypt_dbeaver_credentials(credentials);
        let archive = zip_fixture(&[
            (
                "projects/demo/.dbeaver/data-sources.json",
                data_sources.as_slice(),
            ),
            (
                "projects/demo/.dbeaver/credentials-config.json",
                &encrypted_credentials,
            ),
        ]);
        let parsed = parse(CommunityDatasourceImportFormat::DbeaverDbp, archive);
        assert_eq!(parsed.datasources.len(), 1);
        assert_eq!(parsed.skipped, 1);
        let connection = parsed.datasources[0]
            .connection
            .as_ref()
            .expect("connection exists");
        assert!(connection.properties.iter().any(|property| {
            property.key == "password" && property.value == "dbeaver-secret" && property.sensitive
        }));
    }

    #[test]
    fn datagrip_text_accepts_mysql_and_rejects_doctype() {
        let text = br#"#DataSourceSettings#
#BEGIN#
<data-source name="DataGrip MySQL">
  <database-info dbms="MYSQL"/>
  <jdbc-url>jdbc:mysql://localhost:3306/demo</jdbc-url>
  <user-name>root</user-name>
  <jdbc-driver>com.mysql.cj.jdbc.Driver</jdbc-driver>
</data-source>
#END#
"#;
        let parsed = parse(CommunityDatasourceImportFormat::DatagripText, text.to_vec());
        assert_eq!(parsed.datasources.len(), 1);
        let connection = parsed.datasources[0]
            .connection
            .as_ref()
            .expect("connection exists");
        assert!(
            connection
                .properties
                .iter()
                .any(|property| { property.key == "user" && property.value == "root" })
        );

        let malicious = br#"#DataSourceSettings#
#BEGIN#
<!DOCTYPE data-source [<!ENTITY xxe SYSTEM "file:///etc/passwd">]>
<data-source name="Unsafe"><database-info dbms="MYSQL"/><jdbc-url>jdbc:mysql://localhost/demo</jdbc-url></data-source>
#END#
"#;
        let result = parse_import(&CommunityDatasourceFileImportRequest {
            format: CommunityDatasourceImportFormat::DatagripText,
            content: malicious.to_vec(),
        });
        assert!(result.is_err());
    }

    #[allow(deprecated)]
    fn encrypt_navicat_11(input: &[u8]) -> Vec<u8> {
        use blowfish::{
            Blowfish,
            cipher::{BlockEncrypt, KeyInit, generic_array::GenericArray},
        };
        use sha1::{Digest, Sha1};

        let key = Sha1::digest(b"3DC5CA39");
        let cipher: Blowfish = Blowfish::new_from_slice(&key).expect("valid key");
        let mut iv = GenericArray::clone_from_slice(&[0xff_u8; 8]);
        cipher.encrypt_block(&mut iv);
        let mut chaining_value = <[u8; 8]>::from(iv);
        let mut output = vec![0_u8; input.len()];
        let full_blocks = input.len() / 8;
        for block_index in 0..full_blocks {
            let offset = block_index * 8;
            let mut block = [0_u8; 8];
            for index in 0..8 {
                block[index] = input[offset + index] ^ chaining_value[index];
            }
            let mut encrypted = GenericArray::clone_from_slice(&block);
            cipher.encrypt_block(&mut encrypted);
            for index in 0..8 {
                output[offset + index] = encrypted[index];
                chaining_value[index] ^= encrypted[index];
            }
        }
        let remaining = input.len() % 8;
        if remaining != 0 {
            let offset = full_blocks * 8;
            let mut encrypted = GenericArray::clone_from_slice(&chaining_value);
            cipher.encrypt_block(&mut encrypted);
            for index in 0..remaining {
                output[offset + index] = input[offset + index] ^ encrypted[index];
            }
        }
        output
    }

    fn encrypt_aes_cbc(key: &[u8], iv: &[u8], plaintext: &[u8]) -> Vec<u8> {
        let padded_len = (plaintext.len() / 16 + 1) * 16;
        let mut buffer = vec![0_u8; padded_len];
        buffer[..plaintext.len()].copy_from_slice(plaintext);
        Aes128CbcEnc::new_from_slices(key, iv)
            .expect("valid key and IV")
            .encrypt_padded_mut::<Pkcs7>(&mut buffer, plaintext.len())
            .expect("padding succeeds")
            .to_vec()
    }

    fn encrypt_dbeaver_credentials(plaintext: &[u8]) -> Vec<u8> {
        const KEY: [u8; 16] = [
            0xba, 0xbb, 0x4a, 0x9f, 0x77, 0x4a, 0xb8, 0x53, 0xc9, 0x6c, 0x2d, 0x65, 0x3d, 0xfe,
            0x54, 0x4a,
        ];
        let iv = [0x24_u8; 16];
        let mut output = iv.to_vec();
        output.extend(encrypt_aes_cbc(&KEY, &iv, plaintext));
        output
    }

    fn zip_fixture(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut writer = ZipWriter::new(cursor);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for (path, content) in entries {
            writer
                .start_file(path, options)
                .expect("fixture entry starts");
            writer.write_all(content).expect("fixture entry writes");
        }
        writer.finish().expect("fixture zip finishes").into_inner()
    }
}
