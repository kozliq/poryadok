//! # Порядок
//!
//! Раскладывает файлы по полкам: скриншоты — к скриншотам, договоры — к договорам.
//! Ничего не удаляет, ничего не перезаписывает, всё можно вернуть.
//!
//! Устроено как «чистое ядро, грязная оболочка»:
//!
//! ```text
//!  диск ──► scan ──► Snapshot ──► plan ──► Plan ──► exec::apply ──► диск
//!            │                       ▲                       │
//!     lsof, скрипты,       RuleSet, Places, Options,     journal (WAL)
//!     скриншоты, iCloud        Disk (3 вопроса)
//!                                                                        │
//!                                                     exec::undo ◄───────┘
//! ```
//!
//! * [`scan`] — единственное чтение диска перед планом.
//! * [`plan`] — решает, что куда, ничего не меняя. Детерминирован: тот же вход — тот же план.
//! * [`exec`] — единственное место, которое меняет диск. Только `rename`, только по журналу.
//! * [`journal`] — журнал с упреждающей записью: откат работает даже после сбоя питания.

pub mod card;
pub mod civil;
pub mod content;
pub mod exec;
pub mod journal;
mod ocr;
pub mod places;
pub mod plan;
pub mod platform;
pub mod rules;
pub mod scan;

pub use exec::{apply, undo, Failure, Outcome, RealDisk};
pub use journal::{Journal, Record};
pub use places::{Place, Places};
pub use plan::{plan, Disk, Move, Options, Plan, Reason, Review, Skip, Why};
pub use rules::{default_rules, Evidence, Rule, RuleSet};
pub use scan::{scan, ScanOptions, Snapshot};

use std::path::{Path, PathBuf};

/// Где программа хранит журналы и свои правила. `PORYADOK_HOME` — чтобы переопределить.
pub fn data_dir() -> PathBuf {
    std::env::var_os("PORYADOK_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::data_dir().unwrap_or_else(std::env::temp_dir).join("poryadok"))
}

/// Файл со своими правилами.
pub fn rules_file() -> PathBuf {
    data_dir().join("rules.json")
}

/// Свои правила, если файл есть и читается; иначе стандартные.
pub fn load_rules() -> Result<RuleSet, String> {
    let path = rules_file();
    match std::fs::read_to_string(&path) {
        Err(_) => Ok(RuleSet::default()),
        Ok(text) => serde_json::from_str::<Vec<Rule>>(&text)
            .map(RuleSet::new)
            .map_err(|e| format!("в {} ошибка: {e}", path.display())),
    }
}

/// Кладёт стандартные правила в файл, чтобы их было удобно править. Существующий не трогает.
pub fn export_rules() -> std::io::Result<PathBuf> {
    let path = rules_file();
    if !path.exists() {
        std::fs::create_dir_all(data_dir())?;
        std::fs::write(&path, serde_json::to_string_pretty(&default_rules())?)?;
    }
    Ok(path)
}

/// Снимок + план за один вызов — то, что нужно и окну, и командной строке.
/// Несколько папок разбираются вместе: имена и дубли согласованы между ними.
pub fn plan_folders(folders: &[PathBuf], rules: &RuleSet, places: &Places, opts: &Options) -> std::io::Result<Plan> {
    let read_text = (!opts.names_only).then_some(rules);
    let snap = scan::scan(folders, &places.home, ScanOptions { look_inside: opts.folders, ask_system: true, read_text })?;
    Ok(plan::plan(&snap, rules, places, opts, &RealDisk))
}

pub fn plan_folder(folder: &Path, rules: &RuleSet, places: &Places, opts: &Options) -> std::io::Result<Plan> {
    plan_folders(&[folder.to_path_buf()], rules, places, opts)
}

/// `1,2 ГБ` — как пишут люди, а не программисты.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["Б", "КБ", "МБ", "ГБ", "ТБ"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} Б")
    } else {
        format!("{value:.1} {}", UNITS[unit]).replace('.', ",")
    }
}

/// «1 файл», «2 файла», «5 файлов».
pub fn plural(n: usize, one: &str, few: &str, many: &str) -> String {
    let word = match (n % 10, n % 100) {
        (1, x) if x != 11 => one,
        (2..=4, x) if !(12..=14).contains(&x) => few,
        _ => many,
    };
    format!("{n} {word}")
}

#[cfg(test)]
mod tests;
