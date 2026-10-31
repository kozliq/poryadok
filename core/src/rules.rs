//! Правила: какой файл на какую полку — и почему.
//!
//! Решение принимается в три ступени, от самого точного признака к самому общему:
//! 1. **слово в названии** («договор», «скриншот»);
//! 2. **фраза в тексте файла** («акт выполненных работ», «посадочный талон») —
//!    текст берётся из PDF, документов и, через встроенное в систему распознавание, из картинок;
//! 3. **расширение** (`.pdf` → «PDF»).
//!
//! Внутри ступени побеждает правило, которое выше в списке.
//!
//! Как ищется слово в названии:
//! * с начала слова: «скан» найдёт «Скан паспорта» и «сканы», но не «москанал»;
//! * без русского окончания: «смета» найдёт «сметы» и «смете»;
//! * латиницей: «договор» найдёт `dogovor_arendy.pdf`, «счёт» — `schet-42.pdf`;
//! * слитные слова делятся: `DogovorArendy` → «dogovor arendy».

use crate::places::Place;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    /// Имя полки для экрана: «Скриншоты».
    pub name: String,
    /// В каком месте лежит полка.
    pub place: Place,
    /// Путь полки внутри места: `Скриншоты`, `Работа/Договоры`.
    #[serde(default)]
    pub path: String,
    /// Расширения без точки. Пусто — любое.
    #[serde(default)]
    pub exts: Vec<String>,
    /// Слова в названии.
    #[serde(default)]
    pub keywords: Vec<String>,
    /// Фразы в тексте файла.
    #[serde(default)]
    pub content: Vec<String>,
    /// Раскладывать ещё и по месяцам: `Скриншоты/2026-10`.
    #[serde(default)]
    pub by_month: bool,
}

impl Rule {
    fn accepts(&self, ext: &str) -> bool {
        self.exts.is_empty() || self.exts.iter().any(|e| e.eq_ignore_ascii_case(ext))
    }
}

/// Чем подтвердилось правило — это и показываем человеку.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "by", content = "what", rename_all = "kebab-case")]
pub enum Evidence {
    /// В названии нашлось слово (как оно записано в правиле).
    Word(String),
    /// В тексте файла нашлась фраза.
    Text(String),
    /// Имя устроено по узнаваемому образцу, например «дата и время съёмки».
    Shape(String),
    /// Подошло только расширение.
    Extension(String),
}

impl fmt::Display for Evidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Evidence::Word(w) => write!(f, "в названии «{w}»"),
            Evidence::Text(w) => write!(f, "в тексте «{w}»"),
            Evidence::Shape(w) => write!(f, "имя — {w}"),
            Evidence::Extension(e) => write!(f, "формат .{e}"),
        }
    }
}

/// Слово, готовое к поиску: нормализованное написание и то, как его показать человеку.
#[derive(Debug, Clone)]
struct Needle {
    find: String,
    show: String,
}

/// Правила, подготовленные к быстрому поиску: слова нормализуются один раз.
#[derive(Debug, Clone)]
pub struct RuleSet {
    rules: Vec<Rule>,
    names: Vec<Vec<Needle>>,
    texts: Vec<Vec<Needle>>,
}

/// Особое «слово» в правиле: имя начинается с даты и времени (`2026-10-24 22.14.19.jpg`,
/// `20261024_221419.jpg`). Так называют снимки камеры и телефона.
pub const DATE_TIME: &str = "#дата-время";

fn looks_like_date_time(normalized: &str) -> bool {
    let digits: String = normalized.split(' ').filter(|t| !t.is_empty()).take_while(|t| t.chars().all(|c| c.is_ascii_digit())).collect();
    let num = |a: usize, b: usize| digits.get(a..b).and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
    digits.len() >= 12 && (1990..=2100).contains(&num(0, 4)) && (1..=12).contains(&num(4, 6)) && (1..=31).contains(&num(6, 8)) && num(8, 10) < 24
}

fn needles(words: &[String], latin: bool) -> Vec<Needle> {
    words
        .iter()
        .filter(|w| !w.starts_with('#'))
        .flat_map(|w| {
            let shown = w.clone();
            spellings(w, latin).into_iter().map(move |v| Needle { find: format!(" {v}"), show: shown.clone() })
        })
        .collect()
}

impl RuleSet {
    pub fn new(rules: Vec<Rule>) -> Self {
        let names = rules.iter().map(|r| needles(&r.keywords, true)).collect();
        // В тексте ищем как написано: латинские двойники там дают ложные совпадения.
        let texts = rules.iter().map(|r| needles(&r.content, false)).collect();
        RuleSet { rules, names, texts }
    }

    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    pub fn get(&self, index: usize) -> &Rule {
        &self.rules[index]
    }

    /// Правило и доказательство. `text` — начало текста файла, если его удалось прочитать.
    pub fn explain(&self, file_name: &str, text: Option<&str>) -> Option<(usize, Evidence)> {
        let path = Path::new(file_name);
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        let stem = path.file_stem().map(|s| s.to_string_lossy()).unwrap_or_default();
        let hay = format!(" {}", normalize(&stem));
        let all = || self.rules.iter().enumerate().filter(|(_, r)| r.accepts(&ext));

        for (i, rule) in all() {
            if let Some(n) = self.names[i].iter().find(|n| hay.contains(n.find.as_str())) {
                return Some((i, Evidence::Word(n.show.clone())));
            }
            if rule.keywords.iter().any(|k| k == DATE_TIME) && looks_like_date_time(&hay) {
                return Some((i, Evidence::Shape("дата и время съёмки".into())));
            }
        }
        if let Some(text) = text {
            let body = format!(" {}", normalize(text));
            for (i, _) in all() {
                if let Some(n) = self.texts[i].iter().find(|n| body.contains(n.find.as_str())) {
                    return Some((i, Evidence::Text(n.show.clone())));
                }
            }
        }
        all()
            .find(|(i, r)| self.names[*i].is_empty() && self.texts[*i].is_empty() && !r.exts.is_empty())
            .map(|(i, _)| (i, Evidence::Extension(ext.clone())))
    }

    /// Номер правила только по названию — для голосования файлов внутри папки.
    pub fn classify(&self, file_name: &str) -> Option<usize> {
        self.explain(file_name, None).map(|(i, _)| i)
    }

    pub fn rule_for(&self, file_name: &str) -> Option<&Rule> {
        self.classify(file_name).map(|i| self.get(i))
    }

    /// Стоит ли читать текст файла: по названию понятно только расширение,
    /// а какое-то правило умеет узнавать такие файлы по тексту.
    pub fn wants_text(&self, file_name: &str) -> bool {
        let ext = crate::scan::extension(file_name);
        let by_name = self.explain(file_name, None);
        let generic = matches!(by_name, None | Some((_, Evidence::Extension(_))));
        generic && self.rules.iter().zip(&self.texts).any(|(r, t)| !t.is_empty() && r.accepts(&ext))
    }
}

impl Default for RuleSet {
    fn default() -> Self {
        RuleSet::new(default_rules())
    }
}

/// Приводит название к виду для сравнения: нижний регистр, «ё» = «е»,
/// любые разделители — один пробел, слитные слова делятся по заглавной букве.
/// «Снимок_экрана—2026» → «снимок экрана 2026», `DogovorArendy` → «dogovor arendy».
///
/// Отдельно чиним «й» и «ё», собранные из двух символов: так их пишут
/// файлы, пришедшие с некоторых сайтов и из старых архивов.
pub fn normalize(s: &str) -> String {
    let fixed = s.replace("и\u{306}", "й").replace("И\u{306}", "Й").replace("е\u{308}", "ё").replace("Е\u{308}", "Ё");
    let mut out = String::with_capacity(fixed.len() + 8);
    let mut gap = true;
    let mut prev_lower = false;
    for c in fixed.chars() {
        if c.is_alphanumeric() {
            if c.is_uppercase() && prev_lower {
                out.push(' ');
            }
            out.extend(c.to_lowercase().map(|l| if l == 'ё' { 'е' } else { l }));
            gap = false;
            prev_lower = c.is_lowercase();
        } else {
            if !gap {
                out.push(' ');
                gap = true;
            }
            prev_lower = false;
        }
    }
    if out.ends_with(' ') {
        out.pop();
    }
    out
}

/// Окончания, которые отрезаем у русских слов: «смета» → «смет».
const RU_ENDINGS: &[char] = &['а', 'е', 'и', 'о', 'у', 'ы', 'э', 'ю', 'я', 'й', 'ь'];
/// Три распространённых способа писать русские слова латиницей.
const SCHEMES: usize = 3;

fn latin(c: char, scheme: usize) -> Option<&'static str> {
    let pick = |v: [&'static str; SCHEMES]| v[scheme];
    Some(match c {
        'а' => "a", 'б' => "b", 'в' => "v", 'г' => "g", 'д' => "d", 'е' | 'ё' | 'э' => "e",
        'ж' => pick(["zh", "zh", "j"]), 'з' => "z", 'и' => "i", 'й' => pick(["y", "j", "i"]), 'к' => "k",
        'л' => "l", 'м' => "m", 'н' => "n", 'о' => "o", 'п' => "p", 'р' => "r", 'с' => "s", 'т' => "t",
        'у' => "u", 'ф' => "f", 'х' => pick(["kh", "h", "h"]), 'ц' => pick(["ts", "c", "ts"]), 'ч' => "ch",
        'ш' => "sh", 'щ' => pick(["shch", "sch", "sh"]), 'ъ' | 'ь' => "", 'ы' => pick(["y", "i", "y"]),
        'ю' => pick(["yu", "ju", "iu"]), 'я' => pick(["ya", "ja", "ia"]),
        _ => return None,
    })
}

fn translit(s: &str, scheme: usize) -> String {
    s.chars().map(|c| latin(c, scheme).map(str::to_string).unwrap_or_else(|| c.to_string())).collect()
}

/// Все написания слова: как есть, без окончания и (если `with_latin`) латиницей.
/// Короткие слова (меньше 4 букв) латиницей не пишем: «инн» → «inn» ловил бы `innovation`.
pub fn spellings(keyword: &str, with_latin: bool) -> Vec<String> {
    let base = normalize(keyword);
    let mut forms = vec![base.clone()];
    let last = base.rsplit(' ').next().unwrap_or("");
    let letters: Vec<char> = last.chars().collect();
    if letters.len() >= 5 && letters.last().is_some_and(|c| RU_ENDINGS.contains(c)) {
        forms.push(format!("{}{}", &base[..base.len() - last.len()], letters[..letters.len() - 1].iter().collect::<String>()));
    }
    let mut out: Vec<String> = Vec::new();
    for form in forms {
        let cyrillic = form.chars().any(|c| latin(c, 0).is_some());
        let latin_ok = with_latin && cyrillic && form.chars().count() >= 4;
        for v in std::iter::once(form.clone()).chain((0..SCHEMES).filter(|_| latin_ok).map(|s| translit(&form, s))) {
            if !v.is_empty() && !out.contains(&v) {
                out.push(v);
            }
        }
    }
    out
}

const IMAGES: &[&str] = &[
    "jpg", "jpeg", "png", "heic", "heif", "gif", "webp", "bmp", "tif", "tiff", "svg", "avif", "raw", "cr2", "cr3", "nef", "arw", "dng",
];
const VIDEO: &[&str] = &["mp4", "mov", "m4v", "avi", "mkv", "webm", "wmv", "flv", "3gp", "mts"];
const AUDIO: &[&str] = &["mp3", "wav", "m4a", "aac", "flac", "ogg", "opus", "aiff", "aif", "wma", "amr"];
const TEXTS: &[&str] = &["doc", "docx", "pages", "odt", "rtf"];
const PDF: &[&str] = &["pdf"];
const TABLES: &[&str] = &["xls", "xlsx", "csv", "tsv", "numbers", "ods"];

fn rule(name: &str, place: Place, path: &str, exts: &[&[&str]], keywords: &[&str], by_month: bool) -> Rule {
    Rule {
        name: name.into(),
        place,
        path: path.into(),
        exts: exts.iter().flat_map(|group| group.iter()).map(|e| e.to_string()).collect(),
        keywords: keywords.iter().map(|k| k.to_string()).collect(),
        content: vec![],
        by_month,
    }
}

impl Rule {
    /// Фразы, по которым правило узнаёт файл по тексту.
    fn reads(mut self, phrases: &[&str]) -> Self {
        self.content = phrases.iter().map(|p| p.to_string()).collect();
        self
    }
}

/// Стандартные полки. Свои можно задать в `rules.json` — формат тот же.
pub fn default_rules() -> Vec<Rule> {
    use Place::*;
    vec![
        // Сначала — с особыми приметами: слова в названии или фразы в тексте.
        rule("Скриншоты", Pictures, "Скриншоты", &[IMAGES], &["снимок экрана", "screenshot", "screen shot", "скриншот", "скрин", "cleanshot"], true),
        rule("Записи экрана", Videos, "Записи экрана", &[VIDEO], &["запись экрана", "screen recording", "screenrecording"], true),
        rule("Рилсы", Videos, "Рилсы", &[VIDEO], &["рилс", "reels", "reel", "shorts", "tiktok", "тикток"], false),
        rule("Договоры и счета", Documents, "Договоры и счета", &[TEXTS, PDF, IMAGES], &["договор", "счет", "invoice", "contract", "квитанция", "акт сверки", "оферта", "receipt"], false).reads(&[
            "договор оказания", "договор аренды", "договор подряда", "договор купли", "трудовой договор", "счет на оплату",
            "счет фактура", "акт выполненных работ", "акт сверки", "кассовый чек", "итого к оплате", "квитанция", "invoice",
        ]),
        rule("Сметы и предложения", Documents, "Сметы и предложения", &[TEXTS, PDF, TABLES], &["смета", "кп", "предложение", "прайс", "price", "estimate", "proposal"], false)
            .reads(&["смета", "коммерческое предложение", "предложение к согласованию", "прайс лист", "стоимость работ"]),
        rule("Билеты", Documents, "Билеты", &[IMAGES, PDF], &["билет", "ticket", "boarding pass", "посадочный"], false)
            .reads(&["посадочный талон", "электронный билет", "маршрутная квитанция", "boarding pass", "e ticket"]),
        rule("Резюме", Documents, "Резюме", &[TEXTS, PDF], &["резюме", "resume", "cv"], false)
            .reads(&["опыт работы", "ключевые навыки", "curriculum vitae", "work experience"]),
        rule("Сканы документов", Documents, "Сканы", &[IMAGES, PDF], &["паспорт", "passport", "скан", "scan", "снилс", "инн", "полис"], false)
            .reads(&["паспорт гражданина", "снилс", "водительское удостоверение", "свидетельство о рождении", "страховой номер"]),
        rule("С телефона", Pictures, "С телефона", &[IMAGES, VIDEO], &["img", "dsc", "pxl", "mvimg", DATE_TIME], true),
        rule("Из Telegram", Pictures, "Из Telegram", &[IMAGES], &["photo", "telegram"], false),
        // Потом — просто по типу файла.
        rule("Картинки", Pictures, "Картинки", &[IMAGES], &[], false),
        rule("Видео", Videos, "Видео", &[VIDEO], &[], false),
        rule("Музыка и звук", Music, "Аудио", &[AUDIO], &[], false),
        rule("PDF", Documents, "PDF", &[PDF], &[], false),
        rule("Тексты", Documents, "Тексты", &[TEXTS], &[], false),
        rule("Заметки", Documents, "Заметки", &[&["txt", "md", "markdown"]], &[], false),
        rule("Таблицы", Documents, "Таблицы", &[TABLES], &[], false),
        rule("Презентации", Documents, "Презентации", &[&["ppt", "pptx", "key", "odp"]], &[], false),
        rule("Книги", Documents, "Книги", &[&["epub", "fb2", "mobi", "djvu"]], &[], false),
        rule("Архивы", Documents, "Архивы", &[&["zip", "rar", "7z", "tar", "gz", "tgz", "bz2", "xz"]], &[], false),
        rule("Установщики", Desktop, "На удаление/Установщики", &[&["dmg", "pkg", "apk", "exe", "msi", "msix", "ipa", "deb", "rpm", "appimage"]], &[], false),
        rule("Дизайн", Documents, "Дизайн", &[&["psd", "fig", "sketch", "ai", "xd", "indd", "afdesign", "afphoto"]], &[], false),
        rule("3D", Documents, "3D", &[&["stl", "obj", "fbx", "blend", "glb", "gltf", "3mf", "usdz"]], &[], false),
        rule("Шрифты", Documents, "Шрифты", &[&["ttf", "otf", "woff", "woff2"]], &[], false),
        // Вырезки текста почти всегда случайны: так решалось и при ручной разборке, — на проверку.
        rule("Вырезки", Desktop, "На удаление/Вырезки", &[&["textclipping"]], &[], false),
        rule("Ссылки", Documents, "Ссылки", &[&["webloc", "url", "inetloc"]], &[], false),
        rule("Код", Documents, "Код", &[&["py", "js", "ts", "html", "css", "json", "sh", "rs", "swift", "go", "java", "kt", "c", "cpp", "rb", "php", "sql", "yaml", "yml", "xml"]], &[], false),
        rule("Торренты", Downloads, "Торренты", &[&["torrent"]], &[], false),
    ]
}
