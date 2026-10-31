//! План: «снимок → что куда». Сам диск не меняет.
//!
//! Всё, что план знает о мире, приходит аргументами: снимок папок, правила, места
//! и [`Disk`] — три вопроса к диску («занято ли имя», «что лежит на полке»,
//! «одинаковые ли файлы»). Поэтому одинаковый вход даёт одинаковый план,
//! а в тестах диск можно подменить.

use crate::civil;
use crate::places::{Place, Places};
use crate::platform;
use crate::rules::{Evidence, Rule, RuleSet};
use crate::scan::{DirInfo, GuardKind, Item, Kind, Snapshot};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

/// Доля файлов одного вида, при которой папка едет целиком.
pub const FOLDER_MAJORITY: f64 = 0.7;
/// Файл моложе двух минут не трогаем: вдруг он ещё качается или сохраняется.
pub const FRESH_SECONDS: i64 = 120;

/// Что план может спросить у диска. Настоящий диск — [`crate::exec::RealDisk`].
pub trait Disk {
    fn exists(&self, path: &Path) -> bool;
    /// Обычные файлы прямо в папке: (путь, размер). Облачные не отдавать — их не сравнить без скачивания.
    fn files_in(&self, dir: &Path) -> Vec<(PathBuf, u64)>;
    /// Байт в байт одинаковые файлы.
    fn same_content(&self, a: &Path, b: &Path) -> bool;
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Options {
    /// Двигать и папки целиком, если почти всё внутри одного вида.
    #[serde(default)]
    pub folders: bool,
    /// Куда класть то, что лучше посмотреть и удалить руками: дубли, пустые, установщики.
    #[serde(default)]
    pub review: Review,
    /// Решать только по названиям, не читая текст файлов.
    #[serde(default)]
    pub names_only: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Review {
    pub place: Place,
    pub path: String,
}

impl Default for Review {
    fn default() -> Self {
        Review { place: Place::Desktop, path: "На удаление".into() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Move {
    pub src: PathBuf,
    pub dst: PathBuf,
    /// Имя полки: «Скриншоты».
    pub rule: String,
    pub place: Place,
    /// Полка внутри места: `Скриншоты/2026-10`.
    pub shelf: String,
    /// Полка разбита по месяцам.
    pub dated: bool,
    pub is_dir: bool,
    pub size: u64,
    /// Лежит только в облаке.
    pub in_cloud: bool,
    /// Облачный файл уезжает из облачной папки — при переносе скачается.
    pub downloads: bool,
    /// Для дублей: такой же файл, который остаётся на месте.
    pub twin: Option<PathBuf>,
    /// Почему сюда — словами для человека.
    pub why: Why,
}

/// Почему вещь едет именно на эту полку.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "by", rename_all = "kebab-case")]
pub enum Why {
    /// Совпало правило: слово в названии, фраза в тексте или расширение.
    Rule { evidence: Evidence },
    /// Папка: большинство файлов внутри — одного вида.
    Majority { count: usize, total: usize },
    /// Точная копия файла, который уже лежит на полке или едет туда.
    Duplicate,
    Empty,
}

impl fmt::Display for Why {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Why::Rule { evidence } => write!(f, "{evidence}"),
            Why::Majority { count, total } => write!(f, "внутри {count} из {total} — такие файлы"),
            Why::Duplicate => write!(f, "точная копия, байт в байт"),
            Why::Empty => write!(f, "пустой"),
        }
    }
}

/// Почему вещь осталась на месте. Отдельный тип, а не строка: экран и тесты
/// опираются на смысл, а текст можно поменять или перевести.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "code", content = "detail", rename_all = "kebab-case")]
pub enum Reason {
    Link,
    Bundle,
    Program,
    Screenshots,
    Busy(String),
    UsedBy(String),
    AppFolder(String),
    Review,
    Project(String),
    FoldersOff,
    Mixed,
    HiddenOnly,
    TooBig,
    Downloading,
    Fresh,
    Unknown,
    AlreadyThere,
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Reason::Link => write!(f, "ярлык"),
            Reason::Bundle => write!(f, "программа"),
            Reason::Program => write!(f, "программа или скрипт"),
            Reason::Screenshots => write!(f, "сюда система сохраняет скриншоты"),
            Reason::Busy(who) => write!(f, "внутри сейчас работает {who}"),
            Reason::UsedBy(who) => write!(f, "нужна для {who}"),
            Reason::AppFolder(app) => write!(f, "рабочая папка программы {app}"),
            Reason::Review => write!(f, "папка на проверку — разберите её сами"),
            Reason::Project(why) => write!(f, "проект ({why})"),
            Reason::FoldersOff => write!(f, "папка — двигаю, только если разрешить"),
            Reason::Mixed => write!(f, "внутри всё вперемешку"),
            Reason::HiddenOnly => write!(f, "внутри только скрытые файлы — похоже на данные программы"),
            Reason::TooBig => write!(f, "очень большая папка"),
            Reason::Downloading => write!(f, "ещё качается"),
            Reason::Fresh => write!(f, "только что появился — вдруг ещё сохраняется"),
            Reason::Unknown => write!(f, "не знаю, куда это"),
            Reason::AlreadyThere => write!(f, "уже на своём месте"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Skip {
    pub path: PathBuf,
    pub reason: Reason,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    pub folders: Vec<PathBuf>,
    pub moves: Vec<Move>,
    pub skipped: Vec<Skip>,
}

impl Plan {
    pub fn total_size(&self) -> u64 {
        self.moves.iter().map(|m| m.size).sum()
    }
    /// Сколько придётся скачать из облака.
    pub fn download_size(&self) -> u64 {
        self.moves.iter().filter(|m| m.downloads).map(|m| m.size).sum()
    }
}

/// Куда решили положить вещь — до проверки имён и дублей.
struct Target {
    rule: String,
    place: Place,
    shelf: PathBuf,
    dated: bool,
    why: Why,
}

impl Target {
    fn from_rule(rule: &Rule, dated: bool, why: Why) -> Self {
        Target { rule: rule.name.clone(), place: rule.place, shelf: PathBuf::from(&rule.path), dated, why }
    }
    fn review(opts: &Options, what: &str, why: Why) -> Self {
        Target { rule: what.into(), place: opts.review.place, shelf: Path::new(&opts.review.path).join(what), dated: false, why }
    }
}

pub fn plan(snap: &Snapshot, rules: &RuleSet, places: &Places, opts: &Options, disk: &dyn Disk) -> Plan {
    let review_dir = places.get(opts.review.place).join(&opts.review.path);
    let mut out = Plan { folders: snap.folders.clone(), ..Default::default() };
    // Имена, которые уже пообещали другим вещам этого плана.
    let mut promised: HashSet<PathBuf> = HashSet::new();
    // Что лежит на полках (с диска) плюс что туда уже едет по плану — для поиска дублей.
    let mut shelves: HashMap<PathBuf, Vec<(PathBuf, u64)>> = HashMap::new();

    for item in &snap.items {
        let mut target = match decide(item, snap, rules, opts, &review_dir) {
            Ok(t) => t,
            Err(reason) => {
                out.skipped.push(Skip { path: item.path.clone(), reason });
                continue;
            }
        };
        let is_dir = matches!(item.kind, Kind::Dir(_));
        let mut shelf = target.shelf.clone();
        if target.dated {
            shelf.push(civil::month_folder(item.created));
        }
        let mut dir = places.get(target.place).join(&shelf);
        // Вещь уже на своей полке — или полка оказалась бы внутри переносимой папки.
        if item.path.parent() == Some(dir.as_path()) || snap.folders.iter().any(|f| f.starts_with(&dir)) || dir.starts_with(&item.path) {
            out.skipped.push(Skip { path: item.path.clone(), reason: Reason::AlreadyThere });
            continue;
        }

        // Точная копия того, что уже лежит на полке (или едет туда в этом же плане), — в дубли.
        let mut twin = None;
        let comparable = !is_dir && !item.in_cloud && item.size > 0 && !dir.starts_with(&review_dir);
        if comparable {
            let known = shelves.entry(dir.clone()).or_insert_with(|| disk.files_in(&dir));
            twin = known.iter().find(|(p, size)| *size == item.size && disk.same_content(&item.path, p)).map(|(p, _)| p.clone());
            match &twin {
                None => known.push((item.path.clone(), item.size)),
                Some(_) => {
                    target = Target::review(opts, "Дубли", Why::Duplicate);
                    shelf = target.shelf.clone();
                    dir = places.get(target.place).join(&shelf);
                }
            }
        }

        let busy = |p: &Path| disk.exists(p) || promised.contains(p);
        let wanted = wanted_name(item, &target, &dir, &busy);
        let dst = free_name(&dir.join(&wanted), is_dir, &busy);
        promised.insert(dst.clone());
        let downloads = item.in_cloud && !snap.cloud_roots.iter().any(|r| dir.starts_with(r));
        out.moves.push(Move {
            src: item.path.clone(),
            dst,
            rule: target.rule,
            place: target.place,
            shelf: shelf.to_string_lossy().replace('\\', "/"),
            dated: target.dated,
            is_dir,
            size: item.size,
            in_cloud: item.in_cloud,
            downloads,
            twin,
            why: target.why,
        });
    }
    out
}

fn decide(item: &Item, snap: &Snapshot, rules: &RuleSet, opts: &Options, review_dir: &Path) -> Result<Target, Reason> {
    // Защита важнее всего: сначала спрашиваем, можно ли вообще трогать.
    if let Some(g) = snap.guards.iter().find(|g| g.path.starts_with(&item.path)) {
        return Err(match &g.why {
            GuardKind::Screenshots => Reason::Screenshots,
            GuardKind::Busy(who) => Reason::Busy(who.clone()),
            GuardKind::UsedBy(who) => Reason::UsedBy(who.clone()),
            GuardKind::AppFolder(app) => Reason::AppFolder(app.clone()),
        });
    }
    if item.path == review_dir {
        return Err(Reason::Review);
    }
    match &item.kind {
        Kind::Link => Err(Reason::Link),
        Kind::Bundle => Err(Reason::Bundle),
        Kind::Program => Err(Reason::Program),
        Kind::File => {
            if platform::is_partial(&item.ext) {
                return Err(Reason::Downloading);
            }
            if snap.taken_at - item.touched < FRESH_SECONDS {
                return Err(Reason::Fresh);
            }
            if item.size == 0 && !item.in_cloud {
                return Ok(Target::review(opts, "Пустые", Why::Empty));
            }
            let (i, evidence) = rules.explain(&item.name, item.text.as_deref()).ok_or(Reason::Unknown)?;
            let rule = rules.get(i);
            Ok(Target::from_rule(rule, rule.by_month, Why::Rule { evidence }))
        }
        Kind::Dir(info) => {
            if let Some(marker) = &info.project {
                return Err(Reason::Project(marker.clone()));
            }
            if !opts.folders || !info.looked_inside {
                return Err(Reason::FoldersOff);
            }
            if info.files.is_empty() && !info.has_bundle {
                if info.hidden > 0 {
                    return Err(Reason::HiddenOnly);
                }
                return Ok(Target::review(opts, "Пустые", Why::Empty));
            }
            let (rule, count) = dir_rule(info, rules)?;
            Ok(Target::from_rule(rule, false, Why::Majority { count, total: info.files.len() }))
        }
    }
}

/// Папка едет целиком, если ≥70 % файлов внутри — одного вида.
fn dir_rule<'r>(info: &DirInfo, rules: &'r RuleSet) -> Result<(&'r Rule, usize), Reason> {
    if info.has_bundle {
        return Err(Reason::Bundle);
    }
    if info.too_big {
        return Err(Reason::TooBig);
    }
    let mut votes: HashMap<usize, usize> = HashMap::new();
    for name in &info.files {
        if let Some(i) = rules.classify(name) {
            *votes.entry(i).or_default() += 1;
        }
    }
    // При равенстве голосов побеждает правило выше в списке — результат не случаен.
    let (best, count) = votes.into_iter().max_by_key(|&(i, n)| (n, std::cmp::Reverse(i))).ok_or(Reason::Mixed)?;
    if count as f64 / info.files.len() as f64 >= FOLDER_MAJORITY {
        Ok((rules.get(best), count))
    } else {
        Err(Reason::Mixed)
    }
}

/// Имя на новом месте. Безымянная папка («1», «Новая папка») получает имя по смыслу и дате,
/// а «отчёт (1).pdf» и «отчёт копия.pdf» — чистое «отчёт.pdf», если оно свободно.
fn wanted_name(item: &Item, target: &Target, dir: &Path, busy: &dyn Fn(&Path) -> bool) -> String {
    if let Kind::Dir(info) = &item.kind {
        if is_unnamed(&item.name) {
            let when = if info.newest > 0 { info.newest } else { item.touched };
            return format!("{} {}", target.rule, civil::date_ru(when));
        }
    }
    if target.rule == "Дубли" {
        return item.name.clone();
    }
    match without_copy_suffix(&item.name, matches!(item.kind, Kind::Dir(_))) {
        Some(clean) if !busy(&dir.join(&clean)) => clean,
        _ => item.name.clone(),
    }
}

/// «1», «23», «Новая папка 2», «untitled folder», «Без названия».
pub fn is_unnamed(name: &str) -> bool {
    let n = name.trim().to_lowercase();
    let base = n.trim_end_matches(|c: char| c.is_ascii_digit() || c == ' ');
    let digits_only = !n.is_empty() && n.len() <= 3 && n.chars().all(|c| c.is_ascii_digit());
    digits_only || ["новая папка", "new folder", "untitled folder", "untitled", "без названия", "папка"].contains(&base)
}

/// Убирает хвост копии: «отчёт (1).pdf», «отчёт копия.pdf», «report copy 2.pdf» → «отчёт.pdf».
pub fn without_copy_suffix(name: &str, is_dir: bool) -> Option<String> {
    let (stem, ext) = split_ext(name, is_dir);
    let stem = stem.trim_end();
    let mut clean = stem;
    if let Some(inner) = stem.strip_suffix(')') {
        if let Some(open) = inner.rfind(" (") {
            let num = &inner[open + 2..];
            if (1..=3).contains(&num.len()) && num.chars().all(|c| c.is_ascii_digit()) {
                clean = &inner[..open];
            }
        }
    } else {
        let no_num = stem.trim_end_matches(|c: char| c.is_ascii_digit()).trim_end();
        for word in [" - копия", " - copy", " копия", " copy"] {
            if let Some(t) = no_num.strip_suffix(word) {
                clean = t;
                break;
            }
        }
    }
    let clean = clean.trim_end();
    (clean.len() < stem.len() && !clean.is_empty()).then(|| format!("{clean}{ext}"))
}

fn split_ext(name: &str, is_dir: bool) -> (&str, &str) {
    match name.rfind('.') {
        Some(dot) if dot > 0 && !is_dir => (&name[..dot], &name[dot..]),
        _ => (name, ""),
    }
}

/// Свободное имя: если `фото.jpg` занято — `фото (2).jpg`, потом `(3)`…
/// Ничего не перезаписываем, никогда.
pub fn free_name(wanted: &Path, is_dir: bool, busy: &dyn Fn(&Path) -> bool) -> PathBuf {
    if !busy(wanted) {
        return wanted.to_path_buf();
    }
    let parent = wanted.parent().unwrap_or(Path::new(""));
    let name = wanted.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    // У папок «расширения» нет: «Отпуск 2.0» → «Отпуск 2.0 (2)».
    let (stem, ext) = split_ext(&name, is_dir);
    (2..).map(|i| parent.join(format!("{stem} ({i}){ext}"))).find(|p| !busy(p)).expect("бесконечный ряд")
}
