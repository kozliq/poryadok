//! Текст файла — для правил, которые узнают документ по содержанию.
//!
//! Читаем только начало: заголовок договора или билета почти всегда в первых строках.
//! Ничего не уходит в сеть: PDF и документы разбираются здесь же, а текст на картинках
//! распознаёт сама система (Vision на Маке, Windows OCR), без установки чего-либо.
//! Облачные файлы не читаем вовсе — иначе система их скачает.

use crate::platform;
use std::fs;
use std::io::Read;
use std::path::Path;

/// Сколько знаков текста нужно правилам. Больше — только медленнее.
pub const MAX_CHARS: usize = 4000;
const MAX_DOC_BYTES: u64 = 40 << 20;
const MAX_IMAGE_BYTES: u64 = 25 << 20;
const MAX_PLAIN_BYTES: u64 = 64 << 10;

/// Картинки, которые система умеет распознать.
pub const OCR_EXTS: &[&str] = &["png", "jpg", "jpeg", "heic", "heif", "tif", "tiff", "bmp", "gif", "webp"];

pub fn read(path: &Path, ext: &str) -> Option<String> {
    let meta = fs::metadata(path).ok()?;
    if !meta.is_file() || platform::in_cloud(&meta) {
        return None;
    }
    let size = meta.len();
    let text = match ext {
        "txt" | "md" | "markdown" | "csv" | "tsv" => plain(path)?,
        "rtf" => strip_rtf(&plain(path)?),
        "html" | "htm" => strip_tags(&plain(path)?),
        "pdf" if size <= MAX_DOC_BYTES => pdf(path)?,
        "docx" if size <= MAX_DOC_BYTES => zipped_xml(path, "word/document.xml")?,
        "odt" if size <= MAX_DOC_BYTES => zipped_xml(path, "content.xml")?,
        e if OCR_EXTS.contains(&e) && size <= MAX_IMAGE_BYTES => platform::ocr(path)?,
        _ => return None,
    };
    let text: String = text.chars().take(MAX_CHARS).collect();
    (!text.trim().is_empty()).then_some(text)
}

fn plain(path: &Path) -> Option<String> {
    let mut buf = Vec::new();
    fs::File::open(path).ok()?.take(MAX_PLAIN_BYTES).read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// PDF разбирает библиотека на чистом Rust. Битые PDF бывают такими, что она падает, —
/// ловим это, чтобы один кривой файл не ронял весь план.
fn pdf(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    std::panic::catch_unwind(|| pdf_extract::extract_text_from_mem(&bytes).ok()).ok().flatten()
}

/// DOCX и ODT — это zip с XML внутри. Достаём текст между тегами.
fn zipped_xml(path: &Path, inner: &str) -> Option<String> {
    let mut archive = zip::ZipArchive::new(fs::File::open(path).ok()?).ok()?;
    let mut xml = String::new();
    archive.by_name(inner).ok()?.take(4 << 20).read_to_string(&mut xml).ok()?;
    Some(strip_tags(&xml))
}

/// Текст без тегов: `<w:t>Договор</w:t><w:t>№ 5</w:t>` → «Договор № 5».
/// Конец абзаца превращаем в перевод строки, сущности вроде `&amp;` — в символы.
pub fn strip_tags(xml: &str) -> String {
    let mut out = String::with_capacity(xml.len() / 4);
    let mut rest = xml;
    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        let Some(close) = rest[open..].find('>') else { break };
        let tag = &rest[open + 1..open + close];
        if tag.starts_with("/w:p") || tag.starts_with("/text:p") || tag.starts_with("br") || tag.starts_with("/p") {
            out.push('\n');
        } else if tag.starts_with("w:tab") {
            out.push(' ');
        }
        rest = &rest[open + close + 1..];
    }
    out.push_str(rest);
    out.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&nbsp;", " ")
}

/// RTF без управляющих слов: `{\rtf1\ansi Договор \par}` → «Договор».
/// Русский в RTF часто записан кодами `\'e4` (cp1251) — их переводим.
pub fn strip_rtf(rtf: &str) -> String {
    let mut out = String::new();
    let mut chars = rtf.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' | '}' => {}
            '\\' => match chars.peek() {
                Some('\'') => {
                    chars.next();
                    let hex: String = chars.by_ref().take(2).collect();
                    if let Ok(b) = u8::from_str_radix(&hex, 16) {
                        out.push(cp1251(b));
                    }
                }
                Some(c) if c.is_ascii_alphabetic() => {
                    let word: String = std::iter::from_fn(|| chars.next_if(|c| c.is_ascii_alphabetic())).collect();
                    while chars.next_if(|c| c.is_ascii_digit() || *c == '-').is_some() {}
                    chars.next_if(|c| *c == ' ');
                    if word == "par" || word == "line" {
                        out.push('\n');
                    }
                }
                _ => {
                    chars.next();
                }
            },
            '\r' | '\n' => {}
            _ => out.push(c),
        }
    }
    out
}

/// Байт кодировки Windows-1251 → буква.
fn cp1251(b: u8) -> char {
    match b {
        0xC0..=0xFF => char::from_u32(0x410 + u32::from(b - 0xC0)).unwrap_or(' '),
        0xA8 => 'Ё',
        0xB8 => 'ё',
        b if b < 0x80 => b as char,
        _ => ' ',
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_xml() {
        let xml = r#"<w:p><w:r><w:t>Договор</w:t></w:r><w:r><w:t xml:space="preserve"> аренды &amp; услуг</w:t></w:r></w:p><w:p><w:t>№ 5</w:t></w:p>"#;
        assert_eq!(strip_tags(xml), "Договор аренды & услуг\n№ 5\n");
    }

    #[test]
    fn strips_rtf_with_cp1251() {
        let rtf = r"{\rtf1\ansi\ansicpg1251 {\fonttbl}\'c4\'ee\'e3\'ee\'e2\'ee\'f0 \b1 аренды\b0\par}";
        assert_eq!(strip_rtf(rtf).trim(), "Договор аренды");
    }
}
