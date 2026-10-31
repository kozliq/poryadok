//! Размер окна-визитки — считается по экрану, а не берётся с потолка.
//!
//! 1. Пропорции — визитка 85 × 55 мм (стандарт ISO/IEC 7810 ID-1 близок к ним).
//!    55/85 ≈ 0,647 — почти обратное золотое сечение 1/φ ≈ 0,618, поэтому глазу спокойно.
//! 2. Ширина — малая доля золотого сечения рабочей области: W = S / φ² ≈ 38,2 % экрана.
//!    Так окно занимает «вторичную» часть поля зрения и не спорит с остальным экраном.
//! 3. Высота не больше S_h / φ — на широких мониторах визитка не становится простынёй.
//! 4. Положение — оптический центр. Геометрический центр глазу кажется ниже середины,
//!    поэтому свободное место по вертикали делим золотым сечением: сверху 38,2 %, снизу 61,8 %.
//! 5. Кегль текста — W / 36: от 15 px на самом маленьком окне, в строку помещается ~45–60 знаков.

#[allow(clippy::approx_constant)] // `f64::consts::GOLDEN_RATIO` есть не во всех версиях Rust
pub const PHI: f64 = 1.618_033_988_749_895;
/// Высота к ширине у визитки 85 × 55 мм.
pub const CARD_RATIO: f64 = 55.0 / 85.0;
/// Сколько «кеглей» в ширине визитки.
pub const EMS_PER_WIDTH: f64 = 36.0;
/// Меньше этого — текст мельче 15 px; больше — визитка превращается в плакат.
pub const MIN_WIDTH: f64 = 540.0;
pub const MAX_WIDTH: f64 = 760.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Окно для рабочей области экрана (без панели меню, Dock и панели задач), в логических точках.
pub fn window_for(area: Rect) -> Rect {
    let mut width = (area.width / (PHI * PHI)).clamp(MIN_WIDTH, MAX_WIDTH).min(area.width);
    let mut height = width * CARD_RATIO;
    if height > area.height / PHI {
        height = area.height / PHI;
        width = height / CARD_RATIO;
    }
    let x = area.x + (area.width - width) / 2.0;
    let y = area.y + (area.height - height) / (PHI * PHI);
    Rect { x: x.round(), y: y.round(), width: width.round(), height: height.round() }
}

/// Базовый кегль для этой ширины.
pub fn base_font(width: f64) -> f64 {
    width / EMS_PER_WIDTH
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(width: f64, height: f64) -> Rect {
        Rect { x: 0.0, y: 0.0, width, height }
    }

    #[test]
    fn macbook_air_13() {
        // MacBook Air 13": экран 1470 × 956 точек, минус панель меню 37 — рабочая область 1470 × 919.
        let w = window_for(area(1470.0, 919.0));
        assert_eq!((w.width, w.height), (561.0, 363.0));
        assert!(base_font(w.width) >= 15.0);
        // Над окном меньше места, чем под ним — оптический центр.
        assert!(w.y < 919.0 - w.y - w.height);
    }

    #[test]
    fn full_hd_windows_at_125_percent() {
        // 1920 × 1080 при масштабе 125 % → 1536 × 816 логических точек без панели задач.
        let w = window_for(area(1536.0, 816.0));
        assert_eq!((w.width, w.height), (587.0, 380.0));
    }

    #[test]
    fn big_and_small_screens_stay_card_shaped() {
        let huge = window_for(area(3840.0, 2120.0));
        assert_eq!(huge.width, MAX_WIDTH);
        let tiny = window_for(area(1024.0, 700.0));
        assert_eq!(tiny.width, MIN_WIDTH);
        let short = window_for(area(1400.0, 500.0));
        assert!(short.height <= (500.0 / PHI).round());
        for w in [huge, tiny, short] {
            assert!((w.height / w.width - CARD_RATIO).abs() < 0.01);
        }
    }

    #[test]
    fn respects_work_area_offset() {
        let w = window_for(Rect { x: 1920.0, y: 25.0, width: 1470.0, height: 919.0 });
        assert!(w.x > 1920.0 && w.y > 25.0);
    }
}
