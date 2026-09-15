//! The graphics quality dropdown's rows: Studio's own enum spelling, so the
//! mode a label stands for is read back out of the label itself (see
//! `Shell::pick_quality`).

use gpui_kit::SharedString;
use rbx_viewer::QualityLevel;

/// Every row of the quality dropdown: `Automatic` first, then one per level.
pub(super) fn quality_labels() -> Vec<SharedString> {
    let levels = (QualityLevel::MIN..=QualityLevel::MAX)
        .map(|level| SharedString::from(format!("Level{level:02}")));

    std::iter::once(SharedString::from("Automatic"))
        .chain(levels)
        .collect()
}

/// Which of those rows a mode sits on.
pub(super) fn quality_row(mode: QualityLevel) -> usize {
    match mode {
        QualityLevel::Automatic => 0,
        QualityLevel::Level(level) => {
            usize::from(level.clamp(QualityLevel::MIN, QualityLevel::MAX))
        }
    }
}

#[cfg(test)]
mod tests {
    use rbx_viewer::QualityLevel;

    use super::{quality_labels, quality_row};

    // The dropdown carries no mapping table: a row means what its own label
    // parses as, so the two must agree for every mode the selector offers.
    #[test]
    fn every_row_parses_back_to_the_mode_it_stands_for() {
        let labels = quality_labels();
        let modes = std::iter::once(QualityLevel::Automatic)
            .chain((QualityLevel::MIN..=QualityLevel::MAX).map(QualityLevel::Level));

        assert_eq!(labels.len(), usize::from(QualityLevel::MAX) + 1);
        for mode in modes {
            let label = &labels[quality_row(mode)];
            assert_eq!(label.parse(), Ok(mode), "{label}");
        }
    }
}
