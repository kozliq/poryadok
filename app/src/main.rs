//! Окно-визитка. Вся логика — в ядре `poryadok`; здесь только мост к экрану:
//! команды, которые зовёт интерфейс, и размер окна по формуле из `poryadok::card`.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use poryadok::card::{self, Rect};
use poryadok::{human_size, plural, Journal, Options, Place, Places, Plan};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{Emitter, LogicalPosition, LogicalSize, Manager, State};

struct AppState {
    places: Places,
    journal: Journal,
    /// Последний показанный план. Раскладываем ровно то, что человек увидел.
    plan: Mutex<Option<Plan>>,
}

// ---------- То, что видит экран ----------

#[derive(Serialize)]
struct Choice {
    label: &'static str,
    paths: Vec<PathBuf>,
}

#[derive(Serialize)]
struct Start {
    choices: Vec<Choice>,
    last: Option<LastRun>,
    rules_error: Option<String>,
}

#[derive(Serialize)]
struct LastRun {
    folder: String,
    count: String,
}

#[derive(Serialize)]
struct ShelfView {
    title: String,
    place: String,
    count: usize,
    size: String,
    items: Vec<String>,
}

#[derive(Serialize)]
struct SkipView {
    name: String,
    reason: String,
}

#[derive(Serialize)]
struct PlanView {
    folder: String,
    total: String,
    size: String,
    count: usize,
    /// «Из облака скачается 1,2 ГБ» — или пусто.
    cloud: Option<String>,
    shelves: Vec<ShelfView>,
    skipped: Vec<SkipView>,
}

#[derive(Serialize)]
struct DoneView {
    moved: usize,
    text: String,
    failed: Vec<SkipView>,
    can_undo: bool,
}

#[derive(Serialize, Clone)]
struct Progress {
    done: usize,
    total: usize,
}

fn things(n: usize) -> String {
    plural(n, "вещь", "вещи", "вещей")
}

fn name_of(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string())
}

fn plan_view(plan: &Plan, places: &Places) -> PlanView {
    // Полка = правило: скриншоты разных месяцев показываем одной строкой.
    let mut shelves: BTreeMap<String, ShelfView> = BTreeMap::new();
    let mut sizes: BTreeMap<String, u64> = BTreeMap::new();
    for m in &plan.moves {
        let shelf = shelves.entry(m.rule.clone()).or_insert_with(|| {
            let base = if m.dated { m.shelf.rsplit_once('/').map(|(b, _)| b).unwrap_or(&m.shelf) } else { &m.shelf };
            let mut place = std::iter::once(m.place.label()).chain(base.split('/')).collect::<Vec<_>>().join(" / ");
            if m.dated {
                place.push_str(" · по месяцам");
            }
            ShelfView { title: m.rule.clone(), place, count: 0, size: String::new(), items: vec![] }
        });
        shelf.count += 1;
        let mut line = name_of(&m.src);
        if m.is_dir {
            line.push_str(" — папкой");
        }
        if m.dst.file_name() != m.src.file_name() {
            line.push_str(&format!(" → «{}»", name_of(&m.dst)));
        }
        match &m.twin {
            Some(twin) => line.push_str(&format!(" — копия: {}", places.describe(twin))),
            None => line.push_str(&format!(" — {}", m.why)),
        }
        shelf.items.push(line);
        *sizes.entry(m.rule.clone()).or_default() += m.size;
    }
    let mut shelves: Vec<ShelfView> = shelves
        .into_values()
        .map(|mut s| {
            s.size = human_size(sizes[&s.title]);
            s
        })
        .collect();
    shelves.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.title.cmp(&b.title)));
    let downloads = plan.download_size();
    PlanView {
        folder: plan.folders.iter().map(|f| places.describe(f)).collect::<Vec<_>>().join(" + "),
        total: things(plan.moves.len()),
        size: human_size(plan.total_size()),
        count: plan.moves.len(),
        cloud: (downloads > 0).then(|| format!("Из облака скачается {} — эти файлы уезжают из папок iCloud.", human_size(downloads))),
        shelves,
        skipped: plan.skipped.iter().map(|s| SkipView { name: name_of(&s.path), reason: s.reason.to_string() }).collect(),
    }
}

fn failures(list: &[poryadok::Failure]) -> Vec<SkipView> {
    list.iter().map(|f| SkipView { name: name_of(&f.path), reason: f.why.clone() }).collect()
}

// ---------- Команды ----------

#[tauri::command]
fn start(state: State<AppState>) -> Start {
    let p = &state.places;
    let choice = |place: Place| Choice { label: place.label(), paths: vec![p.get(place).to_path_buf()] };
    let everything = Choice { label: "Всё сразу", paths: vec![p.desktop.clone(), p.downloads.clone(), p.documents.clone()] };
    Start {
        choices: vec![choice(Place::Desktop), choice(Place::Downloads), choice(Place::Documents), everything],
        last: state.journal.last_active().map(|r| LastRun {
            folder: r.folders.iter().map(|f| p.describe(f)).collect::<Vec<_>>().join(" + "),
            count: things(r.moves),
        }),
        rules_error: poryadok::load_rules().err(),
    }
}

#[tauri::command]
async fn make_plan(paths: Vec<PathBuf>, folders: bool, state: State<'_, AppState>) -> Result<PlanView, String> {
    if paths.is_empty() || paths.iter().any(|p| !p.is_dir()) {
        return Err("Это не папка. Перетащите сюда папку целиком.".into());
    }
    let rules = poryadok::load_rules()?;
    let places = state.places.clone();
    let opts = Options { folders, ..Default::default() };
    let plan = tauri::async_runtime::spawn_blocking(move || poryadok::plan_folders(&paths, &rules, &places, &opts))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("Не открывается папка: {e}"))?;
    let view = plan_view(&plan, &state.places);
    *state.plan.lock().unwrap() = Some(plan);
    Ok(view)
}

#[tauri::command]
async fn apply_plan(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<DoneView, String> {
    let plan = state.plan.lock().unwrap().take().ok_or("Сначала посмотрите план.")?;
    let journal = state.journal.clone();
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        poryadok::apply(&plan, &journal, |done, total| {
            let _ = app.emit("progress", Progress { done, total });
        })
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    Ok(DoneView {
        moved: outcome.moved,
        text: format!("Разложено: {}.", things(outcome.moved)),
        failed: failures(&outcome.failed),
        can_undo: outcome.moved > 0,
    })
}

#[tauri::command]
async fn undo_last(state: State<'_, AppState>) -> Result<DoneView, String> {
    let journal = state.journal.clone();
    let last = journal.last_active().ok_or("Возвращать нечего.")?;
    let outcome = tauri::async_runtime::spawn_blocking(move || poryadok::undo(&journal, &last.path))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    Ok(DoneView {
        moved: outcome.moved,
        text: format!("Вернул на место: {}.", things(outcome.moved)),
        can_undo: !outcome.failed.is_empty(),
        failed: failures(&outcome.failed),
    })
}

/// Показать папку в Finder или Проводнике.
#[tauri::command]
fn reveal(path: PathBuf) -> Result<(), String> {
    open_with_system(&path, true)
}

#[tauri::command]
fn open_rules() -> Result<(), String> {
    let path = poryadok::export_rules().map_err(|e| e.to_string())?;
    open_with_system(&path, false)
}

fn open_with_system(path: &Path, select: bool) -> Result<(), String> {
    let mut cmd = if cfg!(target_os = "macos") {
        let mut c = std::process::Command::new("open");
        c.arg(if select { "-R" } else { "-t" }).arg(path);
        c
    } else if cfg!(windows) {
        let mut c = std::process::Command::new("explorer");
        if select {
            c.arg(format!("/select,{}", path.display()));
        } else {
            c.arg(path);
        }
        c
    } else {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(if select { path.parent().unwrap_or(path) } else { path });
        c
    };
    cmd.spawn().map(|_| ()).map_err(|e| e.to_string())
}

// ---------- Окно по формуле ----------

fn place_card(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    let Some(monitor) = window.current_monitor()?.or(window.primary_monitor()?) else { return Ok(()) };
    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    let card = card::window_for(Rect {
        x: area.position.x as f64 / scale,
        y: area.position.y as f64 / scale,
        width: area.size.width as f64 / scale,
        height: area.size.height as f64 / scale,
    });
    window.set_size(LogicalSize::new(card.width, card.height))?;
    window.set_position(LogicalPosition::new(card.x, card.y))?;
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .manage(AppState { places: Places::system(), journal: Journal::system(), plan: Mutex::new(None) })
        .invoke_handler(tauri::generate_handler![start, make_plan, apply_plan, undo_last, reveal, open_rules])
        .setup(|app| {
            let window = app.get_webview_window("main").expect("окно main описано в tauri.conf.json");
            place_card(&window)?;
            window.show()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("не удалось запустить окно");
}
