//! Эталон: как настоящий Рабочий стол был разобран вручную.
//!
//! 31.10.2026 настоящий Рабочий стол разобрали руками, шаг за шагом. Каждое действие
//! записано в `reference.jsonl` (имена личных проектов и клиентов заменены):
//! чем смотрели (`tools`), что заметили (`features`), куда положили (`done`) и почему (`why`).
//! Записаны только наблюдаемые действия и проговорённые причины.
//!
//! Тест воссоздаёт тот же стол в песочнице, прогоняет алгоритм и сверяет решения.
//! `personal: true` — решение опиралось на знание о проектах и клиентах человека;
//! общему алгоритму для таких случаев нужны свои правила, поэтому в долю совпадений они не входят.

use poryadok::scan::{self, Guard, GuardKind, ScanOptions};
use poryadok::{plan, Options, Places, RealDisk, RuleSet};
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

#[derive(Deserialize)]
struct Action {
    item: String,
    kind: String,
    #[serde(default)]
    inside: Vec<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    guard: Option<String>,
    tools: Vec<String>,
    features: Vec<String>,
    done: String,
    category: String,
    why: String,
    #[serde(default)]
    personal: bool,
}

#[test]
fn algorithm_repeats_manual_decisions() {
    let actions: Vec<Action> = include_str!("reference.jsonl").lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    let base = std::env::temp_dir().join(format!("poryadok-reference-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let places = Places::under(&base.join("дом"));
    let desk = places.desktop.clone();
    fs::create_dir_all(&desk).unwrap();

    // Воссоздаём стол: у каждой вещи своё содержимое, чтобы не было ложных дублей.
    for (n, a) in actions.iter().enumerate() {
        assert!(!a.tools.is_empty() && !a.features.is_empty() && !a.why.is_empty(), "{}: запись неполная", a.item);
        let path = desk.join(&a.item);
        match a.kind.as_str() {
            "file" | "script" => fs::write(&path, a.text.clone().unwrap_or_else(|| format!("файл {n}"))).unwrap(),
            "alias" => fs::write(&path, b"book\0\0\0\0mark\0\0\0\0").unwrap(),
            "folder" => {
                fs::create_dir_all(&path).unwrap();
                for (k, inner) in a.inside.iter().enumerate() {
                    let p = path.join(inner);
                    fs::create_dir_all(p.parent().unwrap()).unwrap();
                    fs::write(p, format!("{n}-{k}")).unwrap();
                }
            }
            other => panic!("неизвестный вид {other}"),
        }
    }

    let rules = RuleSet::default();
    let mut snap = scan::scan(std::slice::from_ref(&desk), &places.home, ScanOptions { look_inside: true, ask_system: false, read_text: Some(&rules) }).unwrap();
    snap.taken_at += 3600;
    // То, что на настоящем Маке говорит система, здесь подставляем сами.
    for a in &actions {
        let why = match a.guard.as_deref() {
            Some(g) if g.starts_with("busy:") => GuardKind::Busy(g[5..].into()),
            Some(g) if g.starts_with("app:") => GuardKind::AppFolder(g[4..].into()),
            Some("screenshots") => GuardKind::Screenshots,
            Some(other) => panic!("неизвестная защита {other}"),
            None => continue,
        };
        let path: PathBuf = if a.guard.as_deref().is_some_and(|g| g.starts_with("busy:")) { desk.join(&a.item).join("tekst") } else { desk.join(&a.item) };
        snap.guards.push(Guard { path, why });
    }
    let plan = plan(&snap, &rules, &places, &Options { folders: true, ..Default::default() }, &RealDisk);

    let decision = |item: &str| -> String {
        if let Some(m) = plan.moves.iter().find(|m| m.src.file_name().is_some_and(|n| n == item)) {
            return m.rule.clone();
        }
        let s = plan.skipped.iter().find(|s| s.path.file_name().is_some_and(|n| n == item)).unwrap_or_else(|| panic!("{item} пропал из плана"));
        format!("оставить:{}", serde_json::to_value(&s.reason).unwrap()["code"].as_str().unwrap())
    };

    let mut wrong = Vec::new();
    let (mut shared, mut agreed) = (0, 0);
    println!("\n{:<44} {:<24} вручную", "вещь", "алгоритм");
    for a in &actions {
        let got = decision(&a.item);
        let ok = got == a.category;
        println!("{:<44} {:<24} {}{}", a.item.chars().take(42).collect::<String>(), got, a.done, if a.personal { "  (личное)" } else { "" });
        if !a.personal {
            shared += 1;
            agreed += usize::from(ok);
        }
        if !ok {
            wrong.push(format!("{}: ждали «{}», получили «{got}»", a.item, a.category));
        }
    }
    println!("\nСовпало с ручной разборкой: {agreed} из {shared} общих решений; ещё {} опирались на знание о проектах человека.", actions.len() - shared);
    let _ = fs::remove_dir_all(&base);
    assert!(wrong.is_empty(), "расхождения:\n{}", wrong.join("\n"));
}
