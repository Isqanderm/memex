use std::path::Path;

use anyhow::Context;
use calamine::{open_workbook_auto, DataType, Reader};

use super::{DocumentAdapter, ParsedDocument, Section};

/// Adapter for Excel workbooks (XLSX, XLS, XLSB, ODS).  Each sheet becomes
/// one section with the sheet name as its heading.
pub struct XlsxAdapter;

impl DocumentAdapter for XlsxAdapter {
    fn can_handle(&self, path: &Path, mime_type: &str) -> bool {
        let spreadsheet_mimes = [
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            "application/vnd.ms-excel",
            "application/vnd.oasis.opendocument.spreadsheet",
            "application/x-xlsb",
        ];
        if spreadsheet_mimes.contains(&mime_type) {
            return true;
        }
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| {
                matches!(
                    e.to_ascii_lowercase().as_str(),
                    "xlsx" | "xls" | "xlsb" | "ods"
                )
            })
            .unwrap_or(false)
    }

    fn parse(&self, path: &Path) -> anyhow::Result<ParsedDocument> {
        let source = path.to_string_lossy().into_owned();

        let mut workbook = open_workbook_auto(path)
            .with_context(|| format!("Cannot open workbook: {}", source))?;

        let sheet_names = workbook.sheet_names().to_owned();
        let mut sections = Vec::new();

        for name in &sheet_names {
            let range = workbook
                .worksheet_range(name)
                .with_context(|| format!("Cannot read sheet '{}' in {}", name, source))?;

            let mut rows_text = Vec::new();
            for row in range.rows() {
                let cells: Vec<String> = row
                    .iter()
                    .map(|cell| cell.as_string().unwrap_or_default())
                    .collect();
                let row_str = cells.join("\t");
                // Empty and null cells are dropped — they add no textual content
                if !row_str.trim().is_empty() {
                    rows_text.push(row_str);
                }
            }

            let content = rows_text.join("\n");
            sections.push(Section {
                content,
                heading: Some(name.clone()),
                level: 1,
                page_number: None,
            });
        }

        Ok(ParsedDocument {
            source,
            mime_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
                .to_string(),
            sections,
            title: None,
            metadata: serde_json::json!({ "sheets": sheet_names }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::DocumentAdapter;
    use super::XlsxAdapter;
    use std::path::Path;

    #[test]
    fn can_handle_xlsx_extension() {
        let adapter = XlsxAdapter;
        assert!(adapter.can_handle(
            Path::new("data.xlsx"),
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        ));
        assert!(adapter.can_handle(Path::new("data.xls"), ""));
        assert!(!adapter.can_handle(Path::new("file.pdf"), "application/pdf"));
    }
}
