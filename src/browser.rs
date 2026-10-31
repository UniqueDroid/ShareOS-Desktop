use crate::nas::Entry;
use eframe::egui;
#[derive(Clone, Copy, PartialEq, Default)]
pub enum Sort {
    #[default]
    Name,
    Size,
    Modified,
}
pub fn visible<'a>(
    entries: &'a [Entry],
    query: &str,
    hidden: bool,
    sort: Sort,
    descending: bool,
) -> Vec<&'a Entry> {
    let query = query.to_lowercase();
    let mut rows: Vec<_> = entries
        .iter()
        .filter(|entry| {
            (hidden || !entry.name.starts_with('.')) && entry.name.to_lowercase().contains(&query)
        })
        .collect();
    rows.sort_by(|a, b| {
        let group = (b.kind == "dir").cmp(&(a.kind == "dir"));
        let value = match sort {
            Sort::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            Sort::Size => a.size.cmp(&b.size),
            Sort::Modified => a.mtime.cmp(&b.mtime),
        }
        .then_with(|| a.name.cmp(&b.name));
        group.then(if descending { value.reverse() } else { value })
    });
    rows
}
pub fn size(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    for unit in ["KiB", "MiB", "GiB", "TiB"] {
        value /= 1024.0;
        if value < 1024.0 || unit == "TiB" {
            return format!("{value:.1} {unit}");
        }
    }
    unreachable!()
}
pub fn modified(timestamp: Option<i64>) -> String {
    let Some(timestamp) = timestamp.filter(|t| *t >= 0) else {
        return "–".into();
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let seconds = now.saturating_sub(timestamp as u64);
    match seconds {
        0..=59 => "gerade eben".into(),
        60..=3599 => format!("vor {} Min.", seconds / 60),
        3600..=86399 => format!("vor {} Std.", seconds / 3600),
        _ => format!("vor {} Tagen", seconds / 86400),
    }
}
pub fn kind(entry: &Entry) -> &str {
    if entry.kind == "dir" {
        "Ordner"
    } else if entry.kind != "file" {
        "Verknüpfung / Spezialdatei"
    } else {
        match entry
            .name
            .rsplit('.')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str()
        {
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "heic" => "Bild",
            "mp4" | "mkv" | "mov" | "avi" | "webm" => "Video",
            "mp3" | "flac" | "wav" | "ogg" | "m4a" => "Audio",
            "zip" | "gz" | "tar" | "7z" | "rar" => "Archiv",
            "pdf" => "PDF",
            "txt" | "md" | "csv" | "log" | "json" | "xml" => "Text",
            _ => "Datei",
        }
    }
}
pub fn icon(ui: &mut egui::Ui, directory: bool, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::hover());
    let painter = ui.painter();
    if directory {
        painter.rect_filled(
            egui::Rect::from_min_size(rect.min + egui::vec2(1.0, 4.0), egui::vec2(9.0, 5.0)),
            2,
            color,
        );
        painter.rect_filled(
            egui::Rect::from_min_size(rect.min + egui::vec2(1.0, 7.0), egui::vec2(20.0, 13.0)),
            3,
            color,
        );
    } else {
        let paper = rect.shrink2(egui::vec2(4.0, 1.0));
        painter.rect_stroke(
            paper,
            2,
            egui::Stroke::new(1.5_f32, color),
            egui::StrokeKind::Inside,
        );
        for y in [8.0, 12.0, 16.0] {
            painter.line_segment(
                [
                    paper.min + egui::vec2(3.0, y),
                    paper.min + egui::vec2(9.0, y),
                ],
                egui::Stroke::new(1.0_f32, color),
            );
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sorting_keeps_folders_first_and_search_respects_hidden_files() {
        let entries: Vec<Entry> = serde_json::from_value(serde_json::json!([
            {"name":"z.txt","type":"file","size":9,"mtime":1},
            {"name":"Team","type":"dir","size":0,"mtime":2},
            {"name":"A.txt","type":"file","size":1,"mtime":3},
            {"name":".secret","type":"file","size":4,"mtime":4}
        ]))
        .unwrap();
        let rows = visible(&entries, "", false, Sort::Size, true);
        assert_eq!(
            rows.iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Team", "z.txt", "A.txt"]
        );
        assert_eq!(
            visible(&entries, "a.TXT", false, Sort::Name, false)[0].name,
            "A.txt"
        );
        assert!(visible(&entries, "secret", false, Sort::Name, false).is_empty());
        assert_eq!(
            visible(&entries, "secret", true, Sort::Name, false).len(),
            1
        );
    }
}
