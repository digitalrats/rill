use egui::{Ui, Slider, Grid, Color32, Sense};
use kama_core::{ParameterHost, ParameterInfo, signal::{Signal, SignalHandler, ParameterChanged}};
use serde_json::{Value, json};

/// GUI контроллер параметров
pub struct ParameterGuiController {
    parameter_host: Box<dyn ParameterHost>,
    pending_changes: Vec<(String, f32)>,
}

impl ParameterGuiController {
    pub fn new(parameter_host: Box<dyn ParameterHost>) -> Self {
        Self {
            parameter_host,
            pending_changes: Vec::new(),
        }
    }
    
    pub fn draw_parameter(&mut self, ui: &mut Ui, param_id: &str) {
        if let Some(info) = self.parameter_host.get_parameter_info(param_id) {
            ui.horizontal(|ui| {
                ui.label(&info.name);
                
                let mut value = self.parameter_host.get_parameter(param_id)
                    .unwrap_or(info.default);
                
                if let Some(step) = info.step {
                    ui.add(Slider::new(&mut value, info.range.0..=info.range.1)
                        .step_by(step as f64));
                } else {
                    ui.add(Slider::new(&mut value, info.range.0..=info.range.1));
                }
                
                if ui.ctx().pointer_hover_pos()
                    .map(|pos| ui.rect().contains(pos))
                    .unwrap_or(false)
                    && ui.ctx().input(|i| i.pointer.any_down())
                {
                    self.pending_changes.push((param_id.to_string(), value));
                }
            });
        }
    }
    
    pub fn draw_knob(&mut self, ui: &mut Ui, param_id: &str, size: f32) -> egui::Response {
        let response = ui.add(egui::widgets::Label::new("").sense(Sense::click_and_drag()));
        
        if response.dragged() {
            let delta = response.drag_delta().y * -0.01; // Инвертируем Y
            if let Some(info) = self.parameter_host.get_parameter_info(param_id) {
                let mut value = self.parameter_host.get_parameter(param_id)
                    .unwrap_or(info.default);
                value = (value + delta).clamp(info.range.0, info.range.1);
                self.pending_changes.push((param_id.to_string(), value));
            }
        }
        
        response
    }
    
    pub fn take_changes(&mut self) -> Vec<(String, f32)> {
        std::mem::take(&mut self.pending_changes)
    }
}

/// Сигнальный обработчик для GUI
pub struct GuiSignalHandler {
    ui_update_fn: Box<dyn FnMut(&Value) + Send + Sync>,
}

impl SignalHandler<ParameterChanged> for GuiSignalHandler {
    fn handle(&mut self, signal: &ParameterChanged) {
        let json = json!({
            "node_id": signal.node_id,
            "parameter_id": signal.parameter_id,
            "value": signal.value,
            "normalized_value": signal.normalized_value,
            "timestamp": signal.timestamp,
        });
        
        (self.ui_update_fn)(&json);
    }
}