use tauri::image::Image;

use crate::time_tracking::TimeEntryDto;

pub const DEFAULT_TRAY_ICON_STYLE: &str = "status-orb";
const ICON_SIZE: usize = 32;
const ICON_PIXELS: usize = ICON_SIZE * ICON_SIZE;

const PLAY_CIRCLE_MASK: &[u8; ICON_PIXELS] =
    include_bytes!("../assets/tray-icons/material-symbols-play-circle-rounded.alpha");
const PAUSE_CIRCLE_MASK: &[u8; ICON_PIXELS] =
    include_bytes!("../assets/tray-icons/material-symbols-pause-circle-rounded.alpha");
const STOP_CIRCLE_MASK: &[u8; ICON_PIXELS] =
    include_bytes!("../assets/tray-icons/material-symbols-stop-circle-rounded.alpha");
const PLAY_MASK: &[u8; ICON_PIXELS] =
    include_bytes!("../assets/tray-icons/material-symbols-play-arrow-rounded.alpha");
const PAUSE_MASK: &[u8; ICON_PIXELS] =
    include_bytes!("../assets/tray-icons/material-symbols-pause-rounded.alpha");
const STOP_MASK: &[u8; ICON_PIXELS] =
    include_bytes!("../assets/tray-icons/material-symbols-stop-rounded.alpha");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayIconStyle {
    StatusOrb,
    Minimal,
    AppIcon,
}

impl TrayIconStyle {
    pub fn parse(value: Option<&str>) -> Self {
        match value {
            Some("minimal") => Self::Minimal,
            Some("app-icon") => Self::AppIcon,
            Some("status-orb") | None => Self::StatusOrb,
            Some(_) => Self::StatusOrb,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::StatusOrb => "status-orb",
            Self::Minimal => "minimal",
            Self::AppIcon => "app-icon",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayTimerState {
    Running,
    Paused,
    Idle,
}

impl TrayTimerState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Idle => "idle",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrayPresentation {
    pub style: TrayIconStyle,
    pub state: TrayTimerState,
    pub tooltip: String,
    pub running: bool,
    pub paused: bool,
}

impl TrayPresentation {
    pub fn from_timer_result(
        result: Result<Option<TimeEntryDto>, String>,
        style_value: Option<&str>,
    ) -> Result<Self, String> {
        result.map(|entry| Self::from_timer(entry.as_ref(), style_value))
    }

    pub fn from_timer(entry: Option<&TimeEntryDto>, style_value: Option<&str>) -> Self {
        let style = TrayIconStyle::parse(style_value);
        let state = match entry.map(|entry| entry.state.as_str()) {
            Some("running") => TrayTimerState::Running,
            Some("paused") => TrayTimerState::Paused,
            _ => TrayTimerState::Idle,
        };
        let tooltip = match (state, entry) {
            (TrayTimerState::Running, Some(entry)) => format!(
                "执行中 · {} · {}",
                timer_label(entry),
                format_duration(entry.duration_seconds)
            ),
            (TrayTimerState::Paused, Some(entry)) => format!(
                "已暂停 · {} · {}",
                timer_label(entry),
                format_duration(entry.duration_seconds)
            ),
            _ => "当前没有执行事项".to_string(),
        };
        Self {
            style,
            state,
            tooltip,
            running: state == TrayTimerState::Running,
            paused: state == TrayTimerState::Paused,
        }
    }

    pub fn cache_key(&self) -> String {
        format!(
            "{}:{}:{}",
            self.style.as_str(),
            self.state.as_str(),
            self.tooltip
        )
    }

    pub fn rgba(&self) -> Option<Vec<u8>> {
        match self.style {
            TrayIconStyle::StatusOrb => Some(draw_status_orb(self.state)),
            TrayIconStyle::Minimal => Some(draw_minimal(self.state)),
            TrayIconStyle::AppIcon => None,
        }
    }

    pub fn image(&self) -> Option<Image<'static>> {
        self.rgba()
            .map(|rgba| Image::new_owned(rgba, ICON_SIZE as u32, ICON_SIZE as u32))
    }
}

fn timer_label(entry: &TimeEntryDto) -> &str {
    let label = entry.label.trim();
    if label.is_empty() {
        "未命名计时"
    } else {
        label
    }
}

fn format_duration(seconds: i64) -> String {
    let seconds = seconds.max(0);
    let hours = seconds / 3600;
    let minutes = seconds % 3600 / 60;
    let seconds = seconds % 60;
    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

fn state_color(state: TrayTimerState) -> (u8, u8, u8) {
    match state {
        TrayTimerState::Running => (25, 195, 125),
        TrayTimerState::Paused => (233, 185, 73),
        TrayTimerState::Idle => (218, 58, 67),
    }
}

fn draw_status_orb(state: TrayTimerState) -> Vec<u8> {
    render_icon_mask(circle_mask(state), state_color(state))
}

fn draw_minimal(state: TrayTimerState) -> Vec<u8> {
    render_icon_mask(minimal_mask(state), state_color(state))
}

fn circle_mask(state: TrayTimerState) -> &'static [u8; ICON_PIXELS] {
    match state {
        TrayTimerState::Running => PLAY_CIRCLE_MASK,
        TrayTimerState::Paused => PAUSE_CIRCLE_MASK,
        TrayTimerState::Idle => STOP_CIRCLE_MASK,
    }
}

fn minimal_mask(state: TrayTimerState) -> &'static [u8; ICON_PIXELS] {
    match state {
        TrayTimerState::Running => PLAY_MASK,
        TrayTimerState::Paused => PAUSE_MASK,
        TrayTimerState::Idle => STOP_MASK,
    }
}

fn render_icon_mask(mask: &[u8; ICON_PIXELS], color: (u8, u8, u8)) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(ICON_PIXELS * 4);
    for alpha in mask {
        rgba.extend_from_slice(&[color.0, color.1, color.2, *alpha]);
    }
    rgba
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(state: &str, label: &str, duration_seconds: i64) -> TimeEntryDto {
        TimeEntryDto {
            id: "timer".to_string(),
            work_date: "2026-10-15".to_string(),
            label: label.to_string(),
            started_at: 0,
            ended_at: None,
            duration_seconds,
            settlement_minutes: 0,
            allocated_minutes: 0,
            default_task_id: None,
            note: None,
            kind: "work".to_string(),
            source_type: "timer".to_string(),
            state: state.to_string(),
            version: 1,
            timer_chain_id: None,
            previous_entry_id: None,
            split_boundary_at: None,
            allocations: Vec::new(),
        }
    }

    #[test]
    fn presentation_covers_running_paused_idle_and_unnamed_timer() {
        let running_entry = entry("running", "编写方案", 65);
        let running = TrayPresentation::from_timer(Some(&running_entry), Some("status-orb"));
        assert_eq!(running.state, TrayTimerState::Running);
        assert!(running.running);
        assert_eq!(running.tooltip, "执行中 · 编写方案 · 01:05");

        let paused_entry = entry("paused", "", 3661);
        let paused = TrayPresentation::from_timer(Some(&paused_entry), Some("minimal"));
        assert_eq!(paused.state, TrayTimerState::Paused);
        assert!(paused.paused);
        assert_eq!(paused.tooltip, "已暂停 · 未命名计时 · 01:01:01");

        let idle = TrayPresentation::from_timer(None, None);
        assert_eq!(idle.state, TrayTimerState::Idle);
        assert_eq!(idle.tooltip, "当前没有执行事项");
    }

    #[test]
    fn unknown_style_falls_back_to_status_orb() {
        assert_eq!(TrayIconStyle::parse(None), TrayIconStyle::StatusOrb);
        assert_eq!(
            TrayIconStyle::parse(Some("unknown")),
            TrayIconStyle::StatusOrb
        );
        assert_eq!(
            TrayIconStyle::parse(Some("app-icon")),
            TrayIconStyle::AppIcon
        );
    }

    #[test]
    fn failed_state_read_does_not_create_a_false_idle_presentation() {
        let result = TrayPresentation::from_timer_result(
            Err("temporary database error".to_string()),
            Some("status-orb"),
        );
        assert_eq!(result.unwrap_err(), "temporary database error");
    }

    #[test]
    fn dynamic_icons_have_expected_size_transparency_and_distinct_states() {
        for style in [TrayIconStyle::StatusOrb, TrayIconStyle::Minimal] {
            let images = [
                TrayTimerState::Running,
                TrayTimerState::Paused,
                TrayTimerState::Idle,
            ]
            .map(|state| TrayPresentation {
                style,
                state,
                tooltip: String::new(),
                running: state == TrayTimerState::Running,
                paused: state == TrayTimerState::Paused,
            })
            .map(|presentation| presentation.rgba().unwrap());
            assert!(images
                .iter()
                .all(|rgba| rgba.len() == ICON_SIZE * ICON_SIZE * 4));
            assert!(images.iter().all(|rgba| rgba[3] == 0));
            assert_ne!(images[0], images[1]);
            assert_ne!(images[1], images[2]);
            assert_ne!(images[0], images[2]);
        }
    }

    #[test]
    fn state_marks_have_distinct_silhouettes_independent_of_fill_color() {
        let silhouettes = [
            TrayTimerState::Running,
            TrayTimerState::Paused,
            TrayTimerState::Idle,
        ]
        .map(|state| {
            draw_status_orb(state)
                .chunks_exact(4)
                .map(|pixel| pixel[3] > 127)
                .collect::<Vec<_>>()
        });

        assert_ne!(silhouettes[0], silhouettes[1]);
        assert_ne!(silhouettes[1], silhouettes[2]);
        assert_ne!(silhouettes[0], silhouettes[2]);
        assert!(!silhouettes[0][15 * ICON_SIZE + 15]);
        assert!(silhouettes[1][15 * ICON_SIZE + 15]);
        assert!(!silhouettes[2][15 * ICON_SIZE + 15]);
    }

    #[test]
    fn dynamic_icons_use_most_of_the_available_canvas() {
        for style in [TrayIconStyle::StatusOrb, TrayIconStyle::Minimal] {
            let rgba = TrayPresentation {
                style,
                state: TrayTimerState::Running,
                tooltip: String::new(),
                running: true,
                paused: false,
            }
            .rgba()
            .unwrap();
            let visible_pixels = (0..ICON_SIZE)
                .flat_map(|y| (0..ICON_SIZE).map(move |x| (x, y)))
                .filter(|(x, y)| rgba[(y * ICON_SIZE + x) * 4 + 3] > 0)
                .collect::<Vec<_>>();
            let min_x = visible_pixels.iter().map(|(x, _)| *x).min().unwrap();
            let max_x = visible_pixels.iter().map(|(x, _)| *x).max().unwrap();
            let min_y = visible_pixels.iter().map(|(_, y)| *y).min().unwrap();
            let max_y = visible_pixels.iter().map(|(_, y)| *y).max().unwrap();

            assert!(min_x <= 2 && min_y <= 2);
            assert!(max_x >= 29 && max_y >= 29);
        }
    }

    #[test]
    fn idle_status_icon_uses_the_material_symbols_stop_circle_mask() {
        let rgba = draw_status_orb(TrayTimerState::Idle);
        assert!(rgba
            .chunks_exact(4)
            .zip(STOP_CIRCLE_MASK)
            .all(|(pixel, alpha)| pixel == [218, 58, 67, *alpha]));
        assert_eq!(rgba[(15 * ICON_SIZE + 15) * 4 + 3], 0);
        assert!(rgba[(2 * ICON_SIZE + 15) * 4 + 3] > 0);
    }
}
