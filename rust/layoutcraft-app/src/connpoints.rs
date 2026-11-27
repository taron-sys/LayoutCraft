//! The Connection Point tool: a click on a shape adds a connection point there, a click on a point chooses it and Delete
//! removes it, a drag moves it. The points and the edits are the engine's; this file asks and draws.
use super::{LayoutCraftApp, Tool};
use eframe::egui;

/// How near (screen px) a click must be to a point to choose it rather than add another.
const POINT_HIT: f32 = 8.0;

/// The chosen connection point: where and when it was chosen, its shape and its number. An edit or a page change after
/// that drops the choice, as the numbers may have moved.
pub(super) struct ChosenPoint {
    page: usize,
    depth: usize,
    id: String,
    number: usize,
    /// Where it was (scene px), to be sure the number still means it.
    at: egui::Pos2,
}

impl LayoutCraftApp {
    /// Take the tool, or put it away. It works on points, so no shape stays selected.
    pub(super) fn toggle_connection_point_tool(&mut self) {
        self.take_tool(if self.tool == Tool::ConnectionPoint { Tool::Select } else { Tool::ConnectionPoint });
    }

    /// Take `tool` in hand.
    pub(super) fn take_tool(&mut self, tool: Tool) {
        self.tool = tool;
        self.chosen_point = None;
        if tool == Tool::ConnectionPoint {
            self.selected = None;
            self.extra_selected.clear();
            self.status = "Click a shape to add a connection point; drag a point to move it, or click it and Delete removes it. Esc leaves.".to_string();
        }
    }

    /// The chosen point as long as nothing changed since it was chosen.
    pub(super) fn chosen_connection_point(&self) -> Option<(&str, usize)> {
        let chosen = self.chosen_point.as_ref()?;
        (chosen.page == self.page_index && chosen.depth == self.engine.undo_depth()).then_some((chosen.id.as_str(), chosen.number))
    }

    /// A click of the tool at the scene point `at`.
    pub(super) fn click_connection_point(&mut self, ctx: &egui::Context, at: egui::Pos2) {
        self.chosen_point = None;
        let reach = POINT_HIT / self.view_scale().max(0.01);
        // a point of the shape under the pointer, or of one whose edge the pointer is just beside
        let under = self.pick_shape(at).or_else(|| self.pick_shape_near(at));
        let Some(id) = under.filter(|id| self.doc_shape(id).is_some_and(|shape| shape.group.is_none())) else {
            self.status = "Click a shape to add a connection point.".to_string();
            return;
        };
        let near = self.sites_of(&id).into_iter().filter(|(_, point)| point.distance(at) <= reach).min_by(|a, b| a.1.distance(at).total_cmp(&b.1.distance(at)));
        if let Some((number, point)) = near {
            self.status = format!("Connection point {number} chosen: Delete removes it.");
            self.chosen_point = Some(ChosenPoint { page: self.page_index, depth: self.engine.undo_depth(), id, number, at: point });
            return;
        }
        let fraction = match self.box_fraction(&id, at) {
            Ok(fraction) => fraction,
            Err(why) => {
                self.status = why.to_string();
                return;
            }
        };
        let result = self.engine.page_op(24, self.page_index, 0, &format!("{id}|{}|{}", fraction.0, fraction.1)).map(|_| ());
        self.finish_edit(ctx, result, "Added a connection point", None);
    }

    /// The scene point `at` as fractions of the box of shape `id`, from its lower left corner and kept inside the box; or why
    /// the tool cannot place a point on that shape.
    fn box_fraction(&self, id: &str, at: egui::Pos2) -> Result<(f64, f64), &'static str> {
        if self.doc_shape(id).is_some_and(|shape| shape.rot.abs() >= 1e-6 || shape.flip_x || shape.flip_y) {
            return Err("This tool cannot yet place points on a turned or flipped shape.");
        }
        let shape = self.scene_shape(id).filter(|shape| shape.w > 1e-9 && shape.h > 1e-9).ok_or("This shape has no size to put a connection point on.")?;
        let across = (f64::from(at.x) - shape.x) / shape.w;
        let up = (shape.y + shape.h - f64::from(at.y)) / shape.h;
        Ok((across.clamp(0.0, 1.0), up.clamp(0.0, 1.0)))
    }

    /// A drag that began on a point moves it: the marker follows the pointer inside the shape's box, the release writes it.
    fn drag_connection_point(&mut self, response: &egui::Response, image_rect: egui::Rect) {
        let primary = egui::PointerButton::Primary;
        let held = response.ctx.input(|input| input.modifiers.command && input.modifiers.shift);
        if response.drag_started_by(primary) && !self.space_down && !held {
            let pressed = response.ctx.input(|input| input.pointer.press_origin());
            self.moving_point = pressed.and_then(|pointer| {
                let at = self.screen_to_scene(pointer, image_rect);
                let id = self.pick_shape(at).or_else(|| self.pick_shape_near(at)).filter(|id| self.doc_shape(id).is_some_and(|shape| shape.group.is_none()))?;
                let reach = POINT_HIT / self.view_scale().max(0.01);
                let (number, point) = self.sites_of(&id).into_iter().filter(|(_, point)| point.distance(at) <= reach).min_by(|a, b| a.1.distance(at).total_cmp(&b.1.distance(at)))?;
                Some(ChosenPoint { page: self.page_index, depth: self.engine.undo_depth(), id, number, at: point })
            });
        }
        if response.drag_stopped_by(primary) {
            let Some(moving) = self.moving_point.take() else { return };
            let Some(pointer) = response.interact_pointer_pos() else { return };
            // nothing else may have changed the drawing while the point was held
            if moving.page != self.page_index || moving.depth != self.engine.undo_depth() {
                return;
            }
            match self.box_fraction(&moving.id, self.screen_to_scene(pointer, image_rect)) {
                Ok(fraction) => {
                    self.chosen_point = None;
                    let result = self.engine.page_op(25, self.page_index, 0, &format!("{}|{}|{}|{}", moving.id, moving.number, fraction.0, fraction.1)).map(|_| ());
                    self.finish_edit(&response.ctx, result, "Moved the connection point", None);
                }
                Err(why) => self.status = why.to_string(),
            }
        } else if !response.ctx.input(|input| input.pointer.primary_down()) {
            self.moving_point = None;
        }
    }

    /// Delete the chosen point; connectors glued to it are unglued by the engine.
    pub(super) fn delete_connection_point(&mut self, ctx: &egui::Context) {
        let chosen = self.chosen_connection_point().map(|(id, number)| (id.to_string(), number));
        let was = self.chosen_point.take().map(|chosen| chosen.at);
        // undoing and editing again can give the same undo depth with other points: the point must still be where it was
        let Some((id, number)) = chosen.filter(|(id, number)| self.sites_of(id).iter().any(|(one, at)| one == number && was.is_some_and(|was| was.distance(*at) < 0.5))) else {
            self.status = "Click a connection point first, then Delete.".to_string();
            return;
        };
        let result = self.engine.page_op(26, self.page_index, 0, &format!("{id}|{number}")).map(|_| ());
        self.finish_edit(ctx, result, "Deleted the connection point", None);
    }

    /// While the tool is held: the points of the shape under the pointer and of the chosen one, the chosen one ringed.
    pub(super) fn show_connection_points(&mut self, response: &egui::Response, painter: &egui::Painter, image_rect: egui::Rect) {
        if self.tool != Tool::ConnectionPoint {
            self.moving_point = None;
            return;
        }
        self.drag_connection_point(response, image_rect);
        let chosen = self.chosen_connection_point().map(|(id, number)| (id.to_string(), number));
        let mut shown: Vec<String> = chosen.iter().map(|(id, _)| id.clone()).collect();
        if let Some(pointer) = response.hover_pos() {
            response.ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
            let at = self.screen_to_scene(pointer, image_rect);
            if let Some(id) = self.pick_shape(at).or_else(|| self.pick_shape_near(at)) {
                if !shown.contains(&id) {
                    shown.push(id);
                }
            }
        }
        let colour = egui::Color32::from_rgb(0, 100, 190);
        // where a point being moved would be let go
        if let Some((moving, pointer)) = self.moving_point.as_ref().zip(response.ctx.pointer_latest_pos()) {
            if let Ok((across, up)) = self.box_fraction(&moving.id, self.screen_to_scene(pointer, image_rect)) {
                if let Some(shape) = self.scene_shape(&moving.id) {
                    let to = egui::pos2((shape.x + across * shape.w) as f32, (shape.y + (1.0 - up) * shape.h) as f32);
                    painter.circle_stroke(self.scene_to_screen(to, image_rect), 5.0, egui::Stroke::new(2.0, colour));
                }
            }
        }
        for id in shown {
            for (number, point) in self.sites_of(&id) {
                let at = self.scene_to_screen(point, image_rect);
                painter.circle(at, 3.5, colour, egui::Stroke::new(1.0, egui::Color32::WHITE));
                // chosen: a ring as well, so that it does not rest on colour
                if chosen.as_ref().is_some_and(|(shape, one)| *shape == id && *one == number) {
                    painter.circle_stroke(at, 7.0, egui::Stroke::new(2.0, colour));
                }
            }
        }
    }
}
