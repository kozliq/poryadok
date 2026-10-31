//! Снимок папок: единственное место, которое читает диск перед планом.
//! Дальше план работает только со снимком — поэтому его легко проверять тестами.
//!
//! Облачные файлы (iCloud, OneDrive) здесь никогда не читаются — только их свойства.
//! Иначе простой просмотр Рабочего стола скачал бы гигабайты.

use crate::rules::RuleSet;
use crate::{civil, content, platform};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// Признаки живого проекта: такие папки не трогаем, даже если просят.
const PROJECT_MARKERS: &[&str] = &[
    ".git", ".hg", ".svn", "package.json", "Cargo.toml", "pyproject.toml", "requirements.txt", "go.mod", "Gemfile",
    "pom.xml", "build.gradle", "Makefile", "CMakeLists.txt", "node_modules", ".claude", "CLAUDE.md", "AGENTS.md",
    ".vscode", ".idea", ".obsidian", ".venv", "venv",
];
/// Больше стольких файлов внутри — папку целиком не двигаем, пусть человек решит сам.
pub const DIR_SCAN_LIMIT: usize = 5000;
/// Сколько файлов за один план читаем по тексту. Остальные — по названию.
pub const TEXT_LIMIT: usize = 300;

/// Что делать при снимке.
#[derive(Clone, Copy, Default)]
pub struct ScanOptions<'a> {
    /// Смотреть внутрь папок — нужно, чтобы двигать папки целиком.
    pub look_inside: bool,
    /// Спрашивать систему: lsof, скриншоты, фоновые задачи, программы.
    /// В тестах выключено, чтобы снимок зависел только от самих папок.
    pub ask_system: bool,
    /// Читать текст файлов, которые эти правила не узнали по названию.
    pub read_text: Option<&'a RuleSet>,
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    /// Какие папки разбираем.
    pub folders: Vec<PathBuf>,
    pub items: Vec<Item>,
    /// Что трогать нельзя и почему: занято программой, нужно скрипту, сюда пишет система.
    pub guards: Vec<Guard>,
    /// Папки, которые синхронизирует облако (см. [`platform::cloud_roots`]).
    pub cloud_roots: Vec<PathBuf>,
    /// Когда сделан снимок, секунды Unix. От него считаем «только что появился».
    pub taken_at: i64,
}

#[derive(Debug, Clone)]
pub struct Item {
    pub path: PathBuf,
    pub name: String,
    pub ext: String,
    pub kind: Kind,
    pub size: u64,
    /// Когда появился (для папок по месяцам). Если система не знает — когда менялся.
    pub created: i64,
    /// Самое позднее из «появился» и «менялся» — для правила «только что появился».
    pub touched: i64,
    pub in_cloud: bool,
    /// Начало текста файла, если его прочитали (PDF, документ, картинка через распознавание).
    pub text: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    File,
    /// Ярлык или символическая ссылка.
    Link,
    /// Программа или пакет (`.app`, `.photoslibrary`).
    Bundle,
    /// Скрипт или программа без пакета, которую человек запускает сам.
    Program,
    Dir(DirInfo),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct DirInfo {
    /// Чем папка похожа на проект (`.git`, `package.json`…).
    pub project: Option<String>,
    /// Имена файлов внутри, на любой глубине. Заполняется, только если смотрели внутрь.
    pub files: Vec<String>,
    pub looked_inside: bool,
    pub too_big: bool,
    pub has_bundle: bool,
    /// Сколько внутри скрытых вещей (кроме служебных `.DS_Store`). Папка из одних
    /// скрытых файлов — это данные программы, а не пустая папка.
    pub hidden: usize,
    /// Когда внутри последний раз что-то менялось — для имени безымянной папки.
    pub newest: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Guard {
    pub path: PathBuf,
    pub why: GuardKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum GuardKind {
    /// Сюда система сохраняет скриншоты.
    Screenshots,
    /// Внутри сейчас работает программа (её рабочая папка).
    Busy(String),
    /// Путь записан в скрипте или фоновой задаче.
    UsedBy(String),
    /// Папка названа как установленная программа — её рабочая папка.
    AppFolder(String),
}

/// Снимок одной или нескольких папок плюс всё, что система знает о защите.
pub fn scan(folders: &[PathBuf], home: &Path, opts: ScanOptions) -> io::Result<Snapshot> {
    let folders: Vec<PathBuf> = folders.iter().map(std::path::absolute).collect::<io::Result<_>>()?;
    let mut items = Vec::new();
    for folder in &folders {
        items.extend(scan_one(folder, opts.look_inside)?);
    }
    if let Some(rules) = opts.read_text {
        read_texts(&mut items, rules);
    }
    Ok(Snapshot {
        guards: guards(&folders, &items, home, opts.ask_system),
        cloud_roots: platform::cloud_roots(home),
        taken_at: civil::now(),
        folders,
        items,
    })
}

fn scan_one(folder: &Path, look_inside_dirs: bool) -> io::Result<Vec<Item>> {
    let mut items = Vec::new();
    for entry in fs::read_dir(folder)?.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(meta) = fs::symlink_metadata(&path) else { continue };
        if platform::is_hidden(&name, &meta) {
            continue;
        }
        let ext = if meta.is_dir() { String::new() } else { extension(&name) };
        let in_cloud = platform::in_cloud(&meta);
        let mut size = meta.len();
        let kind = if meta.file_type().is_symlink() {
            Kind::Link
        } else if meta.is_dir() && platform::is_bundle(&name) {
            Kind::Bundle
        } else if meta.is_dir() {
            let (info, inner) = inspect_dir(&path, look_inside_dirs);
            size = inner;
            Kind::Dir(info)
        } else if platform::is_shortcut(&path, &ext, &meta) {
            Kind::Link
        } else if platform::is_program(&ext, &meta) {
            Kind::Program
        } else {
            Kind::File
        };
        let modified = meta.modified().map(civil::unix).unwrap_or(0);
        let created = meta.created().map(civil::unix).unwrap_or(modified);
        items.push(Item { path, name, ext, kind, size, created, touched: created.max(modified), in_cloud, text: None });
    }
    // Стабильный порядок: план не должен зависеть от того, как диск вернул файлы.
    items.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(items)
}

/// Текст для тех файлов, которые правила не узнали по названию. Читаем в несколько потоков:
/// распознавание картинки занимает доли секунды, и на сотне скриншотов это заметно.
fn read_texts(items: &mut [Item], rules: &RuleSet) {
    let jobs: Vec<(usize, PathBuf, String)> = items
        .iter()
        .enumerate()
        .filter(|(_, i)| i.kind == Kind::File && !i.in_cloud && i.size > 0 && rules.wants_text(&i.name))
        .map(|(n, i)| (n, i.path.clone(), i.ext.clone()))
        .take(TEXT_LIMIT)
        .collect();
    let next = AtomicUsize::new(0);
    let found = Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(8);
    std::thread::scope(|s| {
        for _ in 0..workers.min(jobs.len()) {
            s.spawn(|| {
                while let Some((n, path, ext)) = jobs.get(next.fetch_add(1, Ordering::Relaxed)) {
                    if let Some(text) = content::read(path, ext) {
                        found.lock().unwrap_or_else(|e| e.into_inner()).push((*n, text));
                    }
                }
            });
        }
    });
    for (n, text) in found.into_inner().unwrap_or_else(|e| e.into_inner()) {
        items[n].text = Some(text);
    }
}

pub fn extension(name: &str) -> String {
    Path::new(name).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// Всё, что защищает вещи от переноса. Собирается один раз на снимок.
fn guards(folders: &[PathBuf], items: &[Item], home: &Path, ask_system: bool) -> Vec<Guard> {
    let mut out = Vec::new();
    // Скрипты из самих разбираемых папок и фоновые задачи системы.
    let mut sources: Vec<PathBuf> = items.iter().filter(|i| i.kind == Kind::Program).map(|i| i.path.clone()).collect();
    if ask_system {
        if let Some(p) = platform::screenshot_dir() {
            out.push(Guard { path: p, why: GuardKind::Screenshots });
        }
        for (path, program) in platform::busy_dirs() {
            out.push(Guard { path, why: GuardKind::Busy(program) });
        }
        for dir in platform::agent_dirs(home) {
            if let Ok(rd) = fs::read_dir(dir) {
                sources.extend(rd.flatten().map(|e| e.path()));
            }
        }
    }
    for src in sources {
        let Some(text) = platform::read_text(&src) else { continue };
        let who = src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        for path in platform::find_paths(&text, home) {
            // Нас интересуют только пути внутри разбираемых папок.
            if folders.iter().any(|f| path.starts_with(f)) {
                out.push(Guard { path, why: GuardKind::UsedBy(who.clone()) });
            }
        }
    }
    // Символическая ссылка сломается, если увезти то, на что она указывает.
    for item in items.iter().filter(|i| i.kind == Kind::Link) {
        let Ok(target) = fs::read_link(&item.path) else { continue };
        let target = item.path.parent().map(|p| p.join(&target)).unwrap_or(target);
        if folders.iter().any(|f| target.starts_with(f)) {
            out.push(Guard { path: target, why: GuardKind::UsedBy(format!("ссылки «{}»", item.name)) });
        }
    }
    for item in items.iter().filter(|_| ask_system) {
        if matches!(item.kind, Kind::Dir(_)) {
            if let Some(app) = platform::app_named(&item.name) {
                out.push(Guard { path: item.path.clone(), why: GuardKind::AppFolder(app) });
            }
        }
    }
    out
}

fn project_marker(dir: &Path) -> Option<String> {
    if let Some(m) = PROJECT_MARKERS.iter().find(|m| dir.join(m).exists()) {
        return Some(m.to_string());
    }
    fs::read_dir(dir).ok()?.flatten().find_map(|e| {
        let n = e.file_name().to_string_lossy().to_lowercase();
        (n.ends_with(".xcodeproj") || n.ends_with(".xcworkspace") || n.ends_with(".sln")).then(|| "проект IDE".to_string())
    })
}

/// Что внутри папки. Обход без рекурсии, с пределом — огромные папки не тормозят план.
fn inspect_dir(dir: &Path, look_inside: bool) -> (DirInfo, u64) {
    let mut info = DirInfo { project: project_marker(dir), looked_inside: look_inside, ..Default::default() };
    let mut size = 0;
    if !look_inside || info.project.is_some() {
        return (info, size);
    }
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let Ok(meta) = fs::symlink_metadata(e.path()) else { continue };
            if platform::is_hidden(&name, &meta) {
                if name == ".git" {
                    info.project = Some(".git внутри".into());
                    return (info, size);
                }
                if !matches!(name.as_str(), ".DS_Store" | ".localized" | "Icon\r") {
                    info.hidden += 1;
                }
                continue;
            }
            if meta.is_dir() {
                if platform::is_bundle(&name) {
                    info.has_bundle = true;
                } else {
                    stack.push(e.path());
                }
                continue;
            }
            size += meta.len();
            info.newest = info.newest.max(meta.modified().map(civil::unix).unwrap_or(0));
            info.files.push(name);
            if info.files.len() > DIR_SCAN_LIMIT {
                info.too_big = true;
                return (info, size);
            }
        }
    }
    (info, size)
}
