//! Lines and arrows between shapes: the arrows around a shape under the pointer that add a connection, connectors with loose ends,
//! the line end (arrowhead) menu and reversing a connector's ends. The rules are the engine's; this file asks and draws.
use super::{Action, DragMode, DragState, LayoutCraftApp, Tool, MIN_SHAPE_PX};
use eframe::egui;
use layoutcraft_engine::{Direction, EndTarget};
use layoutcraft_render::SceneItem;

/// The line end styles offered, with the numbers the file stores (`BeginArrow` / `EndArrow`), as the browser shell lists them.
const LINE_ENDS: [(u32, &str); 9] = [
    (0, "None"),
    (1, "Open V"),
    (4, "Filled triangle"),
    (10, "Filled circle"),
    (11, "Filled square"),
    (14, "Hollow triangle"),
    (20, "Hollow circle"),
    (22, "Hollow diamond"),
    (27, "Crow's foot"),
];
const LINE_END_SIZES: [&str; 7] = ["Very small", "Small", "Medium", "Large", "Extra large", "Jumbo", "Colossal"];

/// How far outside a shape's edge its connect arrows sit (screen px).
const ARROW_GAP: f32 = 16.0;
/// The top arrow of the selected shape sits above its rotate handle.
const ARROW_GAP_OVER_ROTATE: f32 = 46.0;
/// The radius within which a connect arrow is hit, and half its drawn length.
const ARROW_HIT: f32 = 10.0;
const ARROW_SIZE: f32 = 6.0;
const DIRECTIONS: [Direction; 4] = [Direction::Right, Direction::Left, Direction::Up, Direction::Down];
/// How many shapes of the open stencil are offered beside a connect arrow, and how far from the arrow's middle.
const QUICK_SHAPES: usize = 4;
const PICKER_GAP: f32 = 14.0;

/// The list of shapes beside a connect arrow: whose arrow, which side, and where the list was drawn.
pub(super) struct ArrowPicker {
    id: String,
    direction: Direction,
    area: egui::Rect,
}

/// The line ends of the selected line as last read: (page, shape, undo depth) and the values.
pub(super) type LineEndsRead = ((usize, String, usize), serde_json::Value);

/// The unit step of `direction` on screen (y down).
fn screen_step(direction: Direction) -> egui::Vec2 {
    let (x, y) = direction.step();
    egui::vec2(x as f32, -y as f32)
}

/// Where the connect arrows of a shape shown at `rect` sit; `over_rotate` moves the top one clear of the rotate handle.
fn arrow_places(rect: egui::Rect, over_rotate: bool) -> [(Direction, egui::Pos2); 4] {
    DIRECTIONS.map(|direction| {
        let step = screen_step(direction);
        let half = egui::vec2(rect.width() / 2.0, rect.height() / 2.0);
        let gap = if over_rotate && direction == Direction::Up { ARROW_GAP_OVER_ROTATE } else { ARROW_GAP };
        (direction, rect.center() + egui::vec2(step.x * (half.x + gap), step.y * (half.y + gap)))
    })
}

/// The area in which the pointer keeps a shape's arrows shown: the shape and the room its arrows take.
fn arrow_area(rect: egui::Rect, over_rotate: bool) -> egui::Rect {
    arrow_places(rect, over_rotate).iter().fold(rect, |area, (_, at)| area.union(egui::Rect::from_center_size(*at, egui::Vec2::splat(2.0 * ARROW_HIT))))
}

fn direction_words(direction: Direction) -> &'static str {
    match direction {
        Direction::Right => "to the right",
        Direction::Left => "to the left",
        Direction::Up => "above",
        Direction::Down => "below",
    }
}

impl LayoutCraftApp {
    /// A connector drag (connector tool or a connect arrow) from shape `from` (empty: from the empty page) at `start` was released at
    /// `end` (scene px). Each end is glued to the shape under it, else left loose there. One undo step.
    pub(super) fn commit_connector(&mut self, ctx: &egui::Context, from: &str, start: egui::Pos2, end: egui::Pos2) {
        let from_site = self.connect_from_site.take();
        let target = self.connect_target(end);
        let to = target.as_ref().map(|(id, _, _)| id.clone());
        if !from.is_empty() && to.as_deref() == Some(from) {
            self.status = "Release on a different shape, or on the empty page.".to_string();
            return;
        }
        if (from.is_empty() || to.is_none()) && (end - start).length() < MIN_SHAPE_PX {
            self.status = "Drag a longer line.".to_string();
            return;
        }
        let page_height = self.page_height_in;
        let loose = |point: egui::Pos2| EndTarget::Free(f64::from(point.x) / super::DPI, page_height - f64::from(point.y) / super::DPI);
        let begin = if from.is_empty() { loose(start) } else { EndTarget::Glue(from.to_string(), from_site) };
        let finish = match &to {
            Some(shape) => EndTarget::Glue(shape.clone(), target.as_ref().and_then(|(_, _, site)| *site)),
            None => loose(self.snapped_point(end)),
        };
        let words = |shape: Option<&str>| shape.map_or("the page".to_string(), |id| format!("shape {id}"));
        let done = format!("Drew a connector from {} to {}", words((!from.is_empty()).then_some(from)), words(to.as_deref()));
        match self.engine.add_connector_between(self.page_index, &begin, &finish) {
            Ok(id) => {
                self.tool = Tool::Select;
                self.finish_edit(ctx, Ok(()), &done, Some(id));
            }
            Err(error) => self.status = format!("Could not connect: {error}"),
        }
    }

    /// Whether shape `id` gets connect arrows: a shape of the page itself (no connector, no member of a group).
    fn takes_connect_arrows(&self, id: &str) -> bool {
        self.doc_shape(id).is_some_and(|shape| shape.group.is_none()) && self.scene_shape(id).is_some()
    }

    fn connect_arrows_allowed(&self) -> bool {
        self.connect_arrows && self.tool == Tool::Select && !self.change_shape && !self.space_down && self.drag.is_none()
    }

    /// The shape's rectangle on screen and whether its rotate handle is shown above it.
    fn arrow_frame(&self, id: &str, image_rect: egui::Rect) -> Option<(egui::Rect, bool)> {
        let rect = self.scene_rect_to_screen(Self::selection_scene_rect(self.scene_shape(id)?), image_rect);
        Some((rect, self.selected.as_deref() == Some(id)))
    }

    /// The connect arrow under the screen point `pointer`, of the shape whose arrows are shown.
    pub(super) fn connect_arrow_at(&self, pointer: egui::Pos2, image_rect: egui::Rect) -> Option<(String, Direction)> {
        if !self.connect_arrows_allowed() {
            return None;
        }
        let id = self.arrow_shape.as_ref()?;
        let (rect, over_rotate) = self.arrow_frame(id, image_rect)?;
        arrow_places(rect, over_rotate).into_iter().find(|(_, at)| at.distance(pointer) <= ARROW_HIT).map(|(direction, _)| (id.clone(), direction))
    }

    /// A press on a connect arrow starts a connector from that side of its shape. `false`: no arrow there.
    pub(super) fn press_connect_arrow(&mut self, pointer: egui::Pos2, image_rect: egui::Rect) -> bool {
        let Some((id, direction)) = self.connect_arrow_at(pointer, image_rect) else { return false };
        let Some(rect) = self.scene_shape(&id).map(Self::selection_scene_rect) else { return false };
        // the connection point on that side when there is one near its middle, else the whole shape
        let step = screen_step(direction);
        let middle = rect.center() + egui::vec2(step.x * rect.width() / 2.0, step.y * rect.height() / 2.0);
        let reach = rect.width().max(rect.height()) * 0.15;
        let site = self.sites_of(&id).into_iter().filter(|(_, at)| at.distance(middle) <= reach).min_by(|a, b| a.1.distance(middle).total_cmp(&b.1.distance(middle)));
        self.connect_from_site = site.map(|(n, _)| n);
        let at = site.map_or(middle, |(_, at)| at);
        let start = egui::Rect::from_min_size(at, egui::Vec2::ZERO);
        self.drag = Some(DragState { visio_id: id, mode: DragMode::Connect, start_pointer: at, end_pointer: at, start_rect: start });
        self.preview = Some(start);
        self.status = "Release on a shape to connect to it, or on the empty page to leave the end loose.".to_string();
        true
    }

    /// A click on a connect arrow: the shape picked in the Shapes panel (else a copy of this one) is placed on that side and
    /// connected. `false`: no arrow there.
    pub(super) fn click_connect_arrow(&mut self, ctx: &egui::Context, pointer: egui::Pos2, image_rect: egui::Rect) -> bool {
        let Some((id, direction)) = self.connect_arrow_at(pointer, image_rect) else { return false };
        let picked = self.armed_master.take();
        self.place_neighbour(ctx, &id, direction, picked.as_ref().map(|(stencil, master, _)| (*stencil, master.as_str())));
        true
    }

    /// Place a shape (the master `pick`, else a copy of `id`) on that side of `id` and connect the two.
    pub(super) fn place_neighbour(&mut self, ctx: &egui::Context, id: &str, direction: Direction, pick: Option<(usize, &str)>) {
        match self.engine.connect_new_neighbour(self.page_index, id, direction, pick) {
            Ok(made) => {
                self.arrow_shape = None;
                self.arrow_picker = None;
                self.finish_edit(ctx, Ok(()), &format!("Placed a shape {} and connected it", direction_words(direction)), Some(made));
            }
            Err(error) => self.status = format!("Could not add a connected shape: {error}"),
        }
    }

    /// The shapes offered beside a connect arrow, the first ones of the open stencil: (stencil, master, name).
    fn quick_shapes(&self) -> Vec<(usize, String, String)> {
        let open = self.stencils.iter().find(|stencil| Some(stencil.index) == self.open_stencil_shown);
        open.map_or_else(Vec::new, |stencil| stencil.masters.iter().take(QUICK_SHAPES).map(|(master, name)| (stencil.index, master.clone(), name.clone())).collect())
    }

    /// Follow the pointer and draw the connect arrows of the shape under it.
    pub(super) fn show_connect_arrows(&mut self, response: &egui::Response, painter: &egui::Painter, image_rect: egui::Rect) {
        if !self.connect_arrows_allowed() {
            self.arrow_shape = None;
            self.arrow_picker = None;
            return;
        }
        // on the list of shapes, or on the way to it, the pointer keeps the list and its arrow (the list covers the canvas,
        // so the canvas itself is not hovered there)
        let on_picker = response.ctx.pointer_hover_pos().filter(|at| self.arrow_picker.as_ref().is_some_and(|picker| picker.area.expand(8.0).contains(*at)));
        // while a shape is dragged from the Shapes panel the canvas is not hovered, but the arrows show where it can be let go
        let dragged = response.ctx.pointer_latest_pos().filter(|at| self.pane_drag.is_some() && response.rect.contains(*at));
        let Some(pointer) = on_picker.or(dragged).or_else(|| response.hover_pos()) else {
            self.arrow_shape = None;
            self.arrow_picker = None;
            return;
        };
        // an arrow of the shown shape keeps it, even where it lies over a neighbour
        if on_picker.is_none() && self.connect_arrow_at(pointer, image_rect).is_none() {
            self.arrow_picker = None;
            let under = self.pick_shape(self.screen_to_scene(pointer, image_rect)).filter(|id| self.takes_connect_arrows(id));
            self.arrow_shape = under.or_else(|| {
                let id = self.arrow_shape.take()?;
                let (rect, over_rotate) = self.arrow_frame(&id, image_rect)?;
                (self.takes_connect_arrows(&id) && arrow_area(rect, over_rotate).contains(pointer)).then_some(id)
            });
        }
        let Some(id) = self.arrow_shape.clone() else { return };
        let Some((rect, over_rotate)) = self.arrow_frame(&id, image_rect) else { return };
        // a shape already being dragged needs no list of shapes
        let shapes = if self.pane_drag.is_some() { Vec::new() } else { self.quick_shapes() };
        let listed = on_picker.and(self.arrow_picker.as_ref().map(|picker| picker.direction));
        let mut offer = None;
        for (direction, at) in arrow_places(rect, over_rotate) {
            // under the pointer an arrow grows as well as changing colour
            let hot = listed.map_or_else(|| at.distance(pointer) <= ARROW_HIT, |chosen| chosen == direction);
            if hot && !shapes.is_empty() {
                offer = Some((direction, at));
            }
            let size = if hot { ARROW_SIZE * 1.5 } else { ARROW_SIZE };
            let step = screen_step(direction);
            let side = egui::vec2(-step.y, step.x);
            let points = vec![at + step * size, at - step * size + side * size, at - step * size - side * size];
            let colour = if hot { egui::Color32::from_rgb(200, 80, 0) } else { egui::Color32::from_rgb(0, 100, 190) };
            painter.add(egui::Shape::convex_polygon(points, colour, egui::Stroke::new(1.5, egui::Color32::WHITE)));
            if hot {
                response.ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                let what = if self.armed_master.is_some() { "place the picked shape" } else { "copy this shape" };
                // the list of shapes takes the place of the hint, which would cover it
                if shapes.is_empty() && self.pane_drag.is_none() {
                    response.clone().on_hover_text_at_pointer(format!("Click to {what} {} and connect them; drag to connect to a shape or to the page", direction_words(direction)));
                }
            }
        }
        let Some((direction, at)) = offer else {
            self.arrow_picker = None;
            return;
        };
        let pivot = match direction {
            Direction::Right => egui::Align2::LEFT_CENTER,
            Direction::Left => egui::Align2::RIGHT_CENTER,
            Direction::Up => egui::Align2::CENTER_BOTTOM,
            Direction::Down => egui::Align2::CENTER_TOP,
        };
        let mut chosen = None;
        // not kept inside the window: that would push it over the arrow near an edge, and the arrow's click would pick a shape
        let list = egui::Area::new(egui::Id::unique("quick-shapes")).order(egui::Order::Foreground).pivot(pivot).fixed_pos(at + screen_step(direction) * PICKER_GAP).constrain(false).show(&response.ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                for (stencil, master, name) in &shapes {
                    let hint = format!("Place {name} {} and connect it", direction_words(direction));
                    if ui.add(egui::Button::new(name).wrap_mode(egui::TextWrapMode::Extend)).on_hover_text(hint).clicked() {
                        chosen = Some((*stencil, master.clone()));
                    }
                }
            });
        });
        self.arrow_picker = Some(ArrowPicker { id, direction, area: list.response.rect });
        if let Some((stencil, master)) = chosen {
            if let Some(picker) = self.arrow_picker.take() {
                self.armed_master = None;
                self.place_neighbour(&response.ctx, &picker.id, picker.direction, Some((stencil, &master)));
                // the click was on the list; the drawing keeps the keyboard
                self.canvas_keys = true;
            }
        }
    }

    /// The selected shapes that are connectors.
    pub(super) fn selected_connectors(&self) -> Vec<String> {
        let routes = self.connector_routes();
        self.all_selected().into_iter().filter(|id| routes.iter().any(|(other, _)| other == id)).collect()
    }

    /// Reverse the ends of every selected connector, as one undo step.
    pub(super) fn reverse_selected_ends(&mut self, ctx: &egui::Context) {
        let connectors = self.selected_connectors();
        if connectors.is_empty() {
            self.status = "Select a connector first.".to_string();
            return;
        }
        let depth = self.engine.undo_depth();
        let result = connectors.iter().try_for_each(|id| self.engine.reverse_ends(self.page_index, id));
        if self.engine.undo_depth() > depth {
            self.engine.collapse_history(depth);
        }
        let primary = self.selected.clone();
        self.finish_edit(ctx, result, "Reversed the ends", primary);
    }

    /// Whether a canvas item can carry line ends: anything selected that is on the page.
    pub(super) fn can_set_line_ends(&self) -> bool {
        self.texture.is_some() && (!self.selected_connectors().is_empty() || self.scene.as_ref().is_some_and(|scene| {
            scene.items.iter().any(|item| matches!(item, SceneItem::Shape(shape) if shape.visio_id.is_some() && shape.visio_id == self.selected))
        }))
    }

    /// The "Line ends" menu: arrowheads and their sizes for the begin and the end of the selected lines.
    pub(super) fn line_ends_menu(&mut self, ui: &mut egui::Ui, action: &mut Option<Action>) {
        let enabled = self.can_set_line_ends();
        ui.add_enabled_ui(enabled, |ui| {
            ui.menu_button("Line ends \u{25be}", |ui| {
                let current = self.current_line_ends();
                for (title, style_key, size_key) in [("Start", "beginArrow", "beginArrowSize"), ("End", "endArrow", "endArrowSize")] {
                    ui.menu_button(format!("{title} arrowhead"), |ui| {
                        for (number, name) in LINE_ENDS {
                            if ui.radio(current[style_key].as_f64() == Some(f64::from(number)), name).clicked() {
                                *action = Some(Action::StyleSelected(serde_json::json!({ style_key: number }).to_string()));
                                ui.close();
                            }
                        }
                    });
                    ui.menu_button(format!("{title} size"), |ui| {
                        for (number, name) in LINE_END_SIZES.iter().enumerate() {
                            if ui.radio(current[size_key].as_f64() == Some(number as f64), *name).clicked() {
                                *action = Some(Action::StyleSelected(serde_json::json!({ size_key: number }).to_string()));
                                ui.close();
                            }
                        }
                    });
                }
            })
            .response
            .on_hover_text("Arrowheads at the start and end of the selected lines");
        });
    }

    /// The line end values of the primary selection, read again only after an edit or a selection change.
    fn current_line_ends(&mut self) -> serde_json::Value {
        let Some(id) = self.selected.clone() else { return serde_json::Value::Null };
        let key = (self.page_index, id, self.engine.undo_depth());
        if let Some((known, value)) = &self.line_ends_read {
            if *known == key {
                return value.clone();
            }
        }
        let value: serde_json::Value = self.engine.style_info_json(key.0, &key.1).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
        self.line_ends_read = Some((key, value.clone()));
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{pos2, vec2, Rect};

    /// Every arrow lies outside its shape, on its own side, clear of the resize handles, and inside the area that keeps them shown;
    /// the top arrow of the selected shape is clear of the rotate handle.
    #[test]
    fn connect_arrows_sit_outside_the_shape_and_stay_reachable() {
        for size in [vec2(8.0, 6.0), vec2(120.0, 60.0)] {
            let rect = Rect::from_min_size(pos2(200.0, 200.0), size);
            for over_rotate in [false, true] {
                for (direction, at) in arrow_places(rect, over_rotate) {
                    assert!(!rect.expand(ARROW_SIZE).contains(at), "{direction:?}");
                    assert!((at - rect.center()).dot(screen_step(direction)) > 0.0, "{direction:?}");
                    assert!(arrow_area(rect, over_rotate).contains(at));
                }
            }
            let rotate_handle = pos2(rect.center().x, rect.top() - super::super::ROTATE_STEM);
            let top = arrow_places(rect, true).into_iter().find(|(d, _)| *d == Direction::Up).unwrap().1;
            assert!(top.distance(rotate_handle) > ARROW_HIT + 9.0);
        }
    }
}
