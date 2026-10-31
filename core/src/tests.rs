//! Тесты ядра. Каждый тест получает свою песочницу: свой «дом», свой журнал.
//! Глобального состояния нет, поэтому тесты идут параллельно.

use super::*;
use crate::journal::Entry;
use crate::rules::Evidence;
use crate::scan::{Guard, GuardKind, Item, Kind, ScanOptions};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write as _;

struct Sandbox {
    base: PathBuf,
    places: Places,
    journal: Journal,
}

impl Sandbox {
    fn new(name: &str) -> Self {
        let base = std::env::temp_dir().join(format!("poryadok-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let places = Places::under(&base.join("дом"));
        fs::create_dir_all(&places.desktop).unwrap();
        fs::create_dir_all(&places.downloads).unwrap();
        Sandbox { journal: Journal::at(base.join("журнал")), places, base }
    }
    fn desk(&self) -> &Path {
        &self.places.desktop
    }
    fn put(&self, rel: &str, body: &[u8]) {
        let p = self.desk().join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, body).unwrap();
    }
    /// Снимок без вопросов к системе: результат зависит только от песочницы.
    fn snap(&self, folders: &[PathBuf], look_inside: bool) -> Snapshot {
        self.snap_with(folders, ScanOptions { look_inside, ..Default::default() })
    }
    fn snap_with(&self, folders: &[PathBuf], opts: ScanOptions) -> Snapshot {
        let mut snap = scan::scan(folders, &self.places.home, opts).unwrap();
        // Файлы в тестах «только что появились»; смотрим на них как будто через час.
        snap.taken_at += 3600;
        snap
    }
    fn plan_of(&self, folders: &[PathBuf], with_folders: bool) -> Plan {
        let snap = self.snap(folders, with_folders);
        plan::plan(&snap, &RuleSet::default(), &self.places, &Options { folders: with_folders, ..Default::default() }, &RealDisk)
    }
    fn plan(&self, with_folders: bool) -> Plan {
        self.plan_of(&[self.desk().to_path_buf()], with_folders)
    }
    /// Всё дерево «дома»: путь → содержимое (у папок — пусто). Для сравнения «до» и «после».
    fn tree(&self) -> BTreeMap<String, Option<Vec<u8>>> {
        let mut out = BTreeMap::new();
        let mut stack = vec![self.places.home.clone()];
        while let Some(d) = stack.pop() {
            for e in fs::read_dir(&d).unwrap().flatten() {
                if e.file_name() == ".DS_Store" {
                    continue;
                }
                let rel = e.path().strip_prefix(&self.places.home).unwrap().to_string_lossy().into_owned();
                if e.path().is_dir() {
                    stack.push(e.path());
                    out.insert(rel, None);
                } else {
                    out.insert(rel, Some(fs::read(e.path()).unwrap()));
                }
            }
        }
        out
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

fn shelf_of(name: &str) -> Option<String> {
    RuleSet::default().rule_for(name).map(|r| r.name.clone())
}

fn why(p: &Plan, name: &str) -> Option<Reason> {
    p.skipped.iter().find(|k| k.path.file_name().is_some_and(|n| n == name)).map(|k| k.reason.clone())
}

fn moved<'a>(p: &'a Plan, name: &str) -> &'a Move {
    p.moves.iter().find(|m| m.src.file_name().is_some_and(|n| n == name)).unwrap_or_else(|| panic!("{name} не едет"))
}

// ---------- Правила ----------

#[test]
fn words_beat_extensions() {
    let cases = [
        ("Снимок экрана 2026-10-30 в 12.34.56.png", Some("Скриншоты")),
        ("Screenshot 2026-01-01 at 10.00.00.png", Some("Скриншоты")),
        ("Запись экрана 2026-10-20 в 16.38.52.mov", Some("Записи экрана")),
        ("Рилс — без подписок.mp4", Some("Рилсы")),
        ("Договор_аренды.pdf", Some("Договоры и счета")),
        ("счёт-42.pdf", Some("Договоры и счета")),
        ("СЧЕТ 42.PDF", Some("Договоры и счета")),
        ("Сметы на ремонт.xlsx", Some("Сметы и предложения")),
        ("КП для кафе.pdf", Some("Сметы и предложения")),
        ("Иванов CV.pdf", Some("Резюме")),
        ("Скан паспорта.jpg", Some("Сканы документов")),
        ("Билет Омск — Москва.pdf", Some("Билеты")),
        ("IMG_2500.PNG", Some("С телефона")),
        ("photo_2026-10-24_22-14-19.jpg", Some("Из Telegram")),
        ("отпуск.jpg", Some("Картинки")),
        ("лекция.pdf", Some("PDF")),
        ("Mimo-TikTok-36.5.4.apk", Some("Установщики")),
        ("archive.tar.gz", Some("Архивы")),
        ("загадочный.xyz", None),
        ("без_расширения", None),
    ];
    for (name, want) in cases {
        assert_eq!(shelf_of(name).as_deref(), want, "{name}");
    }
}

#[test]
fn words_match_from_word_start() {
    assert_eq!(shelf_of("москанал.jpg").as_deref(), Some("Картинки"));
    assert_eq!(shelf_of("recv.pdf").as_deref(), Some("PDF"));
    assert_eq!(shelf_of("тест_скрины.png").as_deref(), Some("Скриншоты"));
}

#[test]
fn latin_spellings_and_glued_words() {
    // Человек пишет латиницей — правило всё равно узнаёт русское слово.
    assert_eq!(shelf_of("dogovor_arendy.pdf").as_deref(), Some("Договоры и счета"));
    assert_eq!(shelf_of("DogovorArendy.pdf").as_deref(), Some("Договоры и счета"));
    assert_eq!(shelf_of("schet-42.pdf").as_deref(), Some("Договоры и счета"));
    assert_eq!(shelf_of("kvitanciya_za_svet.pdf").as_deref(), Some("Договоры и счета"));
    assert_eq!(shelf_of("smeta_remont.xlsx").as_deref(), Some("Сметы и предложения"));
    assert_eq!(shelf_of("ScreenShot2026.png").as_deref(), Some("Скриншоты"));
    assert_eq!(shelf_of("rezyume_ivanov.pdf").as_deref(), Some("Резюме"));
    // Короткие слова латиницей не ищем: «инн» не должен ловить innovation.
    assert_eq!(shelf_of("innovation.pdf").as_deref(), Some("PDF"));
}

#[test]
fn spellings_cover_endings_and_schemes() {
    let s = rules::spellings("Квитанция", true);
    for want in ["квитанция", "квитанци", "kvitantsiya", "kvitancija", "kvitanci"] {
        assert!(s.contains(&want.to_string()), "{want} нет в {s:?}");
    }
    assert_eq!(rules::spellings("cv", true), ["cv"]);
}

#[test]
fn normalize_handles_decomposed_letters_and_case() {
    assert_eq!(rules::normalize("Мои\u{306}  Счёт__2026"), "мой счет 2026");
    assert_eq!(rules::normalize("Ёлка"), "елка");
    assert_eq!(rules::normalize("—a—"), "a");
    assert_eq!(rules::normalize("СЧЕТ"), "счет");
    assert_eq!(rules::normalize("myCodeRepo"), "my code repo");
}

#[test]
fn own_rules_load_from_json() {
    let json = r#"[{"name":"Кафе","place":"documents","path":"Клиенты/Кафе","keywords":["монплезир"]}]"#;
    let rules = RuleSet::new(serde_json::from_str(json).unwrap());
    // Клиент без расширений: любой файл с именем клиента, хоть по-русски, хоть транслитом.
    assert_eq!(rules.rule_for("Меню Монплезира.pdf").unwrap().path, "Клиенты/Кафе");
    assert_eq!(rules.rule_for("monplezir_logo.png").unwrap().path, "Клиенты/Кафе");
    assert!(rules.rule_for("отчёт.pdf").is_none());
}

// ---------- Мелочи ----------

#[test]
fn civil_dates() {
    let d = civil::from_unix(1_793_404_800);
    assert_eq!((d.year, d.month, d.day), (2026, 10, 31));
    let leap = civil::from_unix(951_782_400);
    assert_eq!((leap.year, leap.month, leap.day), (2000, 2, 29));
    assert_eq!(civil::stamp(0), "1970-01-01_00-00-00");
    assert_eq!(civil::month_folder(-1), "1969-12");
    assert_eq!(civil::date_ru(1_793_404_800), "31.10.2026");
}

#[test]
fn russian_plurals() {
    let f = |n| plural(n, "файл", "файла", "файлов");
    assert_eq!([f(1), f(2), f(5), f(11), f(12), f(21), f(104), f(111)], [
        "1 файл", "2 файла", "5 файлов", "11 файлов", "12 файлов", "21 файл", "104 файла", "111 файлов"
    ]);
    assert_eq!(human_size(1536), "1,5 КБ");
}

#[test]
fn places_describe_paths_humanly() {
    let p = Places::under(Path::new("/h"));
    assert_eq!(p.describe(Path::new("/h/Pictures/Скриншоты/2026-10")), "Изображения / Скриншоты / 2026-10");
    assert_eq!(p.describe(Path::new("/h/code")), "Дом / code");
    assert_eq!(p.describe(Path::new("/elsewhere")), "/elsewhere");
}

#[test]
fn free_name_counts_up_and_respects_folders() {
    let taken = |p: &Path| p.ends_with("фото.jpg") || p.ends_with("фото (2).jpg") || p.ends_with("Отпуск 2.0");
    assert!(plan::free_name(Path::new("/x/фото.jpg"), false, &taken).ends_with("фото (3).jpg"));
    assert!(plan::free_name(Path::new("/x/Отпуск 2.0"), true, &taken).ends_with("Отпуск 2.0 (2)"));
    assert!(plan::free_name(Path::new("/x/новое.jpg"), false, &taken).ends_with("новое.jpg"));
}

#[test]
fn copy_suffixes_and_unnamed_folders() {
    let clean = |n: &str| plan::without_copy_suffix(n, false);
    assert_eq!(clean("отчёт (1).pdf").as_deref(), Some("отчёт.pdf"));
    assert_eq!(clean("отчёт копия.pdf").as_deref(), Some("отчёт.pdf"));
    assert_eq!(clean("отчёт копия 2.pdf").as_deref(), Some("отчёт.pdf"));
    assert_eq!(clean("report - copy.docx").as_deref(), Some("report.docx"));
    assert_eq!(clean("Отчёт 2026 (финал).pdf"), None);
    assert_eq!(clean("(1).pdf"), None);
    assert_eq!(clean("отчёт.pdf"), None);
    for name in ["1", "23", "Новая папка", "Новая папка 3", "untitled folder", "New Folder 2", "Без названия"] {
        assert!(plan::is_unnamed(name), "{name}");
    }
    for name in ["2026", "Отпуск", "Папка Ивана"] {
        assert!(!plan::is_unnamed(name), "{name}");
    }
}

#[test]
fn finds_paths_in_scripts() {
    let home = Path::new("/Users/me");
    let text = r#"
        DIR="$HOME/Desktop/videos"
        cp a ~/Desktop/Мои\ файлы
        open "/Users/me/Desktop/Новая папка/x.txt"
        echo ${HOME}/Downloads/$NAME
        url=http://x.com/a~b/c
        set p=%USERPROFILE%\Desktop\out
    "#;
    let found = platform::find_paths(text, home);
    let want = ["/Users/me/Desktop/videos", "/Users/me/Desktop/Мои файлы", "/Users/me/Desktop/Новая папка/x.txt", "/Users/me/Downloads"];
    for w in want {
        assert!(found.contains(&PathBuf::from(w)), "{w} не найден в {found:?}");
    }
    assert!(found.iter().any(|p| p.ends_with("out")));
    assert!(!found.iter().any(|p| p.to_string_lossy().contains("x.com")));
}

#[test]
fn parses_lsof_output() {
    let out = "p101\nczsh\nfcwd\nn/Users/me/Desktop/\\xd0\\xa7\\xd0\\xb8\\xd1\\x81\\xd1\\x82\\xd0\\xbe/tekst\np202\ncvim\nfcwd\nn/Users/me\n";
    assert_eq!(platform::parse_lsof(out), vec![
        (PathBuf::from("/Users/me/Desktop/Чисто/tekst"), "zsh".to_string()),
        (PathBuf::from("/Users/me"), "vim".to_string()),
    ]);
}

// ---------- План ----------

#[test]
fn plan_explains_every_item() {
    let s = Sandbox::new("explain");
    s.put("фото.jpg", b"jpg");
    s.put("непонятно.xyz", b"?");
    s.put("ТТ.command", b"#!/bin/zsh");
    s.put("Код/package.json", b"{}");
    s.put("Отпуск/1.jpg", b"1");
    s.put("фильм.mp4.crdownload", b"...");
    fs::write(s.desk().join("ярлык"), b"book\0\0\0\0mark\0\0\0\0").unwrap();
    let p = s.plan(false);

    assert_eq!(p.moves.len(), 1);
    assert_eq!(why(&p, "непонятно.xyz"), Some(Reason::Unknown));
    assert_eq!(why(&p, "ТТ.command"), Some(Reason::Program));
    assert_eq!(why(&p, "ярлык"), Some(Reason::Link));
    assert_eq!(why(&p, "Код"), Some(Reason::Project("package.json".into())));
    assert_eq!(why(&p, "Отпуск"), Some(Reason::FoldersOff));
    assert_eq!(why(&p, "фильм.mp4.crdownload"), Some(Reason::Downloading));
    // Каждая вещь со стола либо едет, либо объяснена.
    assert_eq!(p.moves.len() + p.skipped.len(), 7);
}

#[test]
fn fresh_files_wait() {
    let s = Sandbox::new("fresh");
    s.put("только что.pdf", b"x");
    let snap = scan::scan(&[s.desk().to_path_buf()], &s.places.home, ScanOptions::default()).unwrap();
    let p = plan::plan(&snap, &RuleSet::default(), &s.places, &Options::default(), &RealDisk);
    assert_eq!(why(&p, "только что.pdf"), Some(Reason::Fresh));
}

#[test]
fn folders_move_whole_by_majority() {
    let s = Sandbox::new("majority");
    for i in 0..7 {
        s.put(&format!("Отпуск/{i}.jpg"), b"x");
    }
    s.put("Отпуск/заметка.txt", b"x");
    s.put("Смесь/a.pdf", b"x");
    s.put("Смесь/b.mp3", b"x");
    s.put("Глубоко/вложено/репо/.git/HEAD", b"x");
    fs::create_dir_all(s.desk().join("Пустая")).unwrap();
    let p = s.plan(true);

    assert_eq!(moved(&p, "Отпуск").dst, s.places.pictures.join("Картинки/Отпуск"));
    assert!(moved(&p, "Отпуск").is_dir);
    assert_eq!(moved(&p, "Пустая").dst, s.desk().join("На удаление/Пустые/Пустая"));
    assert_eq!(why(&p, "Смесь"), Some(Reason::Mixed));
    assert_eq!(why(&p, "Глубоко"), Some(Reason::Project(".git внутри".into())));
}

#[test]
fn unnamed_folder_gets_a_name() {
    let s = Sandbox::new("unnamed");
    for i in 0..3 {
        s.put(&format!("1/IMG_250{i}.PNG"), b"x");
    }
    let p = s.plan(true);
    let today = civil::date_ru(civil::now());
    assert_eq!(moved(&p, "1").dst, s.places.pictures.join(format!("С телефона/С телефона {today}")));
}

#[test]
fn screenshots_go_by_month() {
    let s = Sandbox::new("month");
    s.put("Снимок экрана 1.png", b"x");
    let p = s.plan(false);
    let month = civil::month_folder(civil::now());
    assert_eq!(p.moves[0].shelf, format!("Скриншоты/{month}"));
}

#[test]
fn copies_lose_their_suffix_when_free() {
    let s = Sandbox::new("suffix");
    s.put("отчёт (1).pdf", b"a");
    s.put("договор копия.pdf", b"b");
    let p = s.plan(false);
    assert!(moved(&p, "отчёт (1).pdf").dst.ends_with("PDF/отчёт.pdf"));
    assert!(moved(&p, "договор копия.pdf").dst.ends_with("Договоры и счета/договор.pdf"));
}

#[test]
fn empty_files_and_duplicates_go_to_review() {
    let s = Sandbox::new("dups");
    s.put("пустой.txt", b"");
    s.put("фото.jpg", b"same");
    s.put("a.pdf", b"twin");
    s.put("a копия.pdf", b"twin");
    let shelf = s.places.pictures.join("Картинки");
    fs::create_dir_all(&shelf).unwrap();
    fs::write(shelf.join("старое фото.jpg"), b"same").unwrap();
    let p = s.plan(false);

    let review = s.desk().join("На удаление");
    assert_eq!(moved(&p, "пустой.txt").dst, review.join("Пустые/пустой.txt"));
    let dup = moved(&p, "фото.jpg");
    assert_eq!(dup.dst, review.join("Дубли/фото.jpg"));
    assert_eq!(dup.twin.as_deref(), Some(shelf.join("старое фото.jpg").as_path()));
    // Из двух одинаковых новых файлов первый едет на полку, второй — в дубли.
    assert!(moved(&p, "a копия.pdf").dst.starts_with(s.places.documents.join("PDF")));
    assert_eq!(moved(&p, "a.pdf").dst, review.join("Дубли/a.pdf"));
}

#[test]
fn review_folder_is_left_for_the_human() {
    let s = Sandbox::new("review");
    s.put("На удаление/старьё.dmg", b"x");
    let p = s.plan(true);
    assert_eq!(why(&p, "На удаление"), Some(Reason::Review));
}

#[test]
fn guards_protect_busy_and_scripted_folders() {
    let s = Sandbox::new("guards");
    s.put("Чисто/tekst/a.md", b"x");
    s.put("videos/clip.mp4", b"x");
    s.put("ТТ.command", br#"DIR="$HOME/Desktop/videos""#);
    let mut snap = s.snap(&[s.desk().to_path_buf()], true);
    snap.guards.push(Guard { path: s.desk().join("Чисто/tekst"), why: GuardKind::Busy("zsh".into()) });
    let p = plan::plan(&snap, &RuleSet::default(), &s.places, &Options { folders: true, ..Default::default() }, &RealDisk);

    assert_eq!(why(&p, "Чисто"), Some(Reason::Busy("zsh".into())));
    assert_eq!(why(&p, "videos"), Some(Reason::UsedBy("ТТ.command".into())));
}

#[test]
fn cloud_files_say_when_they_will_download() {
    let s = Sandbox::new("cloud");
    let item = |name: &str| Item {
        path: s.desk().join(name),
        name: name.into(),
        ext: scan::extension(name),
        kind: Kind::File,
        size: 10,
        created: 0,
        touched: 0,
        in_cloud: true,
        text: None,
    };
    let snap = Snapshot {
        folders: vec![s.desk().to_path_buf()],
        items: vec![item("клип.mp4"), item("лекция.pdf")],
        guards: vec![],
        cloud_roots: vec![s.places.desktop.clone(), s.places.documents.clone()],
        taken_at: 10_000,
    };
    let p = plan::plan(&snap, &RuleSet::default(), &s.places, &Options::default(), &RealDisk);
    assert!(moved(&p, "клип.mp4").downloads, "Фильмы вне облака — скачается");
    assert!(!moved(&p, "лекция.pdf").downloads, "Документы в облаке — останется в облаке");
    assert_eq!(p.download_size(), 10);
}

#[test]
fn many_folders_share_names_and_duplicates() {
    let s = Sandbox::new("many");
    s.put("фото.jpg", b"one");
    fs::write(s.places.downloads.join("фото.jpg"), b"two").unwrap();
    fs::write(s.places.downloads.join("копия.pdf"), b"pdf").unwrap();
    s.put("оригинал.pdf", b"pdf");
    let p = s.plan_of(&[s.desk().to_path_buf(), s.places.downloads.clone()], false);
    let dsts: Vec<_> = p.moves.iter().map(|m| m.dst.file_name().unwrap().to_string_lossy().into_owned()).collect();
    assert!(dsts.contains(&"фото.jpg".to_string()) && dsts.contains(&"фото (2).jpg".to_string()), "{dsts:?}");
    assert!(p.moves.iter().any(|m| m.rule == "Дубли"), "одинаковые PDF из разных папок узнаны");
}

#[test]
fn plan_is_deterministic() {
    let s = Sandbox::new("unique");
    s.put("фото.jpg", b"1");
    s.put("b.pdf", b"2");
    fs::create_dir_all(s.places.pictures.join("Картинки")).unwrap();
    fs::write(s.places.pictures.join("Картинки/фото.jpg"), b"old").unwrap();
    let a = s.plan(false);
    let b = s.plan(false);
    assert_eq!(a, b);
    assert!(moved(&a, "фото.jpg").dst.ends_with("фото (2).jpg"));
}

#[test]
fn already_sorted_folder_is_left_alone() {
    let s = Sandbox::new("home");
    let shelf = s.places.documents.join("PDF");
    fs::create_dir_all(&shelf).unwrap();
    fs::write(shelf.join("лекция.pdf"), b"x").unwrap();
    let p = s.plan_of(&[shelf], false);
    assert!(p.moves.is_empty());
    assert_eq!(p.skipped[0].reason, Reason::AlreadyThere);
}

#[test]
fn every_move_says_why() {
    let s = Sandbox::new("why");
    s.put("Договор аренды.pdf", b"a");
    s.put("лекция.pdf", b"b");
    s.put("dogovor_2.pdf", b"c");
    for i in 0..4 {
        s.put(&format!("Альбом/{i}.jpg"), format!("{i}").as_bytes());
    }
    s.put("Альбом/заметка.txt", b"t");
    let p = s.plan(true);
    let why = |n: &str| moved(&p, n).why.to_string();
    assert_eq!(why("Договор аренды.pdf"), "в названии «договор»");
    assert_eq!(why("dogovor_2.pdf"), "в названии «договор»");
    assert_eq!(why("лекция.pdf"), "формат .pdf");
    assert_eq!(why("Альбом"), "внутри 4 из 5 — такие файлы");
}

#[test]
fn text_decides_when_the_name_says_nothing() {
    let s = Sandbox::new("text");
    // RTF с русским в кодах cp1251, как его сохраняет Word.
    s.put("Документ1.rtf", br"{\rtf1\ansi\ansicpg1251 \'d1\'f7\'e5\'f2 \'ed\'e0 \'ee\'ef\'eb\'e0\'f2\'f3 \'b9 5\par}");
    // DOCX — zip с XML внутри.
    {
        let file = fs::File::create(s.desk().join("Новый документ.docx")).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file("word/document.xml", zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all("<w:document><w:body><w:p><w:r><w:t>Иван Петров</w:t></w:r></w:p><w:p><w:r><w:t>Опыт работы</w:t></w:r></w:p></w:body></w:document>".as_bytes()).unwrap();
        zip.finish().unwrap();
    }
    s.put("просто.rtf", br"{\rtf1 hello\par}");
    let rules = RuleSet::default();
    let snap = s.snap_with(&[s.desk().to_path_buf()], ScanOptions { read_text: Some(&rules), ..Default::default() });
    let p = plan::plan(&snap, &rules, &s.places, &Options::default(), &RealDisk);

    assert_eq!(moved(&p, "Документ1.rtf").rule, "Договоры и счета");
    assert_eq!(moved(&p, "Документ1.rtf").why, Why::Rule { evidence: Evidence::Text("счет на оплату".into()) });
    assert_eq!(moved(&p, "Новый документ.docx").rule, "Резюме");
    assert_eq!(moved(&p, "просто.rtf").why.to_string(), "формат .rtf");
}

#[test]
fn names_only_mode_never_reads_files() {
    let s = Sandbox::new("names-only");
    s.put("Документ1.rtf", br"{\rtf1 \'d1\'f7\'e5\'f2 \'ed\'e0 \'ee\'ef\'eb\'e0\'f2\'f3\par}");
    let snap = s.snap(&[s.desk().to_path_buf()], false);
    assert!(snap.items.iter().all(|i| i.text.is_none()));
    let p = plan::plan(&snap, &RuleSet::default(), &s.places, &Options::default(), &RealDisk);
    assert_eq!(moved(&p, "Документ1.rtf").rule, "Тексты");
}

// ---------- Перенос и откат ----------

#[test]
fn apply_then_undo_restores_everything() {
    let s = Sandbox::new("roundtrip");
    s.put("фото.jpg", b"jpg");
    s.put("Снимок экрана 1.png", b"png");
    s.put("отчёт.pdf", b"pdf");
    s.put("непонятно.xyz", b"?");
    for i in 0..3 {
        s.put(&format!("Отпуск/{i}.heic"), format!("h{i}").as_bytes());
    }
    let before = s.tree();
    let p = s.plan(true);
    assert!(s.desk().join("фото.jpg").exists(), "план ничего не трогает");

    let mut ticks = Vec::new();
    let o = apply(&p, &s.journal, |done, total| ticks.push((done, total))).unwrap();
    assert_eq!((o.moved, o.failed.len()), (4, 0));
    assert_eq!(ticks.last(), Some(&(4, 4)));
    assert_eq!(fs::read(s.places.pictures.join("Картинки/фото.jpg")).unwrap(), b"jpg");
    assert!(s.places.pictures.join("Картинки/Отпуск/2.heic").exists());
    assert!(s.desk().join("непонятно.xyz").exists());

    let rec = s.journal.last_active().unwrap();
    assert_eq!(rec.moves, 4);
    let u = undo(&s.journal, &rec.path).unwrap();
    assert_eq!((u.moved, u.failed.len()), (4, 0));
    assert_eq!(s.tree(), before, "всё как было, и пустые полки убраны");
    assert!(s.journal.last_active().is_none());
    assert!(s.journal.history()[0].undone);
}

#[test]
fn never_overwrites_even_if_name_taken_after_plan() {
    let s = Sandbox::new("race");
    s.put("фото.jpg", b"new");
    let shelf = s.places.pictures.join("Картинки");
    fs::create_dir_all(&shelf).unwrap();
    fs::write(shelf.join("фото.jpg"), b"old").unwrap();
    let p = s.plan(false);
    assert!(p.moves[0].dst.ends_with("фото (2).jpg"));

    fs::write(shelf.join("фото (2).jpg"), b"sneaky").unwrap();
    apply(&p, &s.journal, |_, _| {}).unwrap();
    assert_eq!(fs::read(shelf.join("фото.jpg")).unwrap(), b"old");
    assert_eq!(fs::read(shelf.join("фото (2).jpg")).unwrap(), b"sneaky");
    assert_eq!(fs::read(shelf.join("фото (3).jpg")).unwrap(), b"new");
}

#[test]
fn stale_plan_skips_vanished_files() {
    let s = Sandbox::new("stale");
    s.put("a.pdf", b"x");
    s.put("b.pdf", b"y");
    let p = s.plan(false);
    fs::remove_file(s.desk().join("a.pdf")).unwrap();
    let o = apply(&p, &s.journal, |_, _| {}).unwrap();
    assert_eq!((o.moved, o.failed.len()), (1, 1));
}

#[test]
fn undo_never_clobbers_and_can_be_retried() {
    let s = Sandbox::new("clobber");
    s.put("a.pdf", b"first");
    s.put("b.pdf", b"second");
    apply(&s.plan(false), &s.journal, |_, _| {}).unwrap();
    s.put("a.pdf", b"someone else");

    let rec = s.journal.last_active().unwrap();
    let u = undo(&s.journal, &rec.path).unwrap();
    assert_eq!((u.moved, u.failed.len()), (1, 1));
    assert_eq!(fs::read(s.desk().join("a.pdf")).unwrap(), b"someone else");
    assert_eq!(fs::read(s.places.documents.join("PDF/a.pdf")).unwrap(), b"first");
    assert!(s.journal.last_active().is_some(), "неудачный откат не закрывает журнал");

    // Человек убрал мешающий файл — повторный откат доделывает только недоделанное.
    fs::remove_file(s.desk().join("a.pdf")).unwrap();
    let again = undo(&s.journal, &rec.path).unwrap();
    assert_eq!((again.moved, again.failed.len()), (1, 0));
    assert_eq!(fs::read(s.desk().join("a.pdf")).unwrap(), b"first");
    assert_eq!(fs::read(s.desk().join("b.pdf")).unwrap(), b"second");
}

#[test]
fn undo_survives_crash_between_intent_and_done() {
    let s = Sandbox::new("crash");
    s.put("a.pdf", b"a");
    s.put("b.pdf", b"b");
    let src_a = s.desk().join("a.pdf");
    let dst_a = s.places.documents.join("a.pdf");
    let src_b = s.desk().join("b.pdf");
    let dst_b = s.places.documents.join("b.pdf");
    fs::create_dir_all(&s.places.documents).unwrap();

    // Имитация сбоя: оба намерения записаны, «a» успел переехать, «b» — нет, отметок «done» нет.
    // В конце — оборванная строка, как бывает при выключении питания.
    let mut w = s.journal.begin(&[s.desk().to_path_buf()]).unwrap();
    w.write(&Entry::Intent { id: 0, src: src_a.clone(), dst: dst_a.clone() }).unwrap();
    fs::rename(&src_a, &dst_a).unwrap();
    w.write(&Entry::Intent { id: 1, src: src_b.clone(), dst: dst_b.clone() }).unwrap();
    let path = w.path.clone();
    drop(w);
    fs::OpenOptions::new().append(true).open(&path).unwrap().write_all(b"{\"op\":\"do").unwrap();

    let u = undo(&s.journal, &path).unwrap();
    assert_eq!((u.moved, u.failed.len()), (1, 0));
    assert_eq!(fs::read(&src_a).unwrap(), b"a");
    assert_eq!(fs::read(&src_b).unwrap(), b"b");
    assert!(!dst_b.exists());
}

#[test]
fn second_run_waits_for_the_first() {
    let s = Sandbox::new("lock");
    let _held = s.journal.lock().unwrap();
    s.put("a.pdf", b"x");
    let err = apply(&s.plan(false), &s.journal, |_, _| {}).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::WouldBlock);
    assert!(s.desk().join("a.pdf").exists());
}

// ---------- Свойство: на любом столе «разложить + вернуть» = ничего не случилось ----------

/// xorshift64*: маленький предсказуемый генератор, чтобы упавший случай можно было повторить.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[(self.next() % items.len() as u64) as usize]
    }
}

const NAMES: &[&str] = &[
    "Снимок экрана 2026-10-30 в 12.34.56.png", "IMG_2500.HEIC", "договор.pdf", "фото.jpg", "фото (2).jpg",
    "отчёт.docx", "трек.mp3", "видео.mp4", "Рилс.mp4", "архив.zip", "setup.dmg", "заметка.md", "непонятно.xyz",
    "без расширения", "data.json", "Фото.JPG", "лекция.pdf", "Скан паспорта.jpg", "dogovor копия.pdf", "пусто.txt",
];

#[test]
fn random_desktops_roundtrip() {
    for seed in 1..=150u64 {
        let s = Sandbox::new(&format!("random-{seed}"));
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let mut counter = 0;
        // Иногда одинаковое содержимое — чтобы проверить и дубли.
        let mut body = |rng: &mut Rng| {
            counter += 1;
            match rng.next() % 5 {
                0 => b"twin".to_vec(),
                1 => Vec::new(),
                _ => format!("{seed}-{counter}").into_bytes(),
            }
        };
        for _ in 0..(rng.next() % 12) {
            let name = rng.pick(NAMES);
            match rng.next() % 6 {
                0 => {
                    let dir = rng.pick(&["Альбом", "Разное", "Работа 2.0", "1"]);
                    for _ in 0..(rng.next() % 5) {
                        let n = rng.pick(NAMES);
                        let b = body(&mut rng);
                        s.put(&format!("{dir}/{n}"), &b);
                    }
                }
                // Занятые имена на полках — проверяем, что их не задавит.
                1 => {
                    if let Some(rule) = RuleSet::default().rule_for(name) {
                        let shelf = s.places.get(rule.place).join(&rule.path);
                        fs::create_dir_all(&shelf).unwrap();
                        let b = body(&mut rng);
                        fs::write(shelf.join(name), b).unwrap();
                    }
                }
                _ => {
                    let b = body(&mut rng);
                    s.put(name, &b);
                }
            }
        }
        let before = s.tree();
        let contents = |t: &BTreeMap<String, Option<Vec<u8>>>| {
            let mut v: Vec<Vec<u8>> = t.values().flatten().cloned().collect();
            v.sort();
            v
        };
        let p = s.plan(rng.next().is_multiple_of(2));
        let o = apply(&p, &s.journal, |_, _| {}).unwrap();
        assert!(o.failed.is_empty(), "seed {seed}: {:?}", o.failed);
        assert_eq!(contents(&s.tree()), contents(&before), "seed {seed}: ни один файл не потерян и не задавлен");
        if let Some(rec) = s.journal.last_active() {
            let u = undo(&s.journal, &rec.path).unwrap();
            assert!(u.failed.is_empty(), "seed {seed}: {:?}", u.failed);
        }
        assert_eq!(s.tree(), before, "seed {seed}: после отката всё как было");
    }
}

#[test]
fn scan_sees_kinds() {
    let s = Sandbox::new("kinds");
    s.put("a.txt", b"x");
    s.put(".скрытый", b"x");
    fs::create_dir_all(s.desk().join("Программа.app/Contents")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::os::unix::fs::symlink(s.desk().join("a.txt"), s.desk().join("ссылка")).unwrap();
        s.put("yt-dlp", b"#!/usr/bin/env python3");
        fs::set_permissions(s.desk().join("yt-dlp"), fs::Permissions::from_mode(0o755)).unwrap();
    }
    let snap = s.snap(&[s.desk().to_path_buf()], false);
    let kind = |n: &str| snap.items.iter().find(|i| i.name == n).map(|i| i.kind.clone());
    assert_eq!(kind("a.txt"), Some(Kind::File));
    assert_eq!(kind(".скрытый"), None);
    assert_eq!(kind("Программа.app"), Some(Kind::Bundle));
    #[cfg(unix)]
    {
        assert_eq!(kind("ссылка"), Some(Kind::Link));
        assert_eq!(kind("yt-dlp"), Some(Kind::Program));
    }
}
