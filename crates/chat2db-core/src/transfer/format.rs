use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chat2db_contract::{ApiError, TabularImportEncoding, TransferFileFormat};
use xls::core::{Cell, Workbook};
use zip::ZipArchive;

use crate::{AppError, AppErrorKind};

pub(crate) const MAX_IMPORT_FILE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_TABULAR_CELLS: usize = 2_000_000;
const MAX_TABULAR_COLUMNS: usize = 16_384;
const MAX_CELL_BYTES: usize = 16 * 1024 * 1024;
const CELL_ENCODING_PREFIX: &str = "__CHAT2DB_TRANSFER_V1__:";
const NULL_ENCODING: &str = "__CHAT2DB_TRANSFER_V1__:NULL";
const TEXT_ENCODING_PREFIX: &str = "__CHAT2DB_TRANSFER_V1__:TEXT:";
const BYTES_ENCODING_PREFIX: &str = "__CHAT2DB_TRANSFER_V1__:BASE64:";
const MAX_XLSX_ZIP_ENTRIES: usize = 1_024;
const MAX_XLSX_ENTRY_BYTES: u64 = 64 * 1024 * 1024;
const MAX_XLSX_TOTAL_BYTES: u64 = 256 * 1024 * 1024;
const MIN_XLSX_RATIO_CHECK_BYTES: u64 = 1024 * 1024;
const MAX_XLSX_COMPRESSION_RATIO: u64 = 200;
const ZIP_EOCD_SIGNATURE: [u8; 4] = *b"PK\x05\x06";
const ZIP64_EOCD_SIGNATURE: [u8; 4] = *b"PK\x06\x06";
const ZIP64_LOCATOR_SIGNATURE: [u8; 4] = *b"PK\x06\x07";
const ZIP_EOCD_BYTES: usize = 22;
const ZIP_MAX_COMMENT_BYTES: usize = u16::MAX as usize;
const ZIP64_LOCATOR_BYTES: u64 = 20;
const ZIP64_EOCD_MIN_BYTES: usize = 56;

/// Version-one tabular values are stored as readable text except for values that
/// need an explicit envelope: NULL, reserved-prefix text, and raw bytes.
/// `CELL_ENCODING_PREFIX` is a reserved namespace on import; exports escape any
/// user text in that namespace through the TEXT envelope before writing it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TabularValue {
    Null,
    Text(String),
    Bytes(Vec<u8>),
}

#[derive(Debug)]
pub(crate) struct ImportedTable {
    pub(crate) columns: Option<Vec<String>>,
    pub(crate) rows: Vec<Vec<TabularValue>>,
}

pub(crate) trait TabularSink: Send {
    fn write_header(&mut self, columns: &[String]) -> Result<(), AppError>;
    fn write_row(&mut self, values: &[TabularValue]) -> Result<(), AppError>;
    fn finish(&mut self) -> Result<(), AppError>;
}

pub(crate) fn tabular_sink<'a>(
    format: TransferFileFormat,
    file: &'a mut File,
    contains_header: bool,
) -> Result<Box<dyn TabularSink + Send + 'a>, AppError> {
    match format {
        TransferFileFormat::Csv => Ok(Box::new(CsvSink {
            writer: csv::WriterBuilder::new().from_writer(file),
            contains_header,
            columns: None,
        })),
        TransferFileFormat::Xls | TransferFileFormat::Xlsx => Ok(Box::new(SpreadsheetSink {
            file,
            format,
            contains_header,
            workbook: Workbook::new(),
            next_row: 0,
            columns: 0,
            cells: 0,
            finished: false,
        })),
        TransferFileFormat::Sql => Err(AppError::invalid(
            "invalid_transfer_format",
            "SQL exports use the SQL dump writer",
        )),
    }
}

pub(crate) fn read_tabular_file(
    path: &Path,
    format: TransferFileFormat,
    contains_header: bool,
    tabular_encoding: TabularImportEncoding,
) -> Result<ImportedTable, AppError> {
    validate_import_file(path)?;
    match format {
        TransferFileFormat::Csv => read_csv(path, contains_header, tabular_encoding),
        TransferFileFormat::Xls | TransferFileFormat::Xlsx => {
            read_spreadsheet(path, format, contains_header, tabular_encoding)
        }
        TransferFileFormat::Sql => Err(AppError::invalid(
            "invalid_transfer_format",
            "SQL input is not a tabular file",
        )),
    }
}

pub(crate) fn validate_import_file(path: &Path) -> Result<u64, AppError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| {
        AppError::not_found(
            "import_file_not_found",
            "The selected import file does not exist",
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(AppError::invalid(
            "invalid_import_file",
            "The selected import path must be a regular file",
        ));
    }
    if metadata.len() > MAX_IMPORT_FILE_BYTES {
        return Err(resource_error(
            "import_file_too_large",
            format!("Import files are limited to {MAX_IMPORT_FILE_BYTES} bytes"),
        ));
    }
    Ok(metadata.len())
}

fn read_csv(
    path: &Path,
    contains_header: bool,
    tabular_encoding: TabularImportEncoding,
) -> Result<ImportedTable, AppError> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(contains_header)
        .flexible(false)
        .from_path(path)
        .map_err(format_error)?;
    let columns = contains_header
        .then(|| {
            reader
                .headers()
                .map(|headers| headers.iter().map(str::to_owned).collect::<Vec<_>>())
                .map_err(format_error)
        })
        .transpose()?;
    validate_columns(columns.as_deref())?;
    let mut rows = Vec::new();
    let mut cells = columns.as_ref().map_or(0, Vec::len);
    for record in reader.records() {
        let record = record.map_err(format_error)?;
        cells = cells
            .checked_add(record.len())
            .ok_or_else(|| resource_error("tabular_file_too_large", "Too many cells"))?;
        enforce_cell_budget(cells)?;
        let mut row = Vec::with_capacity(record.len());
        for value in &record {
            enforce_cell_size(value)?;
            row.push(decode_tabular_value(value, tabular_encoding)?);
        }
        rows.push(row);
    }
    validate_row_widths(columns.as_deref(), &rows)?;
    Ok(ImportedTable { columns, rows })
}

fn read_spreadsheet(
    path: &Path,
    format: TransferFileFormat,
    contains_header: bool,
    tabular_encoding: TabularImportEncoding,
) -> Result<ImportedTable, AppError> {
    if format == TransferFileFormat::Xlsx {
        validate_xlsx_archive(path)?;
    }
    let file = File::open(path).map_err(|_| {
        AppError::not_found(
            "import_file_not_found",
            "The selected import file could not be opened",
        )
    })?;
    let workbook = match format {
        TransferFileFormat::Xls => xls::core::xls::read(file),
        TransferFileFormat::Xlsx => xls::core::xlsx::read(file),
        TransferFileFormat::Csv | TransferFileFormat::Sql => unreachable!(),
    }
    .map_err(format_error)?;
    let sheet = workbook.sheets.first().ok_or_else(|| {
        AppError::invalid(
            "invalid_spreadsheet",
            "The spreadsheet does not contain a worksheet",
        )
    })?;
    let (row_count, column_count) = sheet.dimensions();
    let row_count = usize::try_from(row_count).map_err(|_| AppError::internal())?;
    let column_count = usize::try_from(column_count).map_err(|_| AppError::internal())?;
    if column_count == 0 || column_count > MAX_TABULAR_COLUMNS {
        return Err(resource_error(
            "tabular_file_too_wide",
            format!("Tabular files are limited to {MAX_TABULAR_COLUMNS} columns"),
        ));
    }
    enforce_cell_budget(row_count.saturating_mul(column_count))?;

    let start_row = usize::from(contains_header);
    let columns = contains_header.then(|| {
        (0..column_count)
            .map(|column| workbook.display_cell(0, 0, u32::try_from(column).unwrap_or(u32::MAX)))
            .collect::<Vec<_>>()
    });
    validate_columns(columns.as_deref())?;
    let mut rows = Vec::with_capacity(row_count.saturating_sub(start_row));
    for row in start_row..row_count {
        let row = u32::try_from(row).map_err(|_| AppError::internal())?;
        let mut values = Vec::with_capacity(column_count);
        for column in 0..column_count {
            let column = u32::try_from(column).map_err(|_| AppError::internal())?;
            let value = if sheet.get(row, column).is_none() {
                TabularValue::Null
            } else {
                let value = workbook.display_cell(0, row, column);
                enforce_cell_size(&value)?;
                decode_tabular_value(&value, tabular_encoding)?
            };
            values.push(value);
        }
        rows.push(values);
    }
    validate_row_widths(columns.as_deref(), &rows)?;
    Ok(ImportedTable { columns, rows })
}

struct CsvSink<W: Write> {
    writer: csv::Writer<W>,
    contains_header: bool,
    columns: Option<usize>,
}

impl<W: Write + Send> TabularSink for CsvSink<W> {
    fn write_header(&mut self, columns: &[String]) -> Result<(), AppError> {
        validate_export_columns(columns)?;
        self.columns = Some(columns.len());
        if self.contains_header {
            self.writer.write_record(columns).map_err(format_error)?;
        }
        Ok(())
    }

    fn write_row(&mut self, values: &[TabularValue]) -> Result<(), AppError> {
        validate_export_row(self.columns, values)?;
        self.writer
            .write_record(values.iter().map(encode_tabular_value))
            .map_err(format_error)
    }

    fn finish(&mut self) -> Result<(), AppError> {
        self.writer.flush().map_err(format_error)
    }
}

struct SpreadsheetSink<'a> {
    file: &'a mut File,
    format: TransferFileFormat,
    contains_header: bool,
    workbook: Workbook,
    next_row: u32,
    columns: usize,
    cells: usize,
    finished: bool,
}

impl TabularSink for SpreadsheetSink<'_> {
    fn write_header(&mut self, columns: &[String]) -> Result<(), AppError> {
        validate_export_columns(columns)?;
        self.columns = columns.len();
        if self.contains_header {
            self.write_values(
                &columns
                    .iter()
                    .cloned()
                    .map(TabularValue::Text)
                    .collect::<Vec<_>>(),
            )?;
        }
        Ok(())
    }

    fn write_row(&mut self, values: &[TabularValue]) -> Result<(), AppError> {
        validate_export_row(Some(self.columns), values)?;
        self.write_values(values)
    }

    fn finish(&mut self) -> Result<(), AppError> {
        if self.finished {
            return Ok(());
        }
        self.file.rewind().map_err(format_error)?;
        self.file.set_len(0).map_err(format_error)?;
        match self.format {
            TransferFileFormat::Xls => xls::core::xls::write(&self.workbook, &mut *self.file),
            TransferFileFormat::Xlsx => xls::core::xlsx::write(&self.workbook, &mut *self.file),
            TransferFileFormat::Csv | TransferFileFormat::Sql => unreachable!(),
        }
        .map_err(format_error)?;
        self.finished = true;
        Ok(())
    }
}

impl SpreadsheetSink<'_> {
    fn write_values(&mut self, values: &[TabularValue]) -> Result<(), AppError> {
        self.cells = self
            .cells
            .checked_add(values.len())
            .ok_or_else(|| resource_error("tabular_export_too_large", "Too many cells"))?;
        enforce_cell_budget(self.cells)?;
        let sheet = self.workbook.sheet_mut(0).ok_or_else(AppError::internal)?;
        for (column, value) in values.iter().enumerate() {
            let value = encode_tabular_value(value);
            enforce_cell_size(&value)?;
            sheet.set(
                self.next_row,
                u32::try_from(column).map_err(|_| AppError::internal())?,
                Cell::Text(value),
            );
        }
        self.next_row = self
            .next_row
            .checked_add(1)
            .ok_or_else(|| resource_error("tabular_export_too_large", "Too many rows"))?;
        Ok(())
    }
}

fn encode_tabular_value(value: &TabularValue) -> String {
    match value {
        TabularValue::Null => NULL_ENCODING.to_owned(),
        TabularValue::Bytes(value) => {
            format!("{BYTES_ENCODING_PREFIX}{}", URL_SAFE_NO_PAD.encode(value))
        }
        TabularValue::Text(value) if value.starts_with(CELL_ENCODING_PREFIX) => {
            format!("{TEXT_ENCODING_PREFIX}{}", URL_SAFE_NO_PAD.encode(value))
        }
        TabularValue::Text(value) => value.clone(),
    }
}

fn decode_tabular_value(
    value: &str,
    tabular_encoding: TabularImportEncoding,
) -> Result<TabularValue, AppError> {
    if tabular_encoding == TabularImportEncoding::Plain {
        return Ok(TabularValue::Text(value.to_owned()));
    }
    if value == NULL_ENCODING {
        return Ok(TabularValue::Null);
    }
    if let Some(value) = value.strip_prefix(BYTES_ENCODING_PREFIX) {
        return URL_SAFE_NO_PAD
            .decode(value)
            .map(TabularValue::Bytes)
            .map_err(|_| invalid_cell_encoding());
    }
    if let Some(value) = value.strip_prefix(TEXT_ENCODING_PREFIX) {
        let value = URL_SAFE_NO_PAD
            .decode(value)
            .map_err(|_| invalid_cell_encoding())?;
        return String::from_utf8(value)
            .map(TabularValue::Text)
            .map_err(|_| invalid_cell_encoding());
    }
    if value.starts_with(CELL_ENCODING_PREFIX) {
        return Err(invalid_cell_encoding());
    }
    Ok(TabularValue::Text(value.to_owned()))
}

fn invalid_cell_encoding() -> AppError {
    AppError::invalid(
        "invalid_tabular_cell_encoding",
        "The Chat2DB transfer file contains an invalid encoded cell",
    )
}

fn validate_xlsx_archive(path: &Path) -> Result<(), AppError> {
    let raw_entry_count = preflight_xlsx_entry_count(path)?;
    let file = File::open(path).map_err(format_error)?;
    let mut archive = ZipArchive::new(file).map_err(format_error)?;
    if archive.len() != raw_entry_count {
        return Err(AppError::invalid(
            "xlsx_archive_duplicate_entries",
            "XLSX archives cannot contain duplicate ZIP entry names",
        ));
    }
    if raw_entry_count > MAX_XLSX_ZIP_ENTRIES {
        return Err(resource_error(
            "xlsx_archive_too_many_entries",
            format!("XLSX archives are limited to {MAX_XLSX_ZIP_ENTRIES} ZIP entries"),
        ));
    }

    let mut declared_total = 0_u64;
    let mut actual_total = 0_u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(format_error)?;
        let declared_size = entry.size();
        let compressed_size = entry.compressed_size();
        if declared_size > MAX_XLSX_ENTRY_BYTES {
            return Err(resource_error(
                "xlsx_archive_entry_too_large",
                format!("One XLSX ZIP entry exceeds {MAX_XLSX_ENTRY_BYTES} bytes"),
            ));
        }
        declared_total = declared_total
            .checked_add(declared_size)
            .ok_or_else(|| resource_error("xlsx_archive_too_large", "XLSX archive is too large"))?;
        if declared_total > MAX_XLSX_TOTAL_BYTES {
            return Err(resource_error(
                "xlsx_archive_too_large",
                format!("XLSX archives may expand to at most {MAX_XLSX_TOTAL_BYTES} bytes"),
            ));
        }
        if declared_size >= MIN_XLSX_RATIO_CHECK_BYTES
            && (compressed_size == 0
                || declared_size > compressed_size.saturating_mul(MAX_XLSX_COMPRESSION_RATIO))
        {
            return Err(resource_error(
                "xlsx_archive_compression_ratio_too_high",
                "One XLSX ZIP entry has an unsafe compression ratio",
            ));
        }
        if entry.is_dir() {
            continue;
        }

        let remaining_total = MAX_XLSX_TOTAL_BYTES.saturating_sub(actual_total);
        let read_limit = MAX_XLSX_ENTRY_BYTES.min(remaining_total).saturating_add(1);
        let actual_size = std::io::copy(&mut (&mut entry).take(read_limit), &mut std::io::sink())
            .map_err(format_error)?;
        if actual_size > MAX_XLSX_ENTRY_BYTES {
            return Err(resource_error(
                "xlsx_archive_entry_too_large",
                format!("One XLSX ZIP entry exceeds {MAX_XLSX_ENTRY_BYTES} bytes"),
            ));
        }
        if actual_size >= MIN_XLSX_RATIO_CHECK_BYTES
            && (compressed_size == 0
                || actual_size > compressed_size.saturating_mul(MAX_XLSX_COMPRESSION_RATIO))
        {
            return Err(resource_error(
                "xlsx_archive_compression_ratio_too_high",
                "One XLSX ZIP entry has an unsafe compression ratio",
            ));
        }
        actual_total = actual_total
            .checked_add(actual_size)
            .ok_or_else(|| resource_error("xlsx_archive_too_large", "XLSX archive is too large"))?;
        if actual_total > MAX_XLSX_TOTAL_BYTES {
            return Err(resource_error(
                "xlsx_archive_too_large",
                format!("XLSX archives may expand to at most {MAX_XLSX_TOTAL_BYTES} bytes"),
            ));
        }
    }
    Ok(())
}

fn preflight_xlsx_entry_count(path: &Path) -> Result<usize, AppError> {
    let mut file = File::open(path).map_err(format_error)?;
    let file_len = file.metadata().map_err(format_error)?.len();
    let tail_budget = ZIP_EOCD_BYTES
        .checked_add(ZIP_MAX_COMMENT_BYTES)
        .and_then(|value| value.checked_add(usize::try_from(ZIP64_LOCATOR_BYTES).ok()?))
        .ok_or_else(invalid_xlsx_archive)?;
    let tail_len = usize::try_from(file_len.min(u64::try_from(tail_budget).unwrap_or(u64::MAX)))
        .map_err(|_| invalid_xlsx_archive())?;
    if tail_len < ZIP_EOCD_BYTES {
        return Err(invalid_xlsx_archive());
    }
    let tail_start = file_len
        .checked_sub(u64::try_from(tail_len).map_err(|_| invalid_xlsx_archive())?)
        .ok_or_else(invalid_xlsx_archive)?;
    file.seek(SeekFrom::Start(tail_start))
        .map_err(format_error)?;
    let mut tail = vec![0_u8; tail_len];
    file.read_exact(&mut tail).map_err(format_error)?;

    let mut eocd_relative = None;
    for offset in (0..=tail_len - ZIP_EOCD_BYTES).rev() {
        if tail.get(offset..offset + 4) != Some(ZIP_EOCD_SIGNATURE.as_slice()) {
            continue;
        }
        let Some(comment_len) = read_u16_at(&tail, offset + 20) else {
            continue;
        };
        let Some(candidate_end) = offset
            .checked_add(ZIP_EOCD_BYTES)
            .and_then(|value| value.checked_add(usize::from(comment_len)))
        else {
            continue;
        };
        if candidate_end == tail_len {
            eocd_relative = Some(offset);
            break;
        }
    }
    let eocd_relative = eocd_relative.ok_or_else(invalid_xlsx_archive)?;
    let eocd_position = tail_start
        .checked_add(u64::try_from(eocd_relative).map_err(|_| invalid_xlsx_archive())?)
        .ok_or_else(invalid_xlsx_archive)?;
    let disk_number = read_u16_at(&tail, eocd_relative + 4).ok_or_else(invalid_xlsx_archive)?;
    let central_disk = read_u16_at(&tail, eocd_relative + 6).ok_or_else(invalid_xlsx_archive)?;
    let disk_entries = read_u16_at(&tail, eocd_relative + 8).ok_or_else(invalid_xlsx_archive)?;
    let total_entries = read_u16_at(&tail, eocd_relative + 10).ok_or_else(invalid_xlsx_archive)?;
    let central_size = read_u32_at(&tail, eocd_relative + 12).ok_or_else(invalid_xlsx_archive)?;
    let central_offset = read_u32_at(&tail, eocd_relative + 16).ok_or_else(invalid_xlsx_archive)?;
    if disk_number != 0 || central_disk != 0 {
        return Err(invalid_xlsx_archive());
    }

    let uses_zip64 = disk_entries == u16::MAX
        || total_entries == u16::MAX
        || central_size == u32::MAX
        || central_offset == u32::MAX;
    let (entry_count, central_size, central_offset, central_limit) = if uses_zip64 {
        read_zip64_directory_metadata(&mut file, eocd_position, file_len)?
    } else {
        if disk_entries != total_entries {
            return Err(invalid_xlsx_archive());
        }
        (
            u64::from(total_entries),
            u64::from(central_size),
            u64::from(central_offset),
            eocd_position,
        )
    };
    if entry_count > u64::try_from(MAX_XLSX_ZIP_ENTRIES).unwrap_or(u64::MAX) {
        return Err(resource_error(
            "xlsx_archive_too_many_entries",
            format!("XLSX archives are limited to {MAX_XLSX_ZIP_ENTRIES} ZIP entries"),
        ));
    }
    validate_central_directory_bounds(central_offset, central_size, central_limit, file_len)?;
    usize::try_from(entry_count).map_err(|_| invalid_xlsx_archive())
}

fn read_zip64_directory_metadata(
    file: &mut File,
    eocd_position: u64,
    file_len: u64,
) -> Result<(u64, u64, u64, u64), AppError> {
    let locator_position = eocd_position
        .checked_sub(ZIP64_LOCATOR_BYTES)
        .ok_or_else(invalid_xlsx_archive)?;
    let locator = read_exact_at::<20>(file, locator_position)?;
    if locator[..4] != ZIP64_LOCATOR_SIGNATURE
        || read_u32_at(&locator, 4) != Some(0)
        || read_u32_at(&locator, 16) != Some(1)
    {
        return Err(invalid_xlsx_archive());
    }
    let zip64_position = read_u64_at(&locator, 8).ok_or_else(invalid_xlsx_archive)?;
    let fixed = read_exact_at::<ZIP64_EOCD_MIN_BYTES>(file, zip64_position)?;
    if fixed[..4] != ZIP64_EOCD_SIGNATURE {
        return Err(invalid_xlsx_archive());
    }
    let record_payload_size = read_u64_at(&fixed, 4).ok_or_else(invalid_xlsx_archive)?;
    if record_payload_size < 44 {
        return Err(invalid_xlsx_archive());
    }
    let record_end = zip64_position
        .checked_add(12)
        .and_then(|value| value.checked_add(record_payload_size))
        .ok_or_else(invalid_xlsx_archive)?;
    if record_end > locator_position || record_end > file_len {
        return Err(invalid_xlsx_archive());
    }
    let disk_number = read_u32_at(&fixed, 16).ok_or_else(invalid_xlsx_archive)?;
    let central_disk = read_u32_at(&fixed, 20).ok_or_else(invalid_xlsx_archive)?;
    let disk_entries = read_u64_at(&fixed, 24).ok_or_else(invalid_xlsx_archive)?;
    let total_entries = read_u64_at(&fixed, 32).ok_or_else(invalid_xlsx_archive)?;
    if disk_number != 0 || central_disk != 0 || disk_entries != total_entries {
        return Err(invalid_xlsx_archive());
    }
    let central_size = read_u64_at(&fixed, 40).ok_or_else(invalid_xlsx_archive)?;
    let central_offset = read_u64_at(&fixed, 48).ok_or_else(invalid_xlsx_archive)?;
    Ok((total_entries, central_size, central_offset, zip64_position))
}

fn validate_central_directory_bounds(
    offset: u64,
    size: u64,
    metadata_position: u64,
    file_len: u64,
) -> Result<(), AppError> {
    let end = offset.checked_add(size).ok_or_else(invalid_xlsx_archive)?;
    if offset > file_len || size > file_len || end > metadata_position || end > file_len {
        return Err(invalid_xlsx_archive());
    }
    Ok(())
}

fn read_exact_at<const N: usize>(file: &mut File, offset: u64) -> Result<[u8; N], AppError> {
    file.seek(SeekFrom::Start(offset)).map_err(format_error)?;
    let mut bytes = [0_u8; N];
    file.read_exact(&mut bytes).map_err(format_error)?;
    Ok(bytes)
}

fn read_u16_at(bytes: &[u8], offset: usize) -> Option<u16> {
    let value = bytes.get(offset..offset.checked_add(2)?)?.try_into().ok()?;
    Some(u16::from_le_bytes(value))
}

fn read_u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    let value = bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?;
    Some(u32::from_le_bytes(value))
}

fn read_u64_at(bytes: &[u8], offset: usize) -> Option<u64> {
    let value = bytes.get(offset..offset.checked_add(8)?)?.try_into().ok()?;
    Some(u64::from_le_bytes(value))
}

fn invalid_xlsx_archive() -> AppError {
    AppError::invalid(
        "invalid_xlsx_archive",
        "The XLSX ZIP directory is invalid or unsupported",
    )
}

fn validate_columns(columns: Option<&[String]>) -> Result<(), AppError> {
    let Some(columns) = columns else {
        return Ok(());
    };
    validate_export_columns(columns)?;
    if columns.iter().any(|column| column.trim().is_empty()) {
        return Err(AppError::invalid(
            "invalid_tabular_header",
            "Column headers cannot be empty",
        ));
    }
    let mut normalized = std::collections::HashSet::with_capacity(columns.len());
    if columns
        .iter()
        .any(|column| !normalized.insert(column.to_ascii_lowercase()))
    {
        return Err(AppError::invalid(
            "invalid_tabular_header",
            "Column headers must be unique",
        ));
    }
    Ok(())
}

fn validate_export_columns(columns: &[String]) -> Result<(), AppError> {
    if columns.is_empty() || columns.len() > MAX_TABULAR_COLUMNS {
        return Err(resource_error(
            "tabular_file_too_wide",
            format!("Tabular files require 1 to {MAX_TABULAR_COLUMNS} columns"),
        ));
    }
    for column in columns {
        enforce_cell_size(column)?;
    }
    Ok(())
}

fn validate_export_row(expected: Option<usize>, values: &[TabularValue]) -> Result<(), AppError> {
    if expected != Some(values.len()) {
        return Err(AppError::invalid(
            "invalid_tabular_row",
            "Every tabular row must match the column count",
        ));
    }
    for value in values {
        let encoded = encode_tabular_value(value);
        enforce_cell_size(&encoded)?;
    }
    Ok(())
}

fn validate_row_widths(
    columns: Option<&[String]>,
    rows: &[Vec<TabularValue>],
) -> Result<(), AppError> {
    let expected = columns
        .map(<[String]>::len)
        .or_else(|| rows.first().map(Vec::len));
    if expected.is_none() {
        return Err(AppError::invalid(
            "empty_tabular_file",
            "The tabular import file does not contain rows",
        ));
    }
    if rows.iter().any(|row| Some(row.len()) != expected) {
        return Err(AppError::invalid(
            "invalid_tabular_row",
            "Every tabular row must match the column count",
        ));
    }
    Ok(())
}

fn enforce_cell_budget(cells: usize) -> Result<(), AppError> {
    if cells > MAX_TABULAR_CELLS {
        return Err(resource_error(
            "tabular_file_too_large",
            format!("Tabular files are limited to {MAX_TABULAR_CELLS} cells"),
        ));
    }
    Ok(())
}

fn enforce_cell_size(value: &str) -> Result<(), AppError> {
    if value.len() > MAX_CELL_BYTES {
        return Err(resource_error(
            "tabular_cell_too_large",
            format!("One tabular cell exceeds {MAX_CELL_BYTES} bytes"),
        ));
    }
    Ok(())
}

fn format_error(error: impl std::fmt::Display) -> AppError {
    AppError::invalid(
        "invalid_transfer_file",
        format!("The transfer file could not be processed: {error}"),
    )
}

fn resource_error(code: impl Into<String>, message: impl Into<String>) -> AppError {
    AppError::new(
        AppErrorKind::ResourceExhausted,
        ApiError::new(code, message),
    )
}

#[cfg(test)]
mod tests {
    use std::{fs, fs::File, io::Write as _};

    use chat2db_contract::{TabularImportEncoding, TransferFileFormat};
    use tempfile::NamedTempFile;
    use xls::core::{Cell, Workbook};
    use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

    use super::{
        BYTES_ENCODING_PREFIX, CELL_ENCODING_PREFIX, MAX_XLSX_COMPRESSION_RATIO,
        MAX_XLSX_ENTRY_BYTES, MAX_XLSX_TOTAL_BYTES, MAX_XLSX_ZIP_ENTRIES,
        MIN_XLSX_RATIO_CHECK_BYTES, NULL_ENCODING, TEXT_ENCODING_PREFIX, TabularValue,
        preflight_xlsx_entry_count, read_tabular_file, tabular_sink, validate_xlsx_archive,
    };

    #[test]
    fn tabular_round_trip_preserves_null_empty_utf8_binary_and_reserved_text() {
        for format in [
            TransferFileFormat::Csv,
            TransferFileFormat::Xls,
            TransferFileFormat::Xlsx,
        ] {
            let temporary = NamedTempFile::new().expect("temp file");
            let mut file = File::options()
                .read(true)
                .write(true)
                .open(temporary.path())
                .expect("temp file opens");
            {
                let mut sink = tabular_sink(format, &mut file, true).expect("sink creates");
                sink.write_header(&[
                    "plain_value".to_owned(),
                    "null_value".to_owned(),
                    "empty_value".to_owned(),
                    "utf8_value".to_owned(),
                    "binary_value".to_owned(),
                    "reserved_value".to_owned(),
                ])
                .expect("header writes");
                sink.write_row(&[
                    TabularValue::Text("1".to_owned()),
                    TabularValue::Null,
                    TabularValue::Text(String::new()),
                    TabularValue::Text("中文,quoted".to_owned()),
                    TabularValue::Bytes(vec![0x00, 0xff]),
                    TabularValue::Text(format!("{CELL_ENCODING_PREFIX}NULL")),
                ])
                .expect("row writes");
                sink.finish().expect("sink finishes");
            }
            if format == TransferFileFormat::Csv {
                let mut reader = csv::Reader::from_path(temporary.path()).expect("CSV reads");
                let record = reader
                    .records()
                    .next()
                    .expect("CSV row exists")
                    .expect("CSV row decodes");
                assert_eq!(&record[0], "1", "ordinary numeric text remains readable");
                assert_eq!(&record[1], NULL_ENCODING);
                assert_eq!(&record[2], "", "empty text remains an empty CSV field");
                assert_eq!(&record[3], "中文,quoted");
                assert!(record[4].starts_with(BYTES_ENCODING_PREFIX));
                assert!(record[5].starts_with(TEXT_ENCODING_PREFIX));
            }
            let imported = read_tabular_file(
                temporary.path(),
                format,
                true,
                TabularImportEncoding::Chat2dbV1,
            )
            .expect("tabular file reads");
            assert_eq!(
                imported.rows,
                vec![vec![
                    TabularValue::Text("1".to_owned()),
                    TabularValue::Null,
                    TabularValue::Text(String::new()),
                    TabularValue::Text("中文,quoted".to_owned()),
                    TabularValue::Bytes(vec![0x00, 0xff]),
                    TabularValue::Text(format!("{CELL_ENCODING_PREFIX}NULL")),
                ]],
                "{format:?} must preserve typed values"
            );
        }
    }

    #[test]
    fn plain_tabular_import_preserves_the_v1_namespace_as_external_text() {
        let values = [
            NULL_ENCODING.to_owned(),
            format!("{BYTES_ENCODING_PREFIX}YWJj"),
            format!("{CELL_ENCODING_PREFIX}UNKNOWN"),
        ];
        for format in [
            TransferFileFormat::Csv,
            TransferFileFormat::Xls,
            TransferFileFormat::Xlsx,
        ] {
            let temporary = NamedTempFile::new().expect("temp file");
            match format {
                TransferFileFormat::Csv => {
                    let mut writer = csv::Writer::from_path(temporary.path()).expect("CSV opens");
                    writer
                        .write_record(["null_marker", "bytes_marker", "unknown_marker"])
                        .expect("CSV header writes");
                    writer.write_record(&values).expect("CSV row writes");
                    writer.flush().expect("CSV flushes");
                }
                TransferFileFormat::Xls | TransferFileFormat::Xlsx => {
                    let mut workbook = Workbook::new();
                    let sheet = workbook.sheet_mut(0).expect("default sheet exists");
                    for (column, value) in ["null_marker", "bytes_marker", "unknown_marker"]
                        .into_iter()
                        .enumerate()
                    {
                        sheet.set(
                            0,
                            u32::try_from(column).unwrap(),
                            Cell::Text(value.to_owned()),
                        );
                    }
                    for (column, value) in values.iter().enumerate() {
                        sheet.set(1, u32::try_from(column).unwrap(), Cell::Text(value.clone()));
                    }
                    let mut file = temporary.reopen().expect("spreadsheet opens");
                    match format {
                        TransferFileFormat::Xls => xls::core::xls::write(&workbook, &mut file),
                        TransferFileFormat::Xlsx => xls::core::xlsx::write(&workbook, &mut file),
                        TransferFileFormat::Csv | TransferFileFormat::Sql => unreachable!(),
                    }
                    .expect("spreadsheet writes");
                }
                TransferFileFormat::Sql => unreachable!(),
            }
            let imported =
                read_tabular_file(temporary.path(), format, true, TabularImportEncoding::Plain)
                    .expect("plain tabular file reads");
            assert_eq!(
                imported.rows,
                vec![
                    values
                        .iter()
                        .cloned()
                        .map(TabularValue::Text)
                        .collect::<Vec<_>>()
                ],
                "{format:?} must not interpret an unmarked external cell"
            );
        }
    }

    #[test]
    fn xlsx_rejects_a_declared_entry_larger_than_the_budget() {
        let temporary = zip_fixture(1, CompressionMethod::Stored, &[]);
        patch_central_uncompressed_sizes(temporary.path(), MAX_XLSX_ENTRY_BYTES + 1);
        let error = validate_xlsx_archive(temporary.path()).expect_err("oversized entry rejects");
        assert_eq!(error.api_error().code, "xlsx_archive_entry_too_large");
    }

    #[test]
    fn xlsx_rejects_a_declared_cumulative_size_larger_than_the_budget() {
        let declared_size = MIN_XLSX_RATIO_CHECK_BYTES - 1;
        let entries = usize::try_from(MAX_XLSX_TOTAL_BYTES / declared_size + 1)
            .expect("entry count fits usize");
        assert!(entries < MAX_XLSX_ZIP_ENTRIES);
        let temporary = zip_fixture(entries, CompressionMethod::Stored, &[]);
        patch_central_uncompressed_sizes(temporary.path(), declared_size);
        let error = validate_xlsx_archive(temporary.path()).expect_err("oversized archive rejects");
        assert_eq!(error.api_error().code, "xlsx_archive_too_large");
    }

    #[test]
    fn xlsx_rejects_too_many_zip_entries() {
        let temporary = zip_fixture(MAX_XLSX_ZIP_ENTRIES + 1, CompressionMethod::Stored, &[]);
        let error = validate_xlsx_archive(temporary.path()).expect_err("entry flood rejects");
        assert_eq!(error.api_error().code, "xlsx_archive_too_many_entries");
    }

    #[test]
    fn xlsx_rejects_a_high_compression_ratio_before_workbook_parsing() {
        let repeated = vec![0_u8; usize::try_from(MIN_XLSX_RATIO_CHECK_BYTES).unwrap()];
        let temporary = zip_fixture(1, CompressionMethod::Deflated, &repeated);
        let compressed = fs::metadata(temporary.path()).expect("ZIP metadata").len();
        assert!(
            MIN_XLSX_RATIO_CHECK_BYTES > compressed * MAX_XLSX_COMPRESSION_RATIO,
            "fixture must exceed the configured compression ratio"
        );
        let error = validate_xlsx_archive(temporary.path()).expect_err("ZIP bomb rejects");
        assert_eq!(
            error.api_error().code,
            "xlsx_archive_compression_ratio_too_high"
        );
    }

    #[test]
    fn xlsx_rechecks_compression_ratio_with_the_actual_uncompressed_size() {
        let repeated = vec![0_u8; usize::try_from(MIN_XLSX_RATIO_CHECK_BYTES).unwrap()];
        let temporary = zip_fixture(1, CompressionMethod::Deflated, &repeated);
        patch_central_uncompressed_sizes(temporary.path(), MIN_XLSX_RATIO_CHECK_BYTES - 1);
        let error = validate_xlsx_archive(temporary.path())
            .expect_err("actual high compression ratio rejects");
        assert_eq!(
            error.api_error().code,
            "xlsx_archive_compression_ratio_too_high"
        );
    }

    #[test]
    fn xlsx_rejects_duplicate_central_directory_names() {
        let temporary = zip_fixture(2, CompressionMethod::Stored, &[]);
        patch_second_central_name_to_match_first(temporary.path());
        let error = validate_xlsx_archive(temporary.path()).expect_err("duplicates reject");
        assert_eq!(error.api_error().code, "xlsx_archive_duplicate_entries");
    }

    #[test]
    fn xlsx_preflights_zip64_entry_counts_before_archive_construction() {
        let accepted = zip64_fixture(1);
        assert_eq!(
            preflight_xlsx_entry_count(accepted.path()).expect("ZIP64 count reads"),
            1
        );

        let rejected = zip64_fixture(u64::try_from(MAX_XLSX_ZIP_ENTRIES).unwrap() + 1);
        let error = preflight_xlsx_entry_count(rejected.path()).expect_err("ZIP64 flood rejects");
        assert_eq!(error.api_error().code, "xlsx_archive_too_many_entries");
    }

    fn zip_fixture(
        entries: usize,
        compression: CompressionMethod,
        contents: &[u8],
    ) -> NamedTempFile {
        let temporary = NamedTempFile::new().expect("temp file");
        let file = temporary.reopen().expect("ZIP fixture opens");
        let mut writer = ZipWriter::new(file);
        let options = SimpleFileOptions::default().compression_method(compression);
        for index in 0..entries {
            writer
                .start_file(format!("xl/worksheets/sheet{index}.xml"), options)
                .expect("ZIP entry starts");
            writer.write_all(contents).expect("ZIP entry writes");
        }
        writer.finish().expect("ZIP fixture finishes");
        temporary
    }

    fn patch_central_uncompressed_sizes(path: &std::path::Path, size: u64) {
        let size = u32::try_from(size).expect("test declaration fits classic ZIP");
        let mut bytes = fs::read(path).expect("ZIP fixture reads");
        let mut cursor = 0;
        let mut patched = 0;
        while let Some(relative) = bytes[cursor..]
            .windows(4)
            .position(|window| window == b"PK\x01\x02")
        {
            let header = cursor + relative;
            bytes[header + 24..header + 28].copy_from_slice(&size.to_le_bytes());
            patched += 1;
            cursor = header + 46;
        }
        assert_ne!(patched, 0, "at least one central-directory entry patches");
        fs::write(path, bytes).expect("patched ZIP fixture writes");
    }

    fn patch_second_central_name_to_match_first(path: &std::path::Path) {
        let mut bytes = fs::read(path).expect("ZIP fixture reads");
        let headers = bytes
            .windows(4)
            .enumerate()
            .filter_map(|(offset, signature)| (signature == b"PK\x01\x02").then_some(offset))
            .collect::<Vec<_>>();
        assert_eq!(headers.len(), 2, "fixture has two central entries");
        let first_length = usize::from(u16::from_le_bytes(
            bytes[headers[0] + 28..headers[0] + 30]
                .try_into()
                .expect("first name length reads"),
        ));
        let second_length = usize::from(u16::from_le_bytes(
            bytes[headers[1] + 28..headers[1] + 30]
                .try_into()
                .expect("second name length reads"),
        ));
        assert_eq!(first_length, second_length);
        let first_name = bytes[headers[0] + 46..headers[0] + 46 + first_length].to_vec();
        bytes[headers[1] + 46..headers[1] + 46 + second_length].copy_from_slice(&first_name);
        fs::write(path, bytes).expect("duplicate-name ZIP fixture writes");
    }

    fn zip64_fixture(entry_count: u64) -> NamedTempFile {
        let temporary = zip_fixture(1, CompressionMethod::Stored, &[]);
        let bytes = fs::read(temporary.path()).expect("ZIP fixture reads");
        let eocd = bytes
            .windows(4)
            .rposition(|signature| signature == b"PK\x05\x06")
            .expect("EOCD exists");
        let central_size = u32::from_le_bytes(
            bytes[eocd + 12..eocd + 16]
                .try_into()
                .expect("central size reads"),
        );
        let central_offset = u32::from_le_bytes(
            bytes[eocd + 16..eocd + 20]
                .try_into()
                .expect("central offset reads"),
        );
        let mut classic_eocd = bytes[eocd..].to_vec();
        classic_eocd[8..12].fill(0xff);
        classic_eocd[12..20].fill(0xff);

        let zip64_position = u64::try_from(eocd).expect("fixture offset fits u64");
        let mut output = bytes[..eocd].to_vec();
        output.extend_from_slice(b"PK\x06\x06");
        output.extend_from_slice(&44_u64.to_le_bytes());
        output.extend_from_slice(&45_u16.to_le_bytes());
        output.extend_from_slice(&45_u16.to_le_bytes());
        output.extend_from_slice(&0_u32.to_le_bytes());
        output.extend_from_slice(&0_u32.to_le_bytes());
        output.extend_from_slice(&entry_count.to_le_bytes());
        output.extend_from_slice(&entry_count.to_le_bytes());
        output.extend_from_slice(&u64::from(central_size).to_le_bytes());
        output.extend_from_slice(&u64::from(central_offset).to_le_bytes());
        output.extend_from_slice(b"PK\x06\x07");
        output.extend_from_slice(&0_u32.to_le_bytes());
        output.extend_from_slice(&zip64_position.to_le_bytes());
        output.extend_from_slice(&1_u32.to_le_bytes());
        output.extend_from_slice(&classic_eocd);
        fs::write(temporary.path(), output).expect("ZIP64 fixture writes");
        temporary
    }
}
