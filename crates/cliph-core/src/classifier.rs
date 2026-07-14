//! Détection sémantique des contenus du presse-papiers.

/// Classification principale attribuée à un élément.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardClassification {
    PlainText,
    RichText,
    Code,
    Link,
    Image,
    Files,
    Table,
    Html,
    Rtf,
    Color,
    Unknown,
}

impl ClipboardClassification {
    /// Valeur stable utilisée dans SQLite.
    pub const fn as_database_value(self) -> &'static str {
        match self {
            Self::PlainText => "plain_text",
            Self::RichText => "rich_text",
            Self::Code => "code",
            Self::Link => "link",
            Self::Image => "image",
            Self::Files => "files",
            Self::Table => "table",
            Self::Html => "html",
            Self::Rtf => "rtf",
            Self::Color => "color",
            Self::Unknown => "unknown",
        }
    }

    /// Reconstruit une classification depuis SQLite.
    pub fn from_database_value(value: &str) -> Option<Self> {
        match value {
            "plain_text" => Some(Self::PlainText),
            "rich_text" => Some(Self::RichText),
            "code" => Some(Self::Code),
            "link" => Some(Self::Link),
            "image" => Some(Self::Image),
            "files" => Some(Self::Files),
            "table" => Some(Self::Table),
            "html" => Some(Self::Html),
            "rtf" => Some(Self::Rtf),
            "color" => Some(Self::Color),
            "unknown" => Some(Self::Unknown),
            _ => None,
        }
    }

    /// Libellé destiné à l'interface de ClipH.
    pub const fn display_label(self) -> &'static str {
        match self {
            Self::PlainText => "TEXTE",
            Self::RichText => "TEXTE ENRICHI",
            Self::Code => "CODE",
            Self::Link => "LIEN",
            Self::Image => "IMAGE",
            Self::Files => "FICHIERS",
            Self::Table => "TABLEAU",
            Self::Html => "HTML",
            Self::Rtf => "RTF",
            Self::Color => "COULEUR",
            Self::Unknown => "CONTENU",
        }
    }
}

/// Résultat produit par le moteur de classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassificationResult {
    pub classification: ClipboardClassification,

    /// Précision complémentaire :
    /// langage informatique, protocole, format de tableau, etc.
    pub subtype: Option<String>,

    /// Niveau de confiance compris entre 0 et 100.
    pub confidence: u8,
}

impl ClassificationResult {
    fn new(classification: ClipboardClassification, subtype: Option<&str>, confidence: u8) -> Self {
        Self {
            classification,
            subtype: subtype.map(str::to_owned),
            confidence: confidence.min(100),
        }
    }
}

/// Analyse un contenu textuel et ses formats MIME disponibles.
pub fn classify_text(
    plain_text: &str,
    html_text: Option<&str>,
    mime_types: &[String],
) -> ClassificationResult {
    let text = plain_text.trim();

    if text.is_empty() {
        return ClassificationResult::new(ClipboardClassification::Unknown, None, 20);
    }

    if contains_table_format(mime_types)
        || html_text.is_some_and(contains_html_table)
        || looks_like_tabular_text(text)
    {
        let subtype = detect_table_subtype(text, mime_types);

        return ClassificationResult::new(ClipboardClassification::Table, Some(subtype), 96);
    }

    if let Some(link_subtype) = detect_link_subtype(text) {
        return ClassificationResult::new(ClipboardClassification::Link, Some(link_subtype), 99);
    }

    if let Some(color_subtype) = detect_color_subtype(text) {
        return ClassificationResult::new(ClipboardClassification::Color, Some(color_subtype), 98);
    }

    if contains_rtf_format(mime_types) || looks_like_rtf(text) {
        return ClassificationResult::new(ClipboardClassification::Rtf, Some("RTF"), 98);
    }

    if let Some(language) = detect_programming_language(text) {
        return ClassificationResult::new(ClipboardClassification::Code, Some(language), 92);
    }

    if looks_like_html_source(text) {
        return ClassificationResult::new(ClipboardClassification::Html, Some("HTML"), 94);
    }

    if html_text.is_some_and(|html| !html.trim().is_empty()) || contains_html_format(mime_types) {
        return ClassificationResult::new(ClipboardClassification::RichText, Some("HTML"), 95);
    }

    ClassificationResult::new(ClipboardClassification::PlainText, None, 100)
}

fn contains_html_format(mime_types: &[String]) -> bool {
    mime_types
        .iter()
        .any(|mime_type| mime_type.to_ascii_lowercase().starts_with("text/html"))
}

fn contains_rtf_format(mime_types: &[String]) -> bool {
    mime_types.iter().any(|mime_type| {
        matches!(
            mime_type.to_ascii_lowercase().as_str(),
            "text/rtf" | "application/rtf" | "application/x-rtf"
        )
    })
}

fn contains_table_format(mime_types: &[String]) -> bool {
    mime_types.iter().any(|mime_type| {
        matches!(
            mime_type.to_ascii_lowercase().as_str(),
            "text/csv"
                | "text/tab-separated-values"
                | "application/csv"
                | "application/vnd.ms-excel"
        )
    })
}

fn contains_html_table(html: &str) -> bool {
    let html = html.to_ascii_lowercase();

    html.contains("<table")
        && html.contains("<tr")
        && (html.contains("<td") || html.contains("<th"))
}

fn looks_like_tabular_text(text: &str) -> bool {
    looks_like_delimited_table(text, '\t')
        || looks_like_delimited_table(text, ',')
        || looks_like_delimited_table(text, ';')
}

fn looks_like_delimited_table(text: &str, delimiter: char) -> bool {
    let rows = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| line.split(delimiter).map(str::trim).collect::<Vec<_>>())
        .collect::<Vec<_>>();

    if rows.len() < 2 {
        return false;
    }

    let expected_column_count = rows[0].len();

    expected_column_count >= 2
        && rows.iter().all(|row| {
            row.len() == expected_column_count && row.iter().any(|cell| !cell.is_empty())
        })
}

fn detect_table_subtype<'a>(text: &str, mime_types: &'a [String]) -> &'a str {
    if mime_types
        .iter()
        .any(|mime_type| mime_type.eq_ignore_ascii_case("text/tab-separated-values"))
        || text.contains('\t')
    {
        return "TSV";
    }

    if mime_types.iter().any(|mime_type| {
        mime_type.eq_ignore_ascii_case("text/csv")
            || mime_type.eq_ignore_ascii_case("application/csv")
    }) {
        return "CSV";
    }

    if text.lines().any(|line| line.contains(';')) {
        return "CSV";
    }

    if text.lines().any(|line| line.contains(',')) {
        return "CSV";
    }

    "HTML"
}

fn detect_link_subtype(text: &str) -> Option<&'static str> {
    if text.chars().any(char::is_whitespace) {
        return None;
    }

    let lowercase = text.to_ascii_lowercase();

    if lowercase.starts_with("https://") {
        return Some("HTTPS");
    }

    if lowercase.starts_with("http://") {
        return Some("HTTP");
    }

    if lowercase.starts_with("ftp://") {
        return Some("FTP");
    }

    if lowercase.starts_with("mailto:") {
        return Some("E-mail");
    }

    if lowercase.starts_with("www.") && lowercase.contains('.') {
        return Some("Web");
    }

    if looks_like_email(text) {
        return Some("E-mail");
    }

    None
}

fn looks_like_email(text: &str) -> bool {
    let mut parts = text.split('@');

    let Some(local_part) = parts.next() else {
        return false;
    };

    let Some(domain) = parts.next() else {
        return false;
    };

    parts.next().is_none()
        && !local_part.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
}

fn detect_color_subtype(text: &str) -> Option<&'static str> {
    if looks_like_hex_color(text) {
        return Some("HEX");
    }

    let lowercase = text.to_ascii_lowercase();

    if is_css_function(&lowercase, "rgb") || is_css_function(&lowercase, "rgba") {
        return Some("RGB");
    }

    if is_css_function(&lowercase, "hsl") || is_css_function(&lowercase, "hsla") {
        return Some("HSL");
    }

    None
}

fn looks_like_hex_color(text: &str) -> bool {
    let Some(hexadecimal) = text.strip_prefix('#') else {
        return false;
    };

    matches!(hexadecimal.len(), 3 | 4 | 6 | 8)
        && hexadecimal
            .chars()
            .all(|character| character.is_ascii_hexdigit())
}

fn is_css_function(text: &str, function_name: &str) -> bool {
    text.starts_with(&format!("{function_name}(")) && text.ends_with(')')
}

fn looks_like_rtf(text: &str) -> bool {
    text.starts_with("{\\rtf") || text.starts_with("{\\urtf")
}

fn looks_like_html_source(text: &str) -> bool {
    let lowercase = text.to_ascii_lowercase();

    lowercase.starts_with("<!doctype html")
        || lowercase.starts_with("<html")
        || lowercase.starts_with("<body")
        || lowercase.starts_with("<div")
        || lowercase.starts_with("<section")
        || lowercase.starts_with("<article")
        || lowercase.starts_with("<table")
        || lowercase.starts_with("<script")
        || lowercase.starts_with("<style")
        || (lowercase.starts_with('<') && lowercase.contains("</") && lowercase.ends_with('>'))
}

fn detect_programming_language(text: &str) -> Option<&'static str> {
    let lowercase = text.to_ascii_lowercase();

    if lowercase.starts_with("<!doctype html")
        || lowercase.starts_with("<html")
        || lowercase.contains("</div>")
        || lowercase.contains("</section>")
    {
        return Some("HTML");
    }

    if text.starts_with("#!/bin/bash")
        || text.starts_with("#!/usr/bin/env bash")
        || lowercase.contains("sudo apt ")
        || lowercase.contains("apt-get ")
    {
        return Some("Shell");
    }

    if lowercase.starts_with("select ")
        || lowercase.starts_with("insert into ")
        || lowercase.starts_with("update ")
        || lowercase.starts_with("delete from ")
        || lowercase.starts_with("create table ")
        || lowercase.contains("\nfrom ")
    {
        return Some("SQL");
    }

    if lowercase.contains("fn main(")
        || lowercase.starts_with("use std::")
        || lowercase.contains("let mut ")
        || lowercase.contains("impl ") && lowercase.contains(" for ")
    {
        return Some("Rust");
    }

    if lowercase.starts_with("def ")
        || lowercase.contains("\ndef ")
        || lowercase.starts_with("from ") && lowercase.contains(" import ")
        || lowercase.starts_with("import ")
            && text.lines().any(|line| line.trim_end().ends_with(':'))
    {
        return Some("Python");
    }

    if lowercase.contains("function ")
        || lowercase.contains("const ") && lowercase.contains("=>")
        || lowercase.contains("let ") && lowercase.contains("=>")
        || lowercase.contains("console.log(")
    {
        return Some("JavaScript");
    }

    if lowercase.contains("interface ")
        || lowercase.contains("type ") && lowercase.contains(" = {")
        || lowercase.contains(": string")
        || lowercase.contains(": number")
    {
        return Some("TypeScript");
    }

    if lowercase.starts_with("public class ")
        || lowercase.contains("public static void main")
        || lowercase.starts_with("package ") && lowercase.contains("import java.")
    {
        return Some("Java");
    }

    if lowercase.starts_with("<?php")
        || lowercase.contains("$this->")
        || lowercase.contains("namespace ") && lowercase.contains("use ")
    {
        return Some("PHP");
    }

    if looks_like_json(text) {
        return Some("JSON");
    }

    if looks_like_css(text) {
        return Some("CSS");
    }

    if looks_like_generic_code(text) {
        return Some("Code");
    }

    None
}

fn looks_like_json(text: &str) -> bool {
    let trimmed = text.trim();

    let has_json_boundaries = trimmed.starts_with('{') && trimmed.ends_with('}')
        || trimmed.starts_with('[') && trimmed.ends_with(']');

    has_json_boundaries && (trimmed.contains("\":") || trimmed == "{}" || trimmed == "[]")
}

fn looks_like_css(text: &str) -> bool {
    text.contains('{')
        && text.contains('}')
        && text.lines().any(|line| {
            let line = line.trim();

            line.contains(':') && line.ends_with(';')
        })
}

fn looks_like_generic_code(text: &str) -> bool {
    if text.lines().count() < 2 {
        return false;
    }

    let mut score = 0_u8;

    if text.contains('{') && text.contains('}') {
        score += 2;
    }

    if text.contains('(') && text.contains(')') {
        score += 1;
    }

    if text.contains(';') {
        score += 1;
    }

    if text
        .lines()
        .any(|line| line.starts_with("    ") || line.starts_with('\t'))
    {
        score += 1;
    }

    if text.contains("//")
        || text.contains("/*")
        || text.lines().any(|line| line.trim_start().starts_with('#'))
    {
        score += 1;
    }

    score >= 3
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mime_types(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn detects_plain_text() {
        let result = classify_text("Bonjour ClipH", None, &mime_types(&["text/plain"]));

        assert_eq!(result.classification, ClipboardClassification::PlainText,);
    }

    #[test]
    fn detects_rich_text() {
        let result = classify_text(
            "Bonjour ClipH",
            Some("<strong>Bonjour ClipH</strong>"),
            &mime_types(&["text/plain", "text/html"]),
        );

        assert_eq!(result.classification, ClipboardClassification::RichText,);

        assert_eq!(result.subtype.as_deref(), Some("HTML"),);
    }

    #[test]
    fn detects_https_link() {
        let result = classify_text(
            "https://example.com/page",
            None,
            &mime_types(&["text/plain"]),
        );

        assert_eq!(result.classification, ClipboardClassification::Link,);

        assert_eq!(result.subtype.as_deref(), Some("HTTPS"),);
    }

    #[test]
    fn detects_email_address() {
        let result = classify_text("bonjour@example.com", None, &mime_types(&["text/plain"]));

        assert_eq!(result.classification, ClipboardClassification::Link,);

        assert_eq!(result.subtype.as_deref(), Some("E-mail"),);
    }

    #[test]
    fn detects_rust_code() {
        let result = classify_text(
            "fn main() {\n    let mut value = 1;\n    println!(\"{value}\");\n}",
            None,
            &mime_types(&["text/plain"]),
        );

        assert_eq!(result.classification, ClipboardClassification::Code,);

        assert_eq!(result.subtype.as_deref(), Some("Rust"),);
    }

    #[test]
    fn detects_python_code() {
        let result = classify_text(
            "def greet(name):\n    print(name)",
            None,
            &mime_types(&["text/plain"]),
        );

        assert_eq!(result.classification, ClipboardClassification::Code,);

        assert_eq!(result.subtype.as_deref(), Some("Python"),);
    }

    #[test]
    fn detects_html_table() {
        let html = concat!(
            "<table>",
            "<tr><th>Nom</th><th>Âge</th></tr>",
            "<tr><td>Ada</td><td>25</td></tr>",
            "</table>",
        );

        let result = classify_text(
            "Nom\tÂge\nAda\t25",
            Some(html),
            &mime_types(&["text/plain", "text/html"]),
        );

        assert_eq!(result.classification, ClipboardClassification::Table,);
    }

    #[test]
    fn detects_tsv_table() {
        let result = classify_text(
            "Nom\tÂge\nAda\t25\nLinus\t30",
            None,
            &mime_types(&["text/plain", "text/tab-separated-values"]),
        );

        assert_eq!(result.classification, ClipboardClassification::Table,);

        assert_eq!(result.subtype.as_deref(), Some("TSV"),);
    }

    #[test]
    fn detects_hex_color() {
        let result = classify_text("#FF5500", None, &mime_types(&["text/plain"]));

        assert_eq!(result.classification, ClipboardClassification::Color,);

        assert_eq!(result.subtype.as_deref(), Some("HEX"),);
    }

    #[test]
    fn detects_rtf_content() {
        let result = classify_text(r"{\rtf1\ansi Bonjour}", None, &mime_types(&["text/rtf"]));

        assert_eq!(result.classification, ClipboardClassification::Rtf,);
    }
}
