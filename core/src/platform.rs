//! Всё, чем Мак и Windows отличаются, и всё, что программа спрашивает у системы.
//! Остальной код про системы не знает.

use std::fs;
use std::path::{Path, PathBuf};

/// Служебные файлы, которые человек не видит и трогать не надо.
pub fn is_hidden(name: &str, meta: &fs::Metadata) -> bool {
    let lower = name.to_lowercase();
    if name.starts_with('.') || matches!(lower.as_str(), "desktop.ini" | "thumbs.db" | "$recycle.bin" | "icon\r" | "__macosx") {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const HIDDEN: u32 = 0x2;
        const SYSTEM: u32 = 0x4;
        if meta.file_attributes() & (HIDDEN | SYSTEM) != 0 {
            return true;
        }
    }
    let _ = meta;
    false
}

/// Файл лежит только в облаке (iCloud «Оптимизация хранилища», OneDrive «Файлы по запросу»).
/// Читать его нельзя: чтение скачает файл целиком.
pub fn in_cloud(meta: &fs::Metadata) -> bool {
    #[cfg(target_os = "macos")]
    {
        use std::os::macos::fs::MetadataExt;
        const SF_DATALESS: u32 = 0x4000_0000;
        meta.st_flags() & SF_DATALESS != 0
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const OFFLINE: u32 = 0x1000;
        const RECALL_ON_DATA_ACCESS: u32 = 0x40_0000;
        meta.file_attributes() & (OFFLINE | RECALL_ON_DATA_ACCESS) != 0
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = meta;
        false
    }
}

/// Папки, которые синхронизирует облако. Облачный файл, переехавший внутри них,
/// так и остаётся в облаке; переехавший наружу — скачивается. Проверено на iCloud:
/// «Рабочий стол → Документы» — без скачивания, «Рабочий стол → Фильмы» — со скачиванием.
pub fn cloud_roots(home: &Path) -> Vec<PathBuf> {
    #[cfg_attr(not(any(target_os = "macos", windows)), allow(unused_mut))]
    let mut roots = Vec::new();
    #[cfg(target_os = "macos")]
    {
        let drive = home.join("Library/Mobile Documents");
        let docs = drive.join("com~apple~CloudDocs");
        // «Рабочий стол и Документы в iCloud» включены, если в iCloud Drive есть эти папки.
        for name in ["Desktop", "Documents"] {
            if docs.join(name).is_dir() {
                roots.push(home.join(name));
            }
        }
        roots.push(drive);
    }
    #[cfg(windows)]
    {
        for var in ["OneDrive", "OneDriveConsumer", "OneDriveCommercial"] {
            if let Some(p) = std::env::var_os(var) {
                roots.push(PathBuf::from(p));
            }
        }
        let _ = home;
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    let _ = home;
    roots
}

/// Ярлык: псевдоним Finder (закладка `book…mark`) или `.lnk` из Windows.
/// Облачные и большие файлы не читаем: псевдонимы маленькие, а чтение облачного файла его скачает.
pub fn is_shortcut(path: &Path, ext: &str, meta: &fs::Metadata) -> bool {
    if ext == "lnk" {
        return true;
    }
    if in_cloud(meta) || meta.len() > 1 << 20 {
        return false;
    }
    let mut head = [0u8; 12];
    match fs::File::open(path).and_then(|mut f| std::io::Read::read(&mut f, &mut head)) {
        Ok(12) => &head[0..4] == b"book" && &head[8..12] == b"mark",
        _ => false,
    }
}

/// Кнопки-скрипты и программы, которые человек запускает сам. Это инструменты, а не мусор.
pub fn is_program(ext: &str, meta: &fs::Metadata) -> bool {
    if matches!(ext, "command" | "tool" | "bat" | "cmd" | "ps1") {
        return true;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let executable = meta.permissions().mode() & 0o111 != 0;
        if executable && matches!(ext, "" | "sh" | "zsh" | "bash" | "py" | "pl" | "rb" | "bin" | "run" | "out") {
            return true;
        }
    }
    let _ = meta;
    false
}

/// Пакет программы на Маке — папка, которая выглядит как файл.
pub fn is_bundle(name: &str) -> bool {
    let n = name.to_lowercase();
    [".app", ".bundle", ".framework", ".photoslibrary", ".musiclibrary", ".tvlibrary", ".imovielibrary", ".fcpbundle", ".logicx"]
        .iter()
        .any(|s| n.ends_with(s))
}

/// Файл ещё качается.
pub fn is_partial(ext: &str) -> bool {
    matches!(ext, "crdownload" | "part" | "partial" | "download" | "opdownload" | "tmp")
}

/// Куда Мак сохраняет скриншоты: `defaults read com.apple.screencapture location`.
pub fn screenshot_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let o = std::process::Command::new("defaults").args(["read", "com.apple.screencapture", "location"]).output().ok()?;
        if !o.status.success() {
            return None;
        }
        let raw = String::from_utf8_lossy(&o.stdout).trim().to_string();
        let path = match raw.strip_prefix('~') {
            Some(rest) => dirs::home_dir()?.join(rest.trim_start_matches('/')),
            None => PathBuf::from(raw),
        };
        path.is_absolute().then_some(path)
    }
    #[cfg(not(target_os = "macos"))]
    None
}

/// Рабочие папки запущенных программ: `lsof -a -d cwd`. Если в папке сейчас открыт
/// терминал, редактор или чат, её перенос сломает им работу.
/// Пара: (папка, имя программы).
pub fn busy_dirs() -> Vec<(PathBuf, String)> {
    #[cfg(unix)]
    {
        let Ok(o) = std::process::Command::new("lsof").args(["-w", "-a", "-d", "cwd", "-Fcn"]).output() else {
            return vec![];
        };
        parse_lsof(&String::from_utf8_lossy(&o.stdout))
    }
    #[cfg(not(unix))]
    vec![]
}

/// Разбор машинного вывода lsof: строки `p<pid>`, `c<команда>`, `n<путь>`.
pub fn parse_lsof(out: &str) -> Vec<(PathBuf, String)> {
    let mut command = String::new();
    let mut found = Vec::new();
    for line in out.lines() {
        match line.split_at_checked(1) {
            Some(("c", c)) => command = unescape_lsof(c),
            Some(("n", n)) if n.starts_with('/') => found.push((PathBuf::from(unescape_lsof(n)), command.clone())),
            _ => {}
        }
    }
    found
}

/// lsof печатает всё, что не ASCII, кодами: «Чисто» → `\xd0\xa7\xd0\xb8…`. Собираем байты обратно.
fn unescape_lsof(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 3 < b.len() && b[i + 1] == b'x' {
            if let Ok(byte) = u8::from_str_radix(&s[i + 2..i + 4], 16) {
                out.push(byte);
                i += 4;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Где искать скрипты и фоновые задачи, которые ссылаются на папки.
pub fn agent_dirs(home: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![home.join(".local/bin")];
    if cfg!(target_os = "macos") {
        dirs.push(home.join("Library/LaunchAgents"));
    }
    dirs
}

/// Текст файла настроек или скрипта. Двоичный plist переводим в XML через `plutil`.
/// Скрипт из облака читаем, только если он крошечный: скачать пару килобайт — честная цена
/// за то, чтобы не сломать человеку его инструмент.
pub fn read_text(path: &Path) -> Option<String> {
    let meta = fs::metadata(path).ok()?;
    let limit = if in_cloud(&meta) { 64 * 1024 } else { 256 * 1024 };
    if !meta.is_file() || meta.len() > limit {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    #[cfg(target_os = "macos")]
    if bytes.starts_with(b"bplist") {
        let o = std::process::Command::new("plutil").args(["-convert", "xml1", "-o", "-"]).arg(path).output().ok()?;
        return Some(String::from_utf8_lossy(&o.stdout).into_owned());
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Ищет в тексте пути внутри дома: `/Users/me/Desktop/videos`, `~/Desktop/videos`,
/// `$HOME/Desktop/videos`, `"${HOME}/Мои видео"`, `%USERPROFILE%\Desktop\videos`.
/// Путь в кавычках может содержать пробелы; без кавычек — до пробела или знака оболочки.
pub fn find_paths(text: &str, home: &Path) -> Vec<PathBuf> {
    let home_str = home.to_string_lossy();
    let anchors: [&str; 6] = [&home_str, "~", "$HOME", "${HOME}", "%USERPROFILE%", "%userprofile%"];
    let mut found = Vec::new();
    for anchor in anchors {
        if anchor.is_empty() {
            continue;
        }
        let mut from = 0;
        while let Some(pos) = text[from..].find(anchor) {
            let start = from + pos;
            from = start + anchor.len();
            let rest = &text[from..];
            let Some(sep) = rest.chars().next().filter(|c| *c == '/' || *c == '\\') else { continue };
            // `~` внутри слова («a~b») и `/Users/me2` — не наши пути.
            if anchor == "~" && text[..start].chars().next_back().is_some_and(|c| c.is_alphanumeric()) {
                continue;
            }
            let quote = text[..start].chars().next_back().filter(|c| *c == '"' || *c == '\'');
            let mut tail = String::new();
            let mut chars = rest[sep.len_utf8()..].chars().peekable();
            while let Some(c) = chars.next() {
                match quote {
                    Some(q) if c == q || c == '\n' => break,
                    Some(_) => tail.push(c),
                    // `Мои\ файлы` в оболочке — экранированный пробел, а не конец пути.
                    None if c == '\\' && chars.peek().is_some_and(|n| " ()&'\"".contains(*n)) => tail.extend(chars.next()),
                    None if c.is_whitespace() || "\"'<>;&|()`".contains(c) => break,
                    None => tail.push(c),
                }
            }
            // Дальше переменных и масок не идём: `$HOME/Desktop/videos/$NAME` → `Desktop/videos`.
            let clean: String = tail.chars().take_while(|c| !"$*?[{%".contains(*c)).collect();
            let mut path = home.to_path_buf();
            for part in clean.split(['/', '\\']).filter(|p| !p.is_empty() && *p != ".") {
                path.push(part);
            }
            if path != home && !found.contains(&path) {
                found.push(path);
            }
        }
    }
    found
}

/// Папка, названная как установленная программа, — скорее всего, её рабочая папка
/// (так делают DragShift, OBS, Zoom). Возвращает имя программы.
pub fn app_named(name: &str) -> Option<String> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if cfg!(target_os = "macos") {
        candidates.push(PathBuf::from("/Applications").join(format!("{name}.app")));
        if let Some(h) = dirs::home_dir() {
            candidates.push(h.join("Applications").join(format!("{name}.app")));
        }
    }
    if cfg!(windows) {
        for var in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
            if let Some(p) = std::env::var_os(var) {
                let base = PathBuf::from(p);
                candidates.push(base.join(name));
                candidates.push(base.join("Programs").join(name));
            }
        }
    }
    candidates.iter().any(|p| p.exists()).then(|| name.to_string())
}

/// Текст на картинке (см. [`crate::ocr`]).
pub fn ocr(path: &Path) -> Option<String> {
    crate::ocr::recognize(path)
}
