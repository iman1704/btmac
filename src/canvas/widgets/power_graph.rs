use std::borrow::Cow;

use ratatui::{
    Frame,
    layout::{Constraint, Rect},
};

use crate::{
    app::{App, data::Values},
    canvas::{
        Painter,
        components::time_series::{AxisBound, ChartScaling, GraphData, LegendConstraints},
        drawing_utils::should_hide_x_label,
    },
    components::time_series::GraphDrawCtx,
};

fn power_legend_label(values: &Values) -> String {
    // Last value is "current" power.
    let last = values.last().copied().unwrap_or(0.0);
    // Collect visible values for avg/max (simple: all stored values).
    let flat: Vec<f64> = values.iter().copied().collect();
    let (avg, max) = if flat.is_empty() {
        (0.0, 0.0)
    } else {
        let sum: f64 = flat.iter().sum();
        let max = flat.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        (sum / flat.len() as f64, max)
    };
    format!("{last:.2}W avg, max: {avg:.2}W {max:.2}W")
}

fn adjust_power_y_labels(max_entry: f64) -> (f64, Vec<String>) {
    let max_entry_upper = if max_entry == 0.0 { 1.0 } else { max_entry * 1.5 };
    // Keep labels simple: 0, half, max, 1.5*max (like network linear)
    let base = max_entry;
    let labels: Vec<String> = vec![
        "0W".to_string(),
        format!("{:.1}W", base * 0.5),
        format!("{:.1}W", base),
        format!("{:.1}W", base * 1.5),
    ]
    .into_iter()
    .map(|s| format!("{s:>6}"))
    .collect();
    (max_entry_upper, labels)
}

impl Painter {
    pub fn draw_power_graph(
        &self, f: &mut Frame<'_>, app_state: &mut App, draw_loc: Rect, widget_id: u64,
    ) {
        if let Some(power_state) = app_state.states.power_state.widget_states.get_mut(&widget_id) {
            let hide_x_labels = should_hide_x_label(
                app_state.app_config_fields.hide_time,
                app_state.app_config_fields.autohide_time,
                power_state.graph.state_mut().autohide_timer_mut(),
                draw_loc,
            );

            let shared_data = app_state.data_store.get_data();
            let time = &shared_data.time_series_data.time;
            let power = &shared_data.time_series_data.power;

            let y_max = power_state.graph.y_max([power].into_iter(), time);
            let (adjusted_y_max, y_labels) = adjust_power_y_labels(y_max);
            let y_bounds = AxisBound::Max(adjusted_y_max);
            let y_labels: Vec<Cow<'_, str>> = y_labels.into_iter().map(Into::into).collect();

            let label = power_legend_label(power);

            let graph_data = if power.no_elements() {
                vec![]
            } else {
                vec![
                    GraphData::default()
                        .name(label.into())
                        .time(time)
                        .values(power)
                        .style(self.styles.ram_style),
                ]
            };

            let border_style = self.get_border_style(widget_id, app_state.current_widget.widget_id);
            let marker = self.get_marker(app_state.app_config_fields.use_dot);

            power_state.graph.draw(
                f,
                draw_loc,
                GraphDrawCtx {
                    title: " Power ".into(),
                    border_style,
                    title_style: self.styles.widget_title_style,
                    graph_style: self.styles.graph_style,
                    general_widget_style: self.styles.general_widget_style,
                    border_type: self.styles.border_type,
                    marker,
                    hide_x_labels,
                    is_selected: app_state.current_widget.widget_id == widget_id,
                    is_expanded: app_state.is_expanded,
                    legend_position: app_state.app_config_fields.memory_legend_position,
                    legend_constraints: Some(LegendConstraints {
                        width: Constraint::Ratio(3, 4),
                        height: Constraint::Ratio(3, 4),
                    }),
                },
                y_bounds,
                &y_labels,
                ChartScaling::Linear,
                graph_data,
            );
        }

        if app_state.should_get_widget_bounds() {
            if let Some(widget) = app_state.widget_map.get_mut(&widget_id) {
                widget.top_left_corner = Some((draw_loc.x, draw_loc.y));
                widget.bottom_right_corner =
                    Some((draw_loc.x + draw_loc.width, draw_loc.y + draw_loc.height));
            }
        }
    }
}
