//! Исполнение: перенос по плану и откат по журналу. Единственное место, которое меняет диск.
//!
//! Три правила, которые не нарушаются нигде:
//! 1. Только переименование (`rename`). Никаких «скопировать и удалить» — значит, нечему потеряться.
//! 2. Занятое имя не перезаписывается — берём `имя (2)`.
//! 3. Сначала строка в журнале, потом действие.

use crate::journal::{self, Entry, Journal, Writer};
use crate::plan::{free_name, without_copy_suffix, Disk, Plan};
use crate::platform;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Outcome {
    pub journal: Option<PathBuf>,
    pub moved: usize,
    pub failed: Vec<Failure>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Failure {
    pub path: PathBuf,
    pub why: String,
}

/// Есть ли что-то по пути. Битая ссылка тоже считается: её имя занято.
pub fn exists(p: &Path) -> bool {
    fs::symlink_metadata(p).is_ok()
}

/// Настоящий диск для плана.
pub struct RealDisk;

impl Disk for RealDisk {
    fn exists(&self, path: &Path) -> bool {
        exists(path)
    }

    fn files_in(&self, dir: &Path) -> Vec<(PathBuf, u64)> {
        let Ok(rd) = fs::read_dir(dir) else { return vec![] };
        rd.flatten()
            .filter_map(|e| {
                let meta = fs::symlink_metadata(e.path()).ok()?;
                (meta.is_file() && !platform::in_cloud(&meta)).then(|| (e.path(), meta.len()))
            })
            .collect()
    }

    fn same_content(&self, a: &Path, b: &Path) -> bool {
        same_bytes(a, b).unwrap_or(false)
    }
}

/// Сравнение байт в байт, кусками по 64 КБ. Хеш не нужен: сравниваем только файлы
/// одного размера, и прямое сравнение точнее любого хеша. Облачные файлы не читаем.
fn same_bytes(a: &Path, b: &Path) -> io::Result<bool> {
    let (ma, mb) = (fs::metadata(a)?, fs::metadata(b)?);
    if ma.len() != mb.len() || platform::in_cloud(&ma) || platform::in_cloud(&mb) {
        return Ok(false);
    }
    let (mut fa, mut fb) = (fs::File::open(a)?, fs::File::open(b)?);
    let (mut ba, mut bb) = (vec![0u8; 64 * 1024], vec![0u8; 64 * 1024]);
    loop {
        let n = fa.read(&mut ba)?;
        if n == 0 {
            return Ok(true);
        }
        fb.read_exact(&mut bb[..n])?;
        if ba[..n] != bb[..n] {
            return Ok(false);
        }
    }
}

fn explain(e: &io::Error) -> String {
    match e.kind() {
        io::ErrorKind::CrossesDevices => "лежит на другом диске — не переношу, чтобы не копировать".into(),
        io::ErrorKind::PermissionDenied => "нет доступа — файл открыт или защищён".into(),
        io::ErrorKind::NotFound => "файл пропал, пока раскладывали".into(),
        io::ErrorKind::StorageFull => "на диске нет места".into(),
        _ => e.to_string(),
    }
}

/// Создаёт недостающие папки и записывает каждую — чтобы откат убрал их, если опустеют.
fn make_dirs(dir: &Path, w: &mut Writer) -> io::Result<()> {
    let missing: Vec<&Path> = dir.ancestors().take_while(|d| !exists(d)).collect();
    for d in missing.into_iter().rev() {
        w.write(&Entry::Mkdir { path: d.to_path_buf() })?;
        fs::create_dir(d)?;
    }
    Ok(())
}

/// Раскладывает по плану. `progress(сделано, всего)` — для полосы на экране.
pub fn apply(plan: &Plan, journal: &Journal, mut progress: impl FnMut(usize, usize)) -> io::Result<Outcome> {
    let mut out = Outcome::default();
    if plan.moves.is_empty() {
        return Ok(out);
    }
    let _lock = journal.lock()?;
    let mut w = journal.begin(&plan.folders)?;
    out.journal = Some(w.path.clone());
    let total = plan.moves.len();

    for (id, m) in plan.moves.iter().enumerate() {
        progress(id, total);
        let fail = |why: String| Failure { path: m.src.clone(), why };
        // План мог устареть: файл убрали руками, пока человек смотрел на экран.
        if !exists(&m.src) {
            out.failed.push(fail("файла уже нет на месте".into()));
            continue;
        }
        let Some(parent) = m.dst.parent() else { continue };
        if let Err(e) = make_dirs(parent, &mut w) {
            out.failed.push(fail(format!("не получилось создать папку: {}", explain(&e))));
            continue;
        }
        // Имя могли занять после плана — ищем свободное заново, от чистого имени.
        let dst = if exists(&m.dst) {
            let name = m.dst.file_name().unwrap_or_default().to_string_lossy();
            let base = without_copy_suffix(&name, m.is_dir).unwrap_or_else(|| name.to_string());
            free_name(&parent.join(base), m.is_dir, &exists)
        } else {
            m.dst.clone()
        };
        w.write(&Entry::Intent { id, src: m.src.clone(), dst: dst.clone() })?;
        match fs::rename(&m.src, &dst) {
            Ok(()) => {
                w.write(&Entry::Done { id })?;
                out.moved += 1;
            }
            Err(e) => out.failed.push(fail(explain(&e))),
        }
    }
    progress(total, total);
    Ok(out)
}

/// Возвращает всё по журналу, с конца к началу. Повторный запуск безопасен:
/// то, что уже вернулось, просто пропускается.
pub fn undo(journal: &Journal, record: &Path) -> io::Result<Outcome> {
    let _lock = journal.lock()?;
    let entries = journal::read(record)?;
    let done: HashSet<usize> = entries.iter().filter_map(|e| if let Entry::Done { id } = e { Some(*id) } else { None }).collect();
    let mut out = Outcome { journal: Some(record.to_path_buf()), ..Default::default() };

    for e in entries.iter().rev() {
        match e {
            Entry::Intent { id, src, dst } => {
                let (at_dst, at_src) = (exists(dst), exists(src));
                // Без отметки «done» — сбой между намерением и делом. Решаем по диску.
                let happened = done.contains(id) || (at_dst && !at_src);
                if !happened || (!at_dst && at_src) {
                    continue; // не переезжал или уже вернулся
                }
                let fail = |why: &str| Failure { path: dst.clone(), why: why.into() };
                if !at_dst {
                    out.failed.push(fail("файла нет там, куда его положили — его переместили вручную"));
                } else if at_src {
                    out.failed.push(fail("на старом месте уже лежит другой файл с таким именем"));
                } else {
                    if let Some(p) = src.parent() {
                        fs::create_dir_all(p)?;
                    }
                    match fs::rename(dst, src) {
                        Ok(()) => out.moved += 1,
                        Err(err) => out.failed.push(fail(&explain(&err))),
                    }
                }
            }
            Entry::Mkdir { path } => remove_if_empty(path),
            Entry::Start { .. } | Entry::Done { .. } => {}
        }
    }
    if out.failed.is_empty() {
        journal.mark_undone(record)?;
    }
    Ok(out)
}

/// Убирает папку, только если в ней пусто (служебный `.DS_Store` не в счёт).
/// `remove_dir` в принципе не умеет удалять непустое — чужое не пострадает.
fn remove_if_empty(dir: &Path) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let names: Vec<_> = rd.flatten().map(|e| e.file_name()).collect();
    if names.iter().all(|n| n == ".DS_Store") {
        if !names.is_empty() {
            let _ = fs::remove_file(dir.join(".DS_Store"));
        }
        let _ = fs::remove_dir(dir);
    }
}
