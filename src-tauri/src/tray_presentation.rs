use tauri::image::Image;

use crate::time_tracking::TimeEntryDto;

pub const DEFAULT_TRAY_ICON_STYLE: &str = "status-orb";
const ICON_SIZE: usize = 32;

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
        TrayTimerState::Idle => (190, 202, 216),
    }
}

fn state_mark_color(state: TrayTimerState) -> (u8, u8, u8) {
    match state {
        TrayTimerState::Idle => (72, 89, 108),
        TrayTimerState::Running | TrayTimerState::Paused => (250, 252, 251),
    }
}

fn draw_status_orb(state: TrayTimerState) -> Vec<u8> {
    let mut rgba = vec![0u8; ICON_SIZE * ICON_SIZE * 4];
    let center = 15.5f64;
    let radius = 15.15f64;
    let color = state_color(state);
    for y in 0..ICON_SIZE {
        for x in 0..ICON_SIZE {
            let dx = x as f64 - center;
            let dy = y as f64 - center;
            let distance = (dx * dx + dy * dy).sqrt();
            if distance > radius {
                continue;
            }
            let mut pixel = color;
            if distance > radius - 2.0 {
                pixel = if state == TrayTimerState::Idle {
                    blend(pixel, state_mark_color(state), 0.18)
                } else {
                    blend(pixel, (245, 248, 247), 0.42)
                };
            } else if x < 12 && y < 12 {
                pixel = blend(pixel, (255, 255, 255), 0.18);
            }
            set_pixel(&mut rgba, x, y, pixel, 255);
        }
    }
    draw_state_mark(&mut rgba, state, state_mark_color(state), true);
    rgba
}

fn draw_minimal(state: TrayTimerState) -> Vec<u8> {
    let mut rgba = vec![0u8; ICON_SIZE * ICON_SIZE * 4];
    let color = state_color(state);
    for y in 2..ICON_SIZE - 2 {
        for x in 2..ICON_SIZE - 2 {
            let border = x < 5 || x >= ICON_SIZE - 5 || y < 5 || y >= ICON_SIZE - 5;
            set_pixel(
                &mut rgba,
                x,
                y,
                if border {
                    if state == TrayTimerState::Idle {
                        blend(color, state_mark_color(state), 0.16)
                    } else {
                        blend(color, (255, 255, 255), 0.28)
                    }
                } else {
                    color
                },
                255,
            );
        }
    }
    draw_state_mark(&mut rgba, state, state_mark_color(state), false);
    rgba
}

fn draw_state_mark(rgba: &mut [u8], state: TrayTimerState, mark: (u8, u8, u8), rounded_idle: bool) {
    match state {
        TrayTimerState::Running => {
            for y in 8..24 {
                let half_width = if y <= 15 { y - 7 } else { 24 - y };
                for x in 11..=11 + half_width {
                    set_pixel(rgba, x, y, mark, 255);
                }
            }
        }
        TrayTimerState::Paused => {
            for y in 8..24 {
                for x in 9..14 {
                    set_pixel(rgba, x, y, mark, 255);
                }
                for x in 18..23 {
                    set_pixel(rgba, x, y, mark, 255);
                }
            }
        }
        TrayTimerState::Idle => {
            for y in 7..25 {
                for x in 7..25 {
                    let dx = x as f64 - 15.5;
                    let dy = y as f64 - 15.5;
                    let distance = (dx * dx + dy * dy).sqrt();
                    let on_ring = if rounded_idle {
                        (6.1..=8.4).contains(&distance)
                    } else {
                        x == 7 || x == 24 || y == 7 || y == 24
                    };
                    if on_ring {
                        set_pixel(rgba, x, y, mark, 255);
                    }
                }
            }
        }
    }
}

fn set_pixel(rgba: &mut [u8], x: usize, y: usize, color: (u8, u8, u8), alpha: u8) {
    let index = (y * ICON_SIZE + x) * 4;
    rgba[index] = color.0;
    rgba[index + 1] = color.1;
    rgba[index + 2] = color.2;
    rgba[index + 3] = alpha;
}

fn blend(base: (u8, u8, u8), top: (u8, u8, u8), amount: f64) -> (u8, u8, u8) {
    let mix = |a: u8, b: u8| (a as f64 + (b as f64 - a as f64) * amount) as u8;
    (mix(base.0, top.0), mix(base.1, top.1), mix(base.2, top.2))
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
            let mark = state_mark_color(state);
            draw_status_orb(state)
                .chunks_exact(4)
                .map(|pixel| (pixel[0], pixel[1], pixel[2]) == mark)
                .collect::<Vec<_>>()
        });

        assert_ne!(silhouettes[0], silhouettes[1]);
        assert_ne!(silhouettes[1], silhouettes[2]);
        assert_ne!(silhouettes[0], silhouettes[2]);

        let marked =
            |state_index: usize, x: usize, y: usize| silhouettes[state_index][y * ICON_SIZE + x];
        assert!(!marked(0, 9, 15));
        assert!(marked(1, 9, 15));
        assert!(marked(0, 16, 15));
        assert!(!marked(1, 16, 15));
        assert!(marked(2, 15, 8));
        assert!(!marked(2, 15, 15));
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
    fn idle_status_orb_is_light_with_a_clear_dark_hollow_mark() {
        let rgba = draw_status_orb(TrayTimerState::Idle);
        let pixel = |x: usize, y: usize| {
            let index = (y * ICON_SIZE + x) * 4;
            (rgba[index], rgba[index + 1], rgba[index + 2])
        };
        let fill = pixel(15, 15);
        let mark = pixel(15, 8);

        assert!(fill.0 >= 185 && fill.1 >= 195 && fill.2 >= 210);
        assert!(mark.0 < fill.0 && mark.1 < fill.1 && mark.2 < fill.2);
    }
}
