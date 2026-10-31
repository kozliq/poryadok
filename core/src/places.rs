//! Места, куда раскладываем: Документы, Изображения, Фильмы…
//!
//! Пути не зашиты в код: на Windows «Документы» могут жить в OneDrive,
//! на Linux — где скажет XDG. Поэтому их спрашиваем у системы,
//! а в тестах подставляем свою песочницу.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Place {
    Desktop,
    Downloads,
    Documents,
    Pictures,
    Videos,
    Music,
    Home,
}

impl Place {
    pub const ALL: [Place; 7] = [
        Place::Desktop,
        Place::Downloads,
        Place::Documents,
        Place::Pictures,
        Place::Videos,
        Place::Music,
        Place::Home,
    ];

    /// Как эту папку называет Finder или Проводник.
    pub fn label(self) -> &'static str {
        match self {
            Place::Desktop => "Рабочий стол",
            Place::Downloads => "Загрузки",
            Place::Documents => "Документы",
            Place::Pictures => "Изображения",
            Place::Videos if cfg!(target_os = "macos") => "Фильмы",
            Place::Videos => "Видео",
            Place::Music => "Музыка",
            Place::Home => "Дом",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Places {
    pub home: PathBuf,
    pub desktop: PathBuf,
    pub downloads: PathBuf,
    pub documents: PathBuf,
    pub pictures: PathBuf,
    pub videos: PathBuf,
    pub music: PathBuf,
}

impl Places {
    /// Настоящие папки этого компьютера.
    pub fn system() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let or = |found: Option<PathBuf>, fallback: &str| found.unwrap_or_else(|| home.join(fallback));
        Places {
            desktop: or(dirs::desktop_dir(), "Desktop"),
            downloads: or(dirs::download_dir(), "Downloads"),
            documents: or(dirs::document_dir(), "Documents"),
            pictures: or(dirs::picture_dir(), "Pictures"),
            videos: or(dirs::video_dir(), if cfg!(windows) { "Videos" } else { "Movies" }),
            music: or(dirs::audio_dir(), "Music"),
            home: home.clone(),
        }
    }

    /// Те же папки, но внутри `root`. Для тестов и флага `--в`.
    pub fn under(root: &Path) -> Self {
        Places {
            home: root.to_path_buf(),
            desktop: root.join("Desktop"),
            downloads: root.join("Downloads"),
            documents: root.join("Documents"),
            pictures: root.join("Pictures"),
            videos: root.join(if cfg!(windows) { "Videos" } else { "Movies" }),
            music: root.join("Music"),
        }
    }

    pub fn get(&self, place: Place) -> &Path {
        match place {
            Place::Home => &self.home,
            Place::Desktop => &self.desktop,
            Place::Downloads => &self.downloads,
            Place::Documents => &self.documents,
            Place::Pictures => &self.pictures,
            Place::Videos => &self.videos,
            Place::Music => &self.music,
        }
    }

    /// Человеческое имя пути: `/Users/me/Pictures/Скриншоты` → `Изображения / Скриншоты`.
    pub fn describe(&self, path: &Path) -> String {
        let best = Place::ALL
            .iter()
            .filter_map(|&p| path.strip_prefix(self.get(p)).ok().map(|rest| (p, rest)))
            .min_by_key(|(_, rest)| rest.components().count());
        match best {
            Some((place, rest)) => std::iter::once(place.label().to_string())
                .chain(rest.components().map(|c| c.as_os_str().to_string_lossy().into_owned()))
                .collect::<Vec<_>>()
                .join(" / "),
            None => path.display().to_string(),
        }
    }
}
