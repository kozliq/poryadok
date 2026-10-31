//! Журнал раскладки — по принципу журнала базы данных (write-ahead log).
//!
//! Перед каждым переносом пишем намерение (`intent`), после — отметку (`done`),
//! и каждую строку сразу сбрасываем на диск. Если компьютер выключится между
//! этими двумя строками, откат посмотрит на диск и сам поймёт, успел ли файл
//! переехать. Поэтому вернуть всё на место можно даже после сбоя.
//!
//! Формат — JSON Lines: одна строка — одно событие. Его можно прочитать глазами.

use crate::civil;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

pub const FORMAT_VERSION: u32 = 1;
const UNDONE_SUFFIX: &str = ".undone";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum Entry {
    Start { version: u32, folders: Vec<PathBuf>, when: i64 },
    Mkdir { path: PathBuf },
    Intent { id: usize, src: PathBuf, dst: PathBuf },
    Done { id: usize },
}

/// Папка со всеми журналами.
#[derive(Debug, Clone)]
pub struct Journal {
    dir: PathBuf,
}

/// Одна прошлая раскладка.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub path: PathBuf,
    pub folders: Vec<PathBuf>,
    pub when: i64,
    pub moves: usize,
    pub undone: bool,
}

/// Пока жив — другая раскладка не начнётся. Замок снимает сама система,
/// даже если программа упала, поэтому «залипнуть» он не может.
#[derive(Debug)]
pub struct Lock {
    _file: File,
}

impl Journal {
    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Journal { dir: dir.into() }
    }

    /// `~/Library/Application Support/poryadok/journal` на Маке,
    /// `%APPDATA%\poryadok\journal` на Windows.
    pub fn system() -> Self {
        Journal::at(crate::data_dir().join("journal"))
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn lock(&self) -> io::Result<Lock> {
        fs::create_dir_all(&self.dir)?;
        let file = OpenOptions::new().create(true).truncate(false).write(true).open(self.dir.join(".lock"))?;
        match file.try_lock() {
            Ok(()) => Ok(Lock { _file: file }),
            Err(TryLockError::WouldBlock) => Err(io::Error::new(io::ErrorKind::WouldBlock, "уже идёт другая раскладка")),
            Err(TryLockError::Error(e)) => Err(e),
        }
    }

    pub(crate) fn begin(&self, folders: &[PathBuf]) -> io::Result<Writer> {
        fs::create_dir_all(&self.dir)?;
        let when = civil::now();
        let base = civil::stamp(when);
        let mut n = 0;
        let (path, file) = loop {
            let name = if n == 0 { format!("{base}.jsonl") } else { format!("{base}-{n}.jsonl") };
            let path = self.dir.join(name);
            match OpenOptions::new().create_new(true).append(true).open(&path) {
                Ok(file) => break (path, file),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => n += 1,
                Err(e) => return Err(e),
            }
        };
        let mut w = Writer { file, path };
        w.write(&Entry::Start { version: FORMAT_VERSION, folders: folders.to_vec(), when })?;
        Ok(w)
    }

    /// Все раскладки, свежие сверху.
    pub fn history(&self) -> Vec<Record> {
        let Ok(rd) = fs::read_dir(&self.dir) else { return vec![] };
        let mut list: Vec<Record> = rd.flatten().filter_map(|e| record(&e.path())).collect();
        list.sort_by(|a, b| b.when.cmp(&a.when).then_with(|| b.path.cmp(&a.path)));
        list
    }

    /// Последняя раскладка, которую ещё можно отменить.
    pub fn last_active(&self) -> Option<Record> {
        self.history().into_iter().find(|r| !r.undone && r.moves > 0)
    }

    pub(crate) fn mark_undone(&self, path: &Path) -> io::Result<()> {
        let mut to = path.as_os_str().to_owned();
        to.push(UNDONE_SUFFIX);
        fs::rename(path, to)
    }
}

pub(crate) struct Writer {
    file: File,
    pub(crate) path: PathBuf,
}

impl Writer {
    /// Строка за строкой, и каждая — сразу на диск.
    pub(crate) fn write(&mut self, e: &Entry) -> io::Result<()> {
        let mut line = serde_json::to_string(e)?;
        line.push('\n');
        self.file.write_all(line.as_bytes())?;
        self.file.sync_data()
    }
}

/// Читает журнал. Оборванную последнюю строку (сбой посреди записи) пропускает.
pub fn read(path: &Path) -> io::Result<Vec<Entry>> {
    let reader = BufReader::new(File::open(path)?);
    Ok(reader.lines().map_while(Result::ok).filter_map(|l| serde_json::from_str(&l).ok()).collect())
}

fn record(path: &Path) -> Option<Record> {
    let name = path.file_name()?.to_string_lossy();
    let undone = name.ends_with(UNDONE_SUFFIX);
    if !name.ends_with(".jsonl") && !undone {
        return None;
    }
    let entries = read(path).ok()?;
    let Some(Entry::Start { folders, when, .. }) = entries.first() else { return None };
    let moves = entries.iter().filter(|e| matches!(e, Entry::Done { .. })).count();
    Some(Record { path: path.to_path_buf(), folders: folders.clone(), when: *when, moves, undone })
}
