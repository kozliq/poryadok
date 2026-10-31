//! Команда `poryadok`: то же, что окно, но в терминале. Удобно для скриптов и проверки.

use poryadok::{apply, export_rules, human_size, load_rules, plan_folders, plural, undo, Journal, Options, Places, Plan};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

const HELP: &str = "Порядок — раскладывает файлы по полкам. Ничего не удаляет, всё можно вернуть.

  poryadok план [папки…]       показать, что куда поедет (по умолчанию — Рабочий стол)
  poryadok план всё            Рабочий стол, Загрузки и Документы сразу
  poryadok разложить [папки…]  разложить по-настоящему (сначала спросит)
  poryadok почему <файл>       куда бы лёг этот файл и почему (с текстом, который удалось прочитать)
  poryadok вернуть             вернуть последнюю раскладку как было
  poryadok история             все прошлые раскладки
  poryadok правила             файл со своими правилами (создаст, если нет)

Флаги:
  --папки       двигать и папки целиком, если внутри почти всё одного вида
  --только-имена  решать по названиям, не читая файлы
  --да          не спрашивать подтверждение
  --json        план в JSON — для других программ
  --в <папка>   раскладывать не по домашним папкам, а внутри этой

По-английски тоже можно: plan, apply, undo, history, rules, --folders, --yes, --to.";

struct Args {
    command: String,
    folders: Vec<PathBuf>,
    /// Двигать папки целиком.
    whole: bool,
    /// Не читать текст файлов.
    names_only: bool,
    yes: bool,
    json: bool,
    to: Option<PathBuf>,
}

fn parse() -> Args {
    let mut a = Args { command: "помощь".into(), folders: vec![], whole: false, names_only: false, yes: false, json: false, to: None };
    let mut words = Vec::new();
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--папки" | "--folders" => a.whole = true,
            "--только-имена" | "--names-only" => a.names_only = true,
            "--да" | "--yes" | "-y" => a.yes = true,
            "--json" => a.json = true,
            "--в" | "--to" => a.to = it.next().map(PathBuf::from),
            _ => words.push(arg),
        }
    }
    let mut words = words.into_iter();
    if let Some(c) = words.next() {
        a.command = c;
    }
    let system = Places::system();
    for w in words {
        match w.as_str() {
            "всё" | "все" | "all" => a.folders.extend([system.desktop.clone(), system.downloads.clone(), system.documents.clone()]),
            _ => a.folders.push(PathBuf::from(w)),
        }
    }
    if a.folders.is_empty() {
        a.folders.push(system.desktop);
    }
    a
}

fn main() -> ExitCode {
    let args = parse();
    let places = args.to.as_deref().map(Places::under).unwrap_or_else(Places::system);
    let journal = Journal::system();
    let result = match args.command.as_str() {
        "план" | "plan" => show_plan(&args, &places).map(|_| ()),
        "разложить" | "apply" => sort(&args, &places, &journal),
        "вернуть" | "отменить" | "undo" => revert(&journal),
        "почему" | "why" => explain(&args),
        "история" | "history" => {
            history(&journal);
            Ok(())
        }
        "правила" | "rules" => export_rules().map(|p| println!("{}", p.display())).map_err(|e| e.to_string()),
        _ => {
            println!("{HELP}");
            Ok(())
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Не вышло: {e}");
            ExitCode::FAILURE
        }
    }
}

fn show_plan(args: &Args, places: &Places) -> Result<Plan, String> {
    let rules = load_rules()?;
    let opts = Options { folders: args.whole, names_only: args.names_only, ..Default::default() };
    let plan = plan_folders(&args.folders, &rules, places, &opts).map_err(|e| e.to_string())?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&plan).map_err(|e| e.to_string())?);
    } else {
        print_plan(&plan, places);
    }
    Ok(plan)
}

fn sort(args: &Args, places: &Places, journal: &Journal) -> Result<(), String> {
    let plan = show_plan(args, places)?;
    if plan.moves.is_empty() {
        return Ok(());
    }
    if !args.yes {
        eprint!("\nРазложить {}? Напиши «да»: ", plural(plan.moves.len(), "вещь", "вещи", "вещей"));
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer).map_err(|e| e.to_string())?;
        if !matches!(answer.trim().to_lowercase().as_str(), "да" | "д" | "yes" | "y") {
            println!("Ничего не трогал.");
            return Ok(());
        }
    }
    let outcome = apply(&plan, journal, |done, total| {
        eprint!("\r  {done}/{total}");
        let _ = std::io::stderr().flush();
    })
    .map_err(|e| e.to_string())?;
    eprintln!();
    println!("Разложено: {}.", outcome.moved);
    for f in &outcome.failed {
        println!("  не вышло: {} — {}", f.path.display(), f.why);
    }
    println!("Передумал? poryadok вернуть");
    Ok(())
}

fn explain(args: &Args) -> Result<(), String> {
    let rules = load_rules()?;
    for path in &args.folders {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let ext = poryadok::scan::extension(&name);
        let started = std::time::Instant::now();
        let text = rules.wants_text(&name).then(|| poryadok::content::read(path, &ext)).flatten();
        let took = started.elapsed();
        println!("{name}");
        match rules.explain(&name, text.as_deref()) {
            Some((i, evidence)) => {
                let r = rules.get(i);
                println!("  → {} / {}   ({evidence})", r.place.label(), r.path.replace('/', " / "));
            }
            None => println!("  → не знаю, куда это"),
        }
        if let Some(t) = &text {
            let short: String = t.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(240).collect();
            println!("  текст ({} мс): {short}", took.as_millis());
        }
    }
    Ok(())
}

fn revert(journal: &Journal) -> Result<(), String> {
    let Some(last) = journal.last_active() else {
        println!("Возвращать нечего.");
        return Ok(());
    };
    let outcome = undo(journal, &last.path).map_err(|e| e.to_string())?;
    println!("Вернул на место: {}.", outcome.moved);
    for f in &outcome.failed {
        println!("  не вышло: {} — {}", f.path.display(), f.why);
    }
    Ok(())
}

fn history(journal: &Journal) {
    let list = journal.history();
    if list.is_empty() {
        println!("Раскладок ещё не было.");
    }
    for r in list {
        let c = poryadok::civil::from_unix(r.when);
        let mark = if r.undone { "  — возвращено" } else { "" };
        let folders: Vec<String> = r.folders.iter().map(|f| f.display().to_string()).collect();
        println!("{:02}.{:02}.{} {:02}:{:02} UTC  {}  {}{mark}", c.day, c.month, c.year, c.hour, c.minute, folders.join(" + "), r.moves);
    }
}

fn print_plan(plan: &Plan, places: &Places) {
    let mut shelves: BTreeMap<String, Vec<&poryadok::Move>> = BTreeMap::new();
    for m in &plan.moves {
        shelves.entry(places.describe(m.dst.parent().unwrap_or(&m.dst))).or_default().push(m);
    }
    let folders: Vec<String> = plan.folders.iter().map(|f| places.describe(f)).collect();
    println!("{}\n", folders.join(" + "));
    for (shelf, items) in &shelves {
        let size: u64 = items.iter().map(|m| m.size).sum();
        println!("→ {shelf}   {}, {}", plural(items.len(), "вещь", "вещи", "вещей"), human_size(size));
        for m in items {
            let cloud = if m.downloads { "   ☁ скачается из облака" } else { "" };
            let dir = if m.is_dir { "   (папка целиком)" } else { "" };
            let twin = m.twin.as_ref().map(|t| format!("   = {}", places.describe(t))).unwrap_or_default();
            let renamed = if m.dst.file_name() != m.src.file_name() {
                format!("   → «{}»", m.dst.file_name().unwrap_or_default().to_string_lossy())
            } else {
                String::new()
            };
            println!("    {}{dir}{renamed}{twin}{cloud}", m.src.file_name().unwrap_or_default().to_string_lossy());
        }
    }
    if !plan.skipped.is_empty() {
        println!("\nОставляю на месте:");
        for s in &plan.skipped {
            println!("    {} — {}", s.path.file_name().unwrap_or_default().to_string_lossy(), s.reason);
        }
    }
    if plan.download_size() > 0 {
        println!("\n☁ Из облака придётся скачать {}.", human_size(plan.download_size()));
    }
    if plan.moves.is_empty() {
        println!("\nРаскладывать нечего — тут уже порядок.");
    }
}
