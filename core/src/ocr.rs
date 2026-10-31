//! Текст на картинке — распознаванием, которое уже встроено в систему.
//! На Маке это Vision (тот же движок, что выделяет текст на фото в «Просмотре»),
//! на Windows — Windows.Media.Ocr. Ничего не ставится и не уходит в сеть.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Дольше этого распознавание одной картинки не ждём.
const PATIENCE: Duration = Duration::from_secs(5);
/// Распознавание сломалось — до конца работы программы больше не пробуем.
static BROKEN: AtomicBool = AtomicBool::new(false);

/// Распознаёт с предохранителем. Бывает, что системный движок не отвечает десятки секунд
/// (так было на macOS 27: ошибка нейродвижка `e5rt`, и в Swift тоже). Тогда ждём не дольше
/// [`PATIENCE`] (5 с), выключаем распознавание и дальше решаем по названиям — план не зависает.
pub fn recognize(path: &Path) -> Option<String> {
    if BROKEN.load(Ordering::Relaxed) {
        return None;
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let owned = path.to_path_buf();
    std::thread::spawn(move || {
        let _ = tx.send(recognize_now(&owned));
    });
    // Ждём короткими шагами: если у соседнего потока распознавание уже не ответило,
    // остальные сдаются сразу, а не досиживают каждый свои секунды.
    let started = std::time::Instant::now();
    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(text) => return text,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return None,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if BROKEN.load(Ordering::Relaxed) {
                    return None;
                }
                if started.elapsed() >= PATIENCE {
                    BROKEN.store(true, Ordering::Relaxed);
                    return None;
                }
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn recognize_now(path: &Path) -> Option<String> {
    use objc2::rc::autoreleasepool;
    use objc2::AnyThread;
    use objc2_foundation::{NSArray, NSDictionary, NSString, NSURL};
    use objc2_vision::{VNImageRequestHandler, VNRecognizeTextRequest, VNRequest, VNRequestTextRecognitionLevel};

    autoreleasepool(|_| {
        let url = NSURL::fileURLWithPath(&NSString::from_str(path.to_str()?));
        let handler = unsafe { VNImageRequestHandler::initWithURL_options(VNImageRequestHandler::alloc(), &url, &NSDictionary::new()) };
        let request = VNRecognizeTextRequest::new();
        request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
        request.setUsesLanguageCorrection(true);
        request.setRecognitionLanguages(&NSArray::from_retained_slice(&[NSString::from_str("ru-RU"), NSString::from_str("en-US")]));
        let generic: &VNRequest = &request;
        handler.performRequests_error(&NSArray::from_slice(&[generic])).ok()?;
        let mut text = String::new();
        for observation in request.results()?.iter() {
            if let Some(best) = observation.topCandidates(1).firstObject() {
                text.push_str(&best.string().to_string());
                text.push('\n');
            }
        }
        Some(text)
    })
}

#[cfg(windows)]
fn recognize_now(path: &Path) -> Option<String> {
    use windows::core::HSTRING;
    use windows::Graphics::Imaging::BitmapDecoder;
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::{FileAccessMode, StorageFile};

    let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(path.as_os_str())).ok()?.join().ok()?;
    let stream = file.OpenAsync(FileAccessMode::Read).ok()?.join().ok()?;
    let bitmap = BitmapDecoder::CreateAsync(&stream).ok()?.join().ok()?.GetSoftwareBitmapAsync().ok()?.join().ok()?;
    let engine = OcrEngine::TryCreateFromUserProfileLanguages().ok()?;
    let result = engine.RecognizeAsync(&bitmap).ok()?.join().ok()?;
    Some(result.Text().ok()?.to_string_lossy())
}

#[cfg(not(any(target_os = "macos", windows)))]
fn recognize_now(_path: &Path) -> Option<String> {
    None
}
