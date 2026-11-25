//! Deterministic SVG output for a `Scene`.
mod bevel_outline;
use crate::scene::{
    Point, Scene, SceneBevel, SceneConnector, SceneHop, SceneFillPattern, SceneGradient, SceneItem,
    SceneParagraph, SceneReflection, SceneRun, SceneShadow, SceneShape, SceneSoftEdge, SceneText,
};

thread_local! {
    /// The line being written has right-to-left letters: its numbers are isolated (see `isolate_numbers`).
    static RTL_LINE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static FIT_TEXT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Device pixels per SVG unit: the hairline floor is one *device* pixel.
    static DEVICE_SCALE: std::cell::Cell<f64> = const { std::cell::Cell::new(1.0) };
}

pub fn to_svg(scene: &Scene) -> String {
    to_svg_scaled(scene, 1.0)
}

/// SVG for a raster that will be drawn at `scale` device pixels per unit, so hairlines keep
/// Visio's one-device-pixel minimum rather than one SVG unit.
pub fn to_svg_scaled(scene: &Scene, scale: f64) -> String {
    DEVICE_SCALE.with(|value| value.set(if scale.is_finite() && scale > 0.0 { scale } else { 1.0 }));
    FIT_TEXT.with(|flag| flag.set(scene.fit_text));
    let (pixel_w, pixel_h) = scene.raster_size();
    let (pixel_w, pixel_h) = (pixel_w.to_string(), pixel_h.to_string());
    let mut out = String::new();
    out.push_str(&format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{pixel_w}" height="{pixel_h}" viewBox="{} {} {} {}" role="img" aria-labelledby="vl-title">"#,
        num(scene.min_x),
        num(scene.min_y),
        num(scene.width),
        num(scene.height)
    ));
    out.push_str(&format!(r#"<title id="vl-title">{}</title>"#, escape(&scene.title)));
    let defs = image_defs(scene);
    if !defs.is_empty() {
        out.push_str("<defs>");
        out.push_str(&defs);
        out.push_str("</defs>");
    }
    out.push_str(&format!(
        r#"<rect x="{}" y="{}" width="{}" height="{}" fill="{}"/>"#,
        num(scene.min_x),
        num(scene.min_y),
        num(scene.width),
        num(scene.height),
        escape(&scene.background)
    ));
    for (index, item) in scene.items.iter().enumerate() {
        match item {
            SceneItem::Shape(shape) => match shape.visio_id.as_deref() {
                // A wrapper lets a host map a click back to the Visio shape (`data-vid`).
                Some(id) => {
                    match shape.alt_text.as_deref() {
                        Some(alt) => out.push_str(&format!(
                            r#"<g data-vid="{}" role="img" aria-label="{}"><title>{}</title>"#,
                            escape(id),
                            escape(alt),
                            escape(alt)
                        )),
                        None => out.push_str(&format!(r#"<g data-vid="{}">"#, escape(id))),
                    }
                    render_shape(&mut out, shape, index);
                    out.push_str("</g>");
                }
                None => render_shape(&mut out, shape, index),
            },
            SceneItem::Connector(connector) => match connector.visio_id.as_deref() {
                Some(id) => {
                    out.push_str(&format!(r#"<g data-cid="{}">"#, escape(id)));
                    render_connector(&mut out, connector);
                    out.push_str("</g>");
                }
                None => render_connector(&mut out, connector),
            },
        }
    }
    out.push_str("</svg>");
    out
}

fn image_defs(scene: &Scene) -> String {
    let mut out = String::new();
    for (index, item) in scene.items.iter().enumerate() {
        let SceneItem::Shape(shape) = item else {
            continue;
        };
        let Some(image) = &shape.image else {
            continue;
        };
        if image.data_uri.is_none() {
            continue;
        }
        if let Some(clip) = &image.clip_path {
            out.push_str(&format!(
                r#"<clipPath id="imgClip{index}"><path transform="translate({} {})" d="{}"/></clipPath>"#,
                num(shape.x),
                num(shape.y),
                escape(clip)
            ));
        } else {
            // A shape's Rounding cell rounds the corners of its picture as well.
            let rx = shape.rounding.max(0.0).min(shape.w.abs() / 2.0).min(shape.h.abs() / 2.0);
            let rounding = if shape.rounding > 0.1 { format!(r#" rx="{}""#, num(rx)) } else { String::new() };
            out.push_str(&format!(
                r#"<clipPath id="imgClip{index}"><rect x="{}" y="{}" width="{}" height="{}"{rounding}/></clipPath>"#,
                num(shape.x),
                num(shape.y),
                num(shape.w),
                num(shape.h)
            ));
        }
        if let Some(filter) = image_filter_def(index, image) {
            out.push_str(&filter);
        }
    }
    out
}

fn image_filter_attr(index: usize, image: &crate::scene::SceneImage) -> String {
    if image.adjust == Some(false) {
        return String::new();
    }
    let clamp = |value: f64, low: f64, high: f64| value.max(low).min(high);
    let brightness = clamp(image.brightness, -1.0, 1.0);
    let contrast = clamp(image.contrast, -0.99, 0.99);
    let gamma = if (image.gamma - 1.0).abs() > 1e-4 {
        Some(clamp(image.gamma, 0.1, 10.0))
    } else {
        None
    };
    if brightness == 0.0 && contrast == 0.0 && gamma.is_none() {
        return String::new();
    }
    format!(r#" filter="url(#imgAdj{index})""#)
}

fn image_filter_def(index: usize, image: &crate::scene::SceneImage) -> Option<String> {
    if image.adjust == Some(false) {
        return None;
    }
    let clamp = |value: f64, low: f64, high: f64| value.max(low).min(high);
    let brightness = clamp(image.brightness, -1.0, 1.0);
    let contrast = clamp(image.contrast, -0.99, 0.99);
    let gamma = if (image.gamma - 1.0).abs() > 1e-4 {
        Some(clamp(image.gamma, 0.1, 10.0))
    } else {
        None
    };
    if brightness == 0.0 && contrast == 0.0 && gamma.is_none() {
        return None;
    }
    let mut out = format!(
        r#"<filter id="imgAdj{index}" x="-10%" y="-10%" width="120%" height="120%" color-interpolation-filters="sRGB">"#
    );
    if brightness != 0.0 || contrast != 0.0 {
        let slope = if contrast < 0.0 {
            1.0 + contrast
        } else {
            1.0 / (1.0 - contrast)
        };
        let intercept = 0.5 * (1.0 - slope) + (brightness / 2.0) * (1.0 + slope);
        out.push_str("<feComponentTransfer>");
        for channel in ["R", "G", "B"] {
            out.push_str(&format!(
                r#"<feFunc{channel} type="linear" slope="{:.4}" intercept="{:.4}"/>"#,
                slope, intercept
            ));
        }
        out.push_str("</feComponentTransfer>");
    }
    if let Some(gamma) = gamma {
        out.push_str("<feComponentTransfer>");
        for channel in ["R", "G", "B"] {
            out.push_str(&format!(
                r#"<feFunc{channel} type="gamma" exponent="{:.4}"/>"#,
                1.0 / gamma
            ));
        }
        out.push_str("</feComponentTransfer>");
    }
    out.push_str("</filter>");
    Some(out)
}

fn render_shape(out: &mut String, shape: &SceneShape, index: usize) {
    let rotated = shape.rot.abs() > 1e-9;
    if rotated {
        out.push_str(&format!(
            r#"<g transform="rotate({} {} {})">"#,
            num(shape.rot),
            num(shape.x + shape.w / 2.0),
            num(shape.y + shape.h / 2.0)
        ));
    }
    if let Some(glow) = &shape.glow {
        render_glow(out, shape, glow.radius, &glow.color, glow.opacity, index);
    }
    if let Some(shadow) = &shape.shadow {
        render_shadow(out, shape, shadow, index);
    }
    if let Some(reflection) = &shape.reflection {
        render_reflection(out, shape, reflection, index);
    }
    if let Some(image) = &shape.image {
        // An OLE object keeps its own fill and outline behind the preview picture.
        if !shape.parts.is_empty() {
            out.push_str(&format!(r#"<g transform="translate({} {})">"#, num(shape.x), num(shape.y)));
            for part in &shape.parts {
                out.push_str(&format!(
                    r#"<path d="{}"{}/>"#,
                    escape(&part.d),
                    paint_attrs(
                        &part.fill,
                        part.fill_opacity,
                        &part.stroke,
                        part.stroke_opacity,
                        part.stroke_width,
                        part.dash.as_deref()
                    )
                ));
            }
            out.push_str("</g>");
        }
        if let Some(uri) = &image.data_uri {
            let opacity = (1.0 - image.transparency).clamp(0.0, 1.0);
            // A metafile picture (an SVG after conversion) fills its shape's box: Visio's crop cells
            // (ImgWidth/ImgOffset, 12 % margins around the WEBMAP icons) do not apply to it.
            let metafile = uri.starts_with("data:image/svg+xml");
            let x = if metafile { shape.x } else { shape.x + image.offset_x * shape.w };
            let y = if metafile { shape.y } else { shape.y + image.offset_y * shape.h };
            let w = if metafile { shape.w } else { (image.scale_x * shape.w).max(1.0) };
            let h = if metafile { shape.h } else { (image.scale_y * shape.h).max(1.0) };
            out.push_str(&format!(
                r#"<image x="{}" y="{}" width="{}" height="{}" preserveAspectRatio="none"{}{} clip-path="url(#imgClip{index})" href="{}"/>"#,
                num(x),
                num(y),
                num(w),
                num(h),
                if opacity < 1.0 {
                    format!(r#" opacity="{}""#, num(opacity))
                } else {
                    String::new()
                },
                image_filter_attr(index, image),
                escape(uri)
            ));
        } else {
            out.push_str(&format!(
                r##"<rect x="{}" y="{}" width="{}" height="{}" fill="#ececec" stroke="#a0a0a0"/>"##,
                num(shape.x),
                num(shape.y),
                num(shape.w),
                num(shape.h)
            ));
            let label = if image.unsupported {
                "IMAGE (unsupported)".to_string()
            } else if image.name.is_empty() {
                "IMAGE".to_string()
            } else {
                format!("IMAGE {}", image.name)
            };
            out.push_str(&format!(
                r##"<text x="{}" y="{}" text-anchor="middle" font-family="sans-serif" font-size="11" fill="#606060">{}</text>"##,
                num(shape.x + shape.w / 2.0),
                num(shape.y + shape.h / 2.0),
                escape(&label)
            ));
        }
    } else if shape.parts.is_empty() {
        out.push_str(&format!(
            r#"<rect x="{}" y="{}" width="{}" height="{}"{}"#,
            num(shape.x),
            num(shape.y),
            num(shape.w),
            num(shape.h),
            paint_attrs(
                &shape.fill,
                shape.fill_opacity,
                &shape.stroke,
                shape.stroke_opacity,
                shape.stroke_width,
                shape.dash.as_deref()
            )
        ));
        out.push_str("/>");
    } else {
        // Part geometry is stored in the shape's local box; the shape's x/y
        // is the page-space translation applied by the importer.
        let soft_edge = shape.soft_edge.as_ref().filter(|edge| edge.radius > 0.01);
        if let Some(edge) = soft_edge {
            out.push_str(&soft_edge_defs(edge, index));
        }
        let rounding = shape.rounding.max(0.0);
        let rounded = rounding > 0.1 && !shape.parts.is_empty();
        // Several closed polygons that are not the box itself (the three stacked pages of a "Page
        // group") each get their own rounded corners; one clip to the box would cut them away.
        let round_each = rounded
            && shape.parts.len() > 1
            && shape.parts.iter().all(|part| polygon_points(&part.d).is_some())
            && shape.parts.iter().any(|part| part.d != shape.parts[0].d);
        let rounded = rounded && !round_each;
        let rx = rounding
            .min(shape.w.abs() / 2.0)
            .min(shape.h.abs() / 2.0);
        if rounded {
            out.push_str(&format!(
                r#"<defs><clipPath id="round{index}"><rect x="0" y="0" width="{}" height="{}" rx="{}"/></clipPath></defs>"#,
                num(shape.w),
                num(shape.h),
                num(rx)
            ));
        }
        out.push_str(&format!(
            r#"<g transform="translate({} {})"{}{}>"#,
            num(shape.x),
            num(shape.y),
            soft_edge
                .map(|_| format!(r#" filter="url(#soft{index})""#))
                .unwrap_or_default(),
            if rounded {
                format!(r#" clip-path="url(#round{index})""#)
            } else {
                String::new()
            }
        ));
        for (part_index, part) in shape.parts.iter().enumerate() {
            let mut fill = part.fill.clone();
            let mut stroke = part.stroke.clone();
            if let Some(pattern) = &part.fill_pattern {
                let id = format!("pat{index}p{part_index}");
                out.push_str(&pattern_defs(&id, pattern));
                fill = format!("url(#{id})");
            }
            if let Some(gradient) = &part.fill_gradient {
                let id = format!("grad{index}p{part_index}");
                out.push_str(&gradient_defs(&id, gradient, crate::scene::path_extent(&part.d)));
                fill = format!("url(#{id})");
            }
            if let Some(gradient) = &part.stroke_gradient {
                let id = format!("grad{index}p{part_index}s");
                out.push_str(&gradient_defs(&id, gradient, crate::scene::path_extent(&part.d)));
                stroke = format!("url(#{id})");
            }
            out.push_str(&format!(
                r#"<path d="{}"{}"#,
                escape(&if round_each { round_polygon(&part.d, rounding) } else { part.d.clone() }),
                paint_attrs(
                    &fill,
                    part.fill_opacity,
                    &stroke,
                    part.stroke_opacity,
                    part.stroke_width,
                    part.dash.as_deref()
                )
            ));
            out.push_str("/>");
            render_part_arrows(out, part);
        }
        if let Some(shadow) = shape.shadow.as_ref().filter(|shadow| shadow.kind == 3) {
            out.push_str(&inner_shadow_overlay(shape, shadow, index));
        }
        if let Some(bevel) = &shape.bevel {
            out.push_str(&bevel_overlay(shape, bevel, index));
        }
        out.push_str("</g>"); 
        // The outline of a rounded shape: its own line, or (a master whose shape-level line is unset)
        // the line of the part that traces the whole box.
        let frame = if shape.stroke != "none" && shape.stroke_width > 0.0 && shape.stroke_opacity > 0.0 {
            Some((shape.stroke.clone(), shape.stroke_opacity, shape.stroke_width, shape.dash.clone()))
        } else {
            shape
                .parts
                .iter()
                .find(|part| {
                    part.stroke != "none"
                        && part.stroke_width > 0.0
                        && part.stroke_opacity > 0.0
                        && path_is_box(&part.d, shape.w, shape.h)
                })
                .map(|part| (part.stroke.clone(), part.stroke_opacity, part.stroke_width, part.dash.clone()))
        };
        // The frame only closes the corners of geometry that runs into them: a diamond (a Lucidchart decision) has no
        // square corner for the clip to cut, and Visio draws no box around it.
        let reaches_corner = shape.parts.iter().any(|part| path_reaches_box_corner(&part.d, shape.w, shape.h));
        if let (true, Some((frame_stroke, frame_opacity, frame_width, frame_dash))) = (rounded && reaches_corner, frame) {
            // The geometry is square; the clip rounds the fills and trims the
            // straight border, and this rounded outline closes the corners.
            out.push_str(&format!(
                r#"<g transform="translate({} {})"><rect x="0" y="0" width="{}" height="{}" rx="{}"{}/></g>"#,
                num(shape.x),
                num(shape.y),
                num(shape.w),
                num(shape.h),
                num(rx),
                paint_attrs(
                    "none",
                    1.0,
                    &frame_stroke,
                    frame_opacity,
                    frame_width,
                    frame_dash.as_deref()
                )
            ));
        }
    }
    if let Some(text) = &shape.text {
        render_text_block(out, text, shape.text_rot);
    }
    for text in &shape.texts {
        render_text_block(out, text, shape.text_rot + text.rot);
    }
    if rotated {
        out.push_str("</g>");
    }
}

/// Render a text block, counter-rotating it by `text_rot` around its own
/// centre when the shape's `TxtAngle` asks for it. The enclosing group already
/// rotates the shape by `rot`, so the net text rotation is `rot + text_rot`.
fn render_text_block(out: &mut String, text: &SceneText, text_rot: f64) {
    if text_rot.abs() > 1e-9 {
        out.push_str(&format!(
            r#"<g transform="rotate({} {} {})">"#,
            num(text_rot.to_degrees()),
            num(text.x + text.w / 2.0),
            num(text.y + text.h / 2.0)
        ));
        render_text(out, text);
        out.push_str("</g>");
    } else {
        render_text(out, text);
    }
}

fn soft_edge_defs(edge: &SceneSoftEdge, index: usize) -> String {
    format!(
        r#"<defs><filter id="soft{index}" x="-25%" y="-25%" width="150%" height="150%"><feGaussianBlur stdDeviation="{}"/></filter></defs>"#,
        num(edge.radius * 0.28)
    )
}

/// Glow is drawn as a wide blurred stroke around the same silhouette; the
/// original shape paints over the inner half.
fn render_glow(
    out: &mut String,
    shape: &SceneShape,
    radius: f64,
    color: &str,
    opacity: f64,
    index: usize,
) {
    if radius <= 0.01 || opacity <= 0.0 {
        return;
    }
    out.push_str(&format!(
        r#"<defs><filter id="glow{index}" x="-50%" y="-50%" width="200%" height="200%"><feGaussianBlur stdDeviation="{}"/></filter></defs>"#,
        num(radius * 0.32)
    ));
    out.push_str(&format!(
        r#"<g transform="translate({} {})" filter="url(#glow{index})" opacity="{}">"#,
        num(shape.x),
        num(shape.y),
        num(opacity)
    ));
    if shape.parts.is_empty() {
        out.push_str(&format!(
            r#"<rect x="0" y="0" width="{}" height="{}" fill="none" stroke="{}" stroke-width="{}"/>"#,
            num(shape.w),
            num(shape.h),
            escape(color),
            num(radius * 0.8)
        ));
    } else {
        for part in &shape.parts {
            out.push_str(&format!(
                r#"<path d="{}" fill="none" stroke="{}" stroke-width="{}" stroke-linejoin="round"/>"#,
                escape(&part.d),
                escape(color),
                num(radius * 0.8)
            ));
        }
    }
    out.push_str("</g>");
}

/// Mirror the shape below its bottom edge, clipped to the requested fraction
/// of the shape height and optionally blurred.
fn render_reflection(
    out: &mut String,
    shape: &SceneShape,
    reflection: &SceneReflection,
    index: usize,
) {
    if reflection.size <= 0.001 || reflection.opacity <= 0.0 {
        return;
    }
    let height = (reflection.size * shape.h).max(1.0);
    let top = shape.h + reflection.distance;
    out.push_str(&format!(
        r##"<defs><linearGradient id="reflFade{index}" gradientUnits="userSpaceOnUse" x1="0" y1="{}" x2="0" y2="{}"><stop offset="0" stop-color="#ffffff" stop-opacity="{}"/><stop offset="1" stop-color="#ffffff" stop-opacity="0"/></linearGradient><mask id="reflMask{index}" maskUnits="userSpaceOnUse" x="{}" y="{}" width="{}" height="{}"><rect x="{}" y="{}" width="{}" height="{}" fill="url(#reflFade{index})"/></mask></defs>"##,
        num(top),
        num(top + height),
        num(reflection.opacity),
        num(-shape.w),
        num(top),
        num(shape.w * 3.0),
        num(height),
        num(-shape.w),
        num(top),
        num(shape.w * 3.0),
        num(height)
    ));
    if reflection.blur > 0.01 {
        out.push_str(&format!(
            r#"<defs><filter id="reflBlur{index}" x="-50%" y="-50%" width="200%" height="200%"><feGaussianBlur stdDeviation="{}"/></filter></defs>"#,
            num(reflection.blur)
        ));
    }
    let filter = if reflection.blur > 0.01 {
        format!(r#" filter="url(#reflBlur{index})""#)
    } else {
        String::new()
    };
    out.push_str(&format!(
        r#"<g transform="translate({} {})"><g mask="url(#reflMask{index})"><g transform="translate(0 {}) scale(1 -1)"{}>"#,
        num(shape.x),
        num(shape.y),
        num(2.0 * shape.h + reflection.distance),
        filter
    ));
    if shape.parts.is_empty() {
        out.push_str(&format!(
            r#"<rect x="0" y="0" width="{}" height="{}"{}"/>"#,
            num(shape.w),
            num(shape.h),
            paint_attrs(
                &shape.fill,
                shape.fill_opacity,
                &shape.stroke,
                shape.stroke_opacity,
                shape.stroke_width,
                shape.dash.as_deref()
            )
        ));
    } else {
        for part in &shape.parts {
            out.push_str(&format!(
                r#"<path d="{}"{}"#,
                escape(&part.d),
                paint_attrs(
                    &part.fill,
                    part.fill_opacity,
                    &part.stroke,
                    part.stroke_opacity,
                    part.stroke_width,
                    part.dash.as_deref()
                )
            ));
            out.push_str("/>");
        }
    }
    out.push_str("</g></g></g>");
}

/// Approximate a Visio bevel as a clipped gradient ring around the shape
/// bounding box, with a highlighted inset edge for the styles that show a
/// second contour.
fn bevel_overlay(shape: &SceneShape, bevel: &SceneBevel, index: usize) -> String {
    if let Some(profile) = bevel_profile_overlay(shape, bevel, index) {
        return profile;
    }
    if let Some(profile) = bevel_outline::bevel_outline_overlay(shape, bevel, index) {
        return profile;
    }
    let band = bevel
        .width
        .max(bevel.height)
        .min(shape.w / 2.0)
        .min(shape.h / 2.0);
    if band < 1.0 || shape.w < 2.0 || shape.h < 2.0 {
        return String::new();
    }
    let clip = if shape.parts.is_empty() {
        format!(
            r#"<clipPath id="bevelClip{index}"><rect x="0" y="0" width="{}" height="{}"/></clipPath>"#,
            num(shape.w),
            num(shape.h)
        )
    } else {
        let mut paths = String::new();
        for part in &shape.parts {
            paths.push_str(&format!(r#"<path d="{}"/>"#, escape(&part.d)));
        }
        format!(r#"<clipPath id="bevelClip{index}">{paths}</clipPath>"#)
    };
    let (x1, y1, x2, y2) = if bevel.kind == 7 {
        (1.0, 1.0, 0.0, 0.0)
    } else {
        (0.0, 0.0, 1.0, 1.0)
    };
    let highlight = if matches!(bevel.kind, 3 | 4 | 7 | 8 | 10 | 11 | 12) {
        format!(
            r##"<rect x="{}" y="{}" width="{}" height="{}" fill="none" stroke="#ffffff" stroke-opacity="0.45" stroke-width="1"/>"##,
            num(band),
            num(band),
            num(shape.w - 2.0 * band),
            num(shape.h - 2.0 * band)
        )
    } else {
        String::new()
    };
    let ring = format!(
        r#"<path d="M0 0H{0}V{1}H0Z M{2} {2}H{3}V{4}H{2}Z" fill="url(#bevelGrad{index})" fill-rule="evenodd" clip-path="url(#bevelClip{index})"/>"#,
        num(shape.w),
        num(shape.h),
        num(band),
        num(shape.w - band),
        num(shape.h - band)
    );
    format!(
        r##"<defs>{clip}<linearGradient id="bevelGrad{index}" x1="{}" y1="{}" x2="{}" y2="{}"><stop offset="0" stop-color="#ffffff" stop-opacity="0.75"/><stop offset="0.5" stop-color="#ffffff" stop-opacity="0"/><stop offset="1" stop-color="#000000" stop-opacity="0.4"/></linearGradient></defs>{ring}{highlight}"##,
        num(x1),
        num(y1),
        num(x2),
        num(y2)
    )
}

/// A bevel on a plain rectangle drawn from Visio's measured shading (`bevel_table`): per side a mitred band across the
/// bevel width with a gradient whose stops are the fill colour times Visio's lighting gain at that distance from the outline
/// (Visio lights by multiplying the colour: the centre is 104 %, an edge highlight up to 160 %), and the interior lit by
/// the interior gain. A fill that is not a plain colour gets the same shading as a white / black overlay. The profile is
/// stretched to the bevel width; its strength is the one of a bevel as high as it is wide (the table was measured at
/// 6 pt x 6 pt). `None` for other shapes and for rounded corners.
const BEVEL_RATIO_POWER: f64 = 0.75;

fn bevel_profile_overlay(shape: &SceneShape, bevel: &SceneBevel, index: usize) -> Option<String> {
    use crate::bevel_table::{BEVEL_GAIN, BEVEL_INTERIOR_GAIN, BEVEL_SAMPLES};
    if !(1..=12).contains(&bevel.kind) || shape.w < 6.0 || shape.h < 6.0 || shape.rounding > 0.1 {
        return None;
    }
    let plain = match shape.parts.as_slice() {
        [] => true,
        [part] => path_is_box(&part.d, shape.w, shape.h),
        _ => false,
    };
    if !plain {
        return None;
    }
    let (w, h) = (shape.w, shape.h);
    let width = bevel.width.clamp(2.0, w.min(h) / 2.0);
    let fill = shape.parts.first().map_or(shape.fill.as_str(), |part| part.fill.as_str());
    let rgb = (fill.len() == 7 && fill.starts_with('#'))
        .then(|| (u8::from_str_radix(&fill[1..3], 16), u8::from_str_radix(&fill[3..5], 16), u8::from_str_radix(&fill[5..7], 16)))
        .and_then(|(r, g, b)| Some((r.ok()?, g.ok()?, b.ok()?)));
    let table = &BEVEL_GAIN[bevel.kind as usize - 1];
    // a lower bevel (height under width) is flatter: its contrast shrinks with the ratio
    let strength = (bevel.height / bevel.width.max(0.1)).clamp(0.05, 1.0).powf(BEVEL_RATIO_POWER);
    // (colour, opacity) of a lighting gain in percent
    let paint = |gain: u16| -> (String, f64) {
        let gain = (100.0 + (gain as f64 - 100.0) * strength).round().max(0.0) as u16;
        match rgb {
            Some((r, g, b)) => {
                let scale = |c: u8| ((c as f64 * gain as f64 / 100.0).round().clamp(0.0, 255.0)) as u8;
                (format!("#{:02x}{:02x}{:02x}", scale(r), scale(g), scale(b)), 1.0)
            }
            None if gain >= 100 => ("#ffffff".to_string(), ((gain - 100) as f64 / 100.0 * 0.55).min(0.9)),
            None => ("#000000".to_string(), (100 - gain) as f64 / 100.0),
        }
    };
    let stops = |side: usize| -> String {
        let mut out = String::new();
        let mut push = |offset: f64, gain: u16| {
            let (colour, opacity) = paint(gain);
            out.push_str(&format!(r#"<stop offset="{}" stop-color="{colour}" stop-opacity="{}"/>"#, num(offset), num(opacity)));
        };
        // the table starts 6 px (of 24) in, under the outline: the first value is held back to the edge
        push(0.0, table[side][0]);
        for (k, gain) in table[side].iter().enumerate().take(BEVEL_SAMPLES) {
            push((6 + 2 * k) as f64 / 24.0, *gain);
        }
        out
    };
    // (polygon, gradient start, gradient end) for top, left, bottom, right
    let bands = [
        (format!("0,0 {w},0 {},{width} {width},{width}", w - width), (0.0, 0.0), (0.0, width)),
        (format!("0,0 {width},{width} {width},{} 0,{h}", h - width), (0.0, 0.0), (width, 0.0)),
        (format!("0,{h} {width},{} {},{} {w},{h}", h - width, w - width, h - width), (0.0, h), (0.0, h - width)),
        (format!("{w},0 {w},{h} {},{} {},{width}", w - width, h - width, w - width), (w, 0.0), (w - width, 0.0)),
    ];
    let (interior, interior_opacity) = paint(BEVEL_INTERIOR_GAIN);
    let mut defs = String::new();
    let mut body = format!(
        r#"<rect x="{0}" y="{0}" width="{1}" height="{2}" fill="{interior}" fill-opacity="{3}"/>"#,
        num(width),
        num(w - 2.0 * width),
        num(h - 2.0 * width),
        num(interior_opacity)
    );
    for (side, (points, from, to)) in bands.iter().enumerate() {
        defs.push_str(&format!(
            r#"<linearGradient id="bevelP{index}s{side}" gradientUnits="userSpaceOnUse" x1="{}" y1="{}" x2="{}" y2="{}">{}</linearGradient>"#,
            num(from.0),
            num(from.1),
            num(to.0),
            num(to.1),
            stops(side)
        ));
        body.push_str(&format!(r#"<polygon points="{points}" fill="url(#bevelP{index}s{side})"/>"#));
    }
    // the outline is drawn again over the bands (Visio draws the line on top of the bevel)
    let outline = match shape.parts.as_slice() {
        [part] if part.stroke != "none" && part.stroke_width > 0.0 => {
            format!(r#"<path d="{}" fill="none" stroke="{}" stroke-width="{}"/>"#, escape(&part.d), escape(&part.stroke), num(part.stroke_width))
        }
        _ => String::new(),
    };
    Some(format!("<defs>{defs}</defs>{body}{outline}"))
}

/// A hatch tile drawn pixel by pixel: one device pixel per character, `#` is foreground.
fn pixel_tile(rows: &[&str], fore: &str) -> String {
    let mut d = String::new();
    for (y, row) in rows.iter().enumerate() {
        let bytes = row.as_bytes();
        let mut x = 0;
        while x < bytes.len() {
            if bytes[x] == b'#' {
                let start = x;
                while x < bytes.len() && bytes[x] == b'#' {
                    x += 1;
                }
                d.push_str(&format!("M{start} {y}h{}v1h-{}z", x - start, x - start));
            } else {
                x += 1;
            }
        }
    }
    format!(r#"<path d="{d}" fill="{fore}"/>"#)
}

fn pattern_defs(id: &str, pattern: &SceneFillPattern) -> String {
    let fore = escape(&pattern.fore);
    let back = escape(&pattern.back);
    let mut body = String::new();
    let (width, height) = match pattern.kind {
        4 => (8.0, 8.0),
        6 | 7 => (8.0, 8.0),
        13 | 14 | 15 | 16 | 17 | 18 | 24 => (4.0, 4.0),
        8 | 10 => (4.0, 2.0),
        9 => (2.0, 2.0),
        11 => (4.0, 4.0),
        12 => (8.0, 4.0),
        19..=23 => (4.0, 4.0),
        _ => (8.0, 8.0),
    };
    match pattern.kind {
        2 => body.push_str(&format!(
            r#"<path d="M0 {}L{} 0" stroke="{}" stroke-width="1"/>"#,
            num(height),
            num(width),
            fore
        )),
        5 => body.push_str(&format!(
            r#"<path d="M0 0L{} {}" stroke="{}" stroke-width="1"/>"#,
            num(width),
            num(height),
            fore
        )),
        // Horizontal / vertical lines (measured on Visio's 300 dpi export): 6 and 7 are one device
        // pixel every 8, 13 and 14 two pixels every 4.
        6 | 13 => {
            let thickness = if pattern.kind == 13 { 2.0 } else { 1.0 };
            body.push_str(&format!(
                r#"<rect y="{}" width="{}" height="{}" fill="{}"/>"#,
                num(if pattern.kind == 13 { 0.0 } else { 4.0 }),
                num(width),
                num(thickness),
                fore
            ));
        }
        7 | 14 => {
            let thickness = if pattern.kind == 14 { 2.0 } else { 1.0 };
            body.push_str(&format!(
                r#"<rect x="{}" width="{}" height="{}" fill="{}"/>"#,
                num(if pattern.kind == 14 { 0.0 } else { 7.0 }),
                num(thickness),
                num(height),
                fore
            ));
        }
        3 | 23 => body.push_str(&format!(
            r#"<path d="M0 0.5H{}M0.5 0V{}" stroke="{}" stroke-width="1"/>"#,
            num(width),
            num(height),
            fore
        )),
        // Pixel tiles measured on Visio's 300 dpi export (`#` = foreground device pixel).
        4 => body.push_str(&pixel_tile(
            &["....#.#.", "...#...#", "#.#.....", ".#......", "#.#.....", "...#...#", "....#.#.", ".....#.."],
            &fore,
        )),
        15 => body.push_str(&pixel_tile(&["#..#", "##..", ".##.", "..##"], &fore)),
        16 => body.push_str(&pixel_tile(&["##..", "#..#", "..##", ".##."], &fore)),
        17 => body.push_str(&pixel_tile(&["##..", "..##", "..##", "##.."], &fore)),
        18 => body.push_str(&pixel_tile(&["####", ".##.", "####", "#..#"], &fore)),
        24 => body.push_str(&pixel_tile(&["###.", ".#.#", "#.##", ".#.#"], &fore)),
        // Dither patterns 8-12 are single-device-pixel tiles (measured on Visio's 300 dpi export:
        // 8 is 75 % foreground, 9 a checkerboard, 10 25 %, 11 12.5 %, 12 6.25 %).
        8..=12 => {
            let (base, dot, pixels): (&str, &str, &[(u32, u32)]) = match pattern.kind {
                8 => (fore.as_str(), back.as_str(), &[(0, 0), (2, 1)]),
                9 => (back.as_str(), fore.as_str(), &[(1, 0), (0, 1)]),
                10 => (back.as_str(), fore.as_str(), &[(0, 0), (2, 1)]),
                11 => (back.as_str(), fore.as_str(), &[(0, 0), (2, 2)]),
                _ => (back.as_str(), fore.as_str(), &[(3, 0), (7, 2)]),
            };
            body.push_str(&format!(r#"<rect width="{}" height="{}" fill="{base}"/>"#, num(width), num(height)));
            for (x, y) in pixels {
                body.push_str(&format!(r#"<rect x="{x}" y="{y}" width="1" height="1" fill="{dot}"/>"#));
            }
        }
        21 => body.push_str(&pixel_tile(&["...#", "#...", ".#..", "..#."], &fore)),
        22 => body.push_str(&pixel_tile(&[".#..", "#...", "...#", "..#."], &fore)),
        19 => body.push_str(&format!(
            r#"<path d="M0 0.5H{}" stroke="{}" stroke-width="1"/>"#,
            num(width),
            fore
        )),
        20 => body.push_str(&format!(
            r#"<path d="M0.5 0V{}" stroke="{}" stroke-width="1"/>"#,
            num(height),
            fore
        )),
        _ => {}
    }
    // Visio's hatch tile is a fixed number of device pixels (8 at its 300 dpi export), so it does not
    // grow with the render scale: shrink the tile by the device scale (1 for plain SVG, unchanged).
    let device = DEVICE_SCALE.with(|value| value.get());
    let transform = if (device - 1.0).abs() > 1e-9 {
        format!(r#" patternTransform="scale({})""#, num(1.0 / device))
    } else {
        String::new()
    };
    format!(
        r#"<defs><pattern id="{}" patternUnits="userSpaceOnUse"{} width="{}" height="{}"><rect width="{}" height="{}" fill="{}"/>{}</pattern></defs>"#,
        id,
        transform,
        num(width),
        num(height),
        num(width),
        num(height),
        back,
        body
    )
}

/// Visio does not blend a gradient linearly: measured against its own exports (black to white,
/// linear, radial and rectangular presets) the blend factor follows an ease-out curve,
/// `a + (1 - a) * (1 - (1 - x)^p)`, where `x` is the fraction of the way along the gradient
/// (least-squares fit over 94 samples, rms 3.6 of 255).
const GRADIENT_SAMPLES: usize = 16;
/// Exponent of the far-end ease of corner-anchored rectangular gradients (types 8, 9, 11, 12):
/// the end colour's share grows as x^p from the origin. ENTAPP laptop screens fit 1.9, NETLOC
/// House 1.2 (0.0905; 1.5 gives 0.0965, 1.9 gives 0.103), MRKTDG matrices do not care: 1.2.
const FAR_END_EASE: f64 = 1.2;

/// A gradient with two stops (measured against Visio's PNG export of white -> blue, black -> white and red -> blue boxes,
/// `make-gradient-types.mjs`, `gradpos-probe.py`): the ramp is laid over the shape grown by `GRADIENT_OVERRUN` on each side, so
/// the parameter of a point is `(x + a) / (1 + 2a)`. When the stops cover the whole range (first at 0, last at 1) the colours are
/// blended in LINEAR LIGHT along a smoothstep of that parameter; with any other positions the blend is a plain linear one in sRGB.
/// (The older ease-out fit, `1 - (1 - x)^1.8` with a floor, was this curve seen through black -> white only.)
const GRADIENT_OVERRUN: f64 = 0.0245;

fn gradient_parameter(x: f64) -> f64 {
    ((x.clamp(0.0, 1.0) + GRADIENT_OVERRUN) / (1.0 + 2.0 * GRADIENT_OVERRUN)).clamp(0.0, 1.0)
}

fn smoothstep(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn srgb_to_linear(c: f64) -> f64 {
    let c = c / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn linear_to_srgb(l: f64) -> f64 {
    255.0 * if l <= 0.0031308 { l * 12.92 } else { 1.055 * l.powf(1.0 / 2.4) - 0.055 }
}

/// Visio's legacy gradient fill patterns (25-40) blend along a smoothstep curve (fitted to a vertical
/// gradient, pattern 28, over 100 samples: rms 0.01 of the range): slow at both ends, centred a
/// little past the middle.
fn smooth_gradient(x: f64) -> f64 {
    let t = ((x - 0.045) / 0.97).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn hex_rgb(color: &str) -> Option<(f64, f64, f64)> {
    let hex = color.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let channel = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).ok().map(f64::from);
    Some((channel(0)?, channel(2)?, channel(4)?))
}

/// The stops resampled through the ease-out curve, so a plain SVG gradient reproduces Visio's
/// blend. Stops whose colours are not `#rrggbb` are passed through unchanged.
fn eased_stops(stops: &[crate::scene::SceneGradientStop], smooth: bool) -> Vec<(f64, String, f64)> {
    eased_stops_n(stops, GRADIENT_SAMPLES, smooth, false)
}

fn eased_stops_n(stops: &[crate::scene::SceneGradientStop], samples: usize, smooth: bool, from_far_end: bool) -> Vec<(f64, String, f64)> {
    let mut sorted: Vec<&crate::scene::SceneGradientStop> = stops.iter().collect();
    sorted.sort_by(|a, b| a.position.partial_cmp(&b.position).unwrap_or(std::cmp::Ordering::Equal));
    let plain = || {
        stops
            .iter()
            .map(|stop| (stop.position.clamp(0.0, 1.0), stop.color.clone(), stop.opacity.clamp(0.0, 1.0)))
            .collect()
    };
    if sorted.len() < 2 || sorted.iter().any(|stop| hex_rgb(&stop.color).is_none()) {
        return plain();
    }
    let full_span = sorted.len() == 2 && sorted[0].position <= 0.001 && sorted[1].position >= 0.999;
    let at = |t: f64| -> (String, f64) {
        let t = t.clamp(0.0, 1.0);
        let mut lower = sorted[0];
        let mut upper = sorted[sorted.len() - 1];
        for pair in sorted.windows(2) {
            if t >= pair[0].position && t <= pair[1].position {
                lower = pair[0];
                upper = pair[1];
                break;
            }
        }
        if t <= sorted[0].position {
            upper = sorted[0];
            lower = sorted[0];
        }
        let span = (upper.position - lower.position).max(1e-9);
        let f = if upper.position == lower.position { 0.0 } else { ((t - lower.position) / span).clamp(0.0, 1.0) };
        let (r0, g0, b0) = hex_rgb(&lower.color).unwrap_or((0.0, 0.0, 0.0));
        let (r1, g1, b1) = hex_rgb(&upper.color).unwrap_or((0.0, 0.0, 0.0));
        let mix = |a: f64, b: f64| {
            if full_span {
                linear_to_srgb(srgb_to_linear(a) + (srgb_to_linear(b) - srgb_to_linear(a)) * f).round().clamp(0.0, 255.0) as u8
            } else {
                (a + (b - a) * f).round().clamp(0.0, 255.0) as u8
            }
        };
        (
            format!("#{:02x}{:02x}{:02x}", mix(r0, r1), mix(g0, g1), mix(b0, b1)),
            lower.opacity + (upper.opacity - lower.opacity) * f,
        )
    };
    (0..=samples)
        .map(|k| {
            let x = k as f64 / samples as f64;
            let (color, opacity) = at(if from_far_end && full_span {
                smoothstep(gradient_parameter(x))
            } else if from_far_end {
                // Eased from the far end: the share of the end colour grows as x^p from the origin.
                x.powf(FAR_END_EASE)
            } else if smooth {
                smooth_gradient(x)
            } else if full_span {
                smoothstep(gradient_parameter(x))
            } else if sorted.len() == 2 {
                gradient_parameter(x)
            } else {
                // custom gradients with more stops blend linearly between neighbouring stops (a probe of a red / green /
                // blue gradient: the green stop at 50 % is pure green at half way and the segments are linear)
                x
            });
            (x, color, opacity.clamp(0.0, 1.0))
        })
        .collect()
}

fn gradient_defs(id: &str, gradient: &SceneGradient, bbox: Option<(f64, f64, f64, f64)>) -> String {
    let mut out = String::new();
    // A bounding box with some size lets the gradient be laid out in user space, where circles stay
    // circles and an angle means the same on screen as in Visio.
    let user_box = bbox.filter(|(x0, y0, x1, y1)| x1 - x0 > 1e-6 && y1 - y0 > 1e-6);
    if let (8..=13, Some(bounds)) = (gradient.dir, user_box) {
        return rectangular_pattern(id, gradient, bounds);
    }
    let tag = match gradient.dir {
        // 1-7: radial presets, origin at a corner/edge/centre. The gradient
        // reaches the farthest shape corner.
        1..=7 => {
            let (cx, cy) = radial_origin(gradient.dir);
            if let Some((x0, y0, x1, y1)) = user_box {
                let (w, h) = (x1 - x0, y1 - y0);
                let (px, py) = (x0 + cx * w, y0 + cy * h);
                let radius = [(x0, y0), (x1, y0), (x0, y1), (x1, y1)]
                    .iter()
                    .map(|(x, y)| ((x - px).powi(2) + (y - py).powi(2)).sqrt())
                    .fold(0.0_f64, f64::max);
                out.push_str(&format!(
                    r#"<defs><radialGradient id="{}" gradientUnits="userSpaceOnUse" cx="{}" cy="{}" r="{}" fx="{}" fy="{}">"#,
                    id,
                    num(px),
                    num(py),
                    num(radius),
                    num(px),
                    num(py)
                ));
            } else {
                let radius = radial_radius(cx, cy);
                out.push_str(&format!(
                    r#"<defs><radialGradient id="{}" gradientUnits="objectBoundingBox" cx="{}" cy="{}" r="{}" fx="{}" fy="{}">"#,
                    id,
                    num(cx),
                    num(cy),
                    num(radius),
                    num(cx),
                    num(cy)
                ));
            }
            "radialGradient"
        }
        // 8-12: rectangular presets. SVG has no rectangular gradient; a
        // linear fade along the preset direction is the closest match.
        8..=12 if gradient.dir != 10 => {
            let (x1, y1, x2, y2) = rectangular_vector(gradient.dir);
            out.push_str(&format!(
                r#"<defs><linearGradient id="{}" gradientUnits="objectBoundingBox" x1="{}" y1="{}" x2="{}" y2="{}">"#,
                id,
                num(x1),
                num(y1),
                num(x2),
                num(y2)
            ));
            "linearGradient"
        }
        // 10 and 13: centred rectangular/path fades.
        10 | 13 => {
            out.push_str(&format!(
                r#"<defs><radialGradient id="{}" gradientUnits="objectBoundingBox" cx="0.5" cy="0.5" r="0.7071" fx="0.5" fy="0.5">"#,
                id
            ));
            "radialGradient"
        }
        // 0 (and anything unknown): linear, direction from FillGradientAngle.
        _ => {
            // Visio measures the angle clockwise on screen: 0 runs left to right, 90 degrees
            // runs top to bottom (checked by exporting the same shape at 0/90/180/270).
            let (cos, sin) = (gradient.angle.cos(), gradient.angle.sin());
            if let Some((x0, y0, x1, y1)) = user_box {
                // The gradient line spans the whole projection of the box on the direction, so
                // the first stop sits on one corner and the last on the opposite one.
                let (w, h) = (x1 - x0, y1 - y0);
                let half = (w * cos.abs() + h * sin.abs()) / 2.0;
                let (mx, my) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
                out.push_str(&format!(
                    r#"<defs><linearGradient id="{}" gradientUnits="userSpaceOnUse" x1="{}" y1="{}" x2="{}" y2="{}">"#,
                    id,
                    num(mx - cos * half),
                    num(my - sin * half),
                    num(mx + cos * half),
                    num(my + sin * half)
                ));
            } else {
                let (dx, dy) = (cos * 0.5, sin * 0.5);
                out.push_str(&format!(
                    r#"<defs><linearGradient id="{}" gradientUnits="objectBoundingBox" x1="{}" y1="{}" x2="{}" y2="{}">"#,
                    id,
                    num(0.5 - dx),
                    num(0.5 - dy),
                    num(0.5 + dx),
                    num(0.5 + dy)
                ));
            }
            "linearGradient"
        }
    };
    for (offset, color, opacity) in eased_stops(&gradient.stops, gradient.smooth) {
        out.push_str(&format!(
            r#"<stop offset="{}" stop-color="{}" stop-opacity="{}"/>"#,
            num(offset),
            escape(&color),
            num(opacity)
        ));
    }
    out.push_str(&format!("</{tag}></defs>"));
    out
}

/// Visio's rectangular presets (8-13) measured against its own exports: the blend runs from an origin
/// (bottom-right, bottom-left, centre, top-right, top-left corner for 8-12) and its value at a point is
/// the larger of the two normalised distances to the origin, so the contours are nested rectangles
/// with diagonal creases. SVG has no such gradient, so it is drawn as a pattern of nested bands.
const RECT_BANDS: usize = 48;

fn rectangular_origin(dir: u32) -> (f64, f64) {
    match dir {
        8 => (1.0, 1.0),
        9 => (0.0, 1.0),
        11 => (1.0, 0.0),
        12 => (0.0, 0.0),
        _ => (0.5, 0.5),
    }
}

fn rectangular_pattern(id: &str, gradient: &SceneGradient, (x0, y0, x1, y1): (f64, f64, f64, f64)) -> String {
    let (w, h) = (x1 - x0, y1 - y0);
    let (fx, fy) = rectangular_origin(gradient.dir);
    let (ox, oy) = (fx * w, fy * h);
    let (rx, ry) = (ox.max(w - ox), oy.max(h - oy));
    let samples = eased_stops_n(&gradient.stops, RECT_BANDS * 2, gradient.smooth, gradient.far_end || matches!(gradient.dir, 8..=12));
    let mut out = format!(
        r#"<defs><pattern id="{}" patternUnits="userSpaceOnUse" x="{}" y="{}" width="{}" height="{}">"#,
        id,
        num(x0),
        num(y0),
        num(w),
        num(h)
    );
    for k in (1..=RECT_BANDS).rev() {
        let t = k as f64 / RECT_BANDS as f64;
        let (_, color, opacity) = &samples[2 * k - 1];
        let (bx0, bx1) = ((ox - t * rx).max(0.0), (ox + t * rx).min(w));
        let (by0, by1) = ((oy - t * ry).max(0.0), (oy + t * ry).min(h));
        out.push_str(&format!(
            r#"<rect x="{}" y="{}" width="{}" height="{}" fill="{}" fill-opacity="{}"/>"#,
            num(bx0),
            num(by0),
            num(bx1 - bx0),
            num(by1 - by0),
            escape(color),
            num(*opacity)
        ));
    }
    out.push_str("</pattern></defs>");
    out
}

/// Origin of Visio's seven radial gradient presets, in SVG bounding-box
/// space (y grows downwards). Derived from Visio 2021 exports.
fn radial_origin(dir: u32) -> (f64, f64) {
    match dir {
        1 => (1.0, 1.0),   // bottom-right
        2 => (0.0, 1.0),   // bottom-left
        3 => (0.5, 0.5),   // centre
        4 => (0.5, 0.0),   // top edge centre
        5 => (0.5, 1.0),   // bottom edge
        6 => (1.0, 0.0),   // top-right
        _ => (0.0, 0.0),   // 7: top-left (corner-concentrated)
    }
}

/// Radius that reaches the farthest bounding-box corner from `(cx, cy)`.
fn radial_radius(cx: f64, cy: f64) -> f64 {
    [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)]
        .iter()
        .map(|(x, y)| ((x - cx).powi(2) + (y - cy).powi(2)).sqrt())
        .fold(0.0_f64, f64::max)
        .max(0.25)
}

/// Start/end points for Visio's rectangular gradient presets (8-12), mapped
/// to a linear gradient from stop 0 to stop 1.
fn rectangular_vector(dir: u32) -> (f64, f64, f64, f64) {
    match dir {
        8 => (1.0, 1.0, 0.0, 0.0),   // bottom-right -> top-left
        9 => (0.5, 1.0, 0.5, 0.0),   // bottom -> top
        11 => (1.0, 0.5, 0.0, 0.5),  // right -> left
        12 => (0.0, 0.0, 1.0, 1.0),  // top-left -> bottom-right
        _ => (0.5, 0.0, 0.5, 1.0),
    }
}

/// Outer shadow: the same silhouette offset behind the shape, optionally
/// blurred with a Gaussian filter. Filled parts cast a filled shadow; unfilled
/// parts cast their stroke.
fn render_shadow(out: &mut String, shape: &SceneShape, shadow: &SceneShadow, index: usize) {
    // an inner shadow is drawn over the shape's fill (`inner_shadow_overlay`)
    if shadow.opacity <= 0.0 || shadow.kind == 3 {
        return;
    }
    let filter = if shadow.blur > 0.05 {
        out.push_str(&format!(
            r#"<defs><filter id="shdw{index}" x="-50%" y="-50%" width="200%" height="200%"><feGaussianBlur stdDeviation="{}"/></filter></defs>"#,
            num(shadow.blur)
        ));
        format!(r#" filter="url(#shdw{index})""#)
    } else {
        String::new()
    };
    let transform = if shadow.kind == 2 {
        // An oblique shadow (measured in Visio, `probe-oblique.ps1`): the shape's vertical axis is tilted by the angle and scaled
        // by the scale factor, standing on the shape's bottom edge moved by the offset: a point `u` above the bottom edge goes to
        // `scale * u` along (sin angle, cos angle) from the base. Scale 2 at 0 degrees is a copy twice as tall; 45 degrees leans it
        // right and makes it flatter (height scale * cos).
        let (sin, cos) = shadow.oblique.sin_cos();
        let k = shadow.scale.max(0.01);
        format!(
            "matrix(1 0 {} {} {} {})",
            num(0.0 - k * sin),
            num(k * cos),
            num(shape.x + shadow.dx + k * sin * shape.h),
            num(shape.y + shape.h + shadow.dy - k * cos * shape.h)
        )
    } else {
        format!(
            "translate({} {})",
            num(shape.x + shadow.dx),
            num(shape.y + shadow.dy)
        )
    };
    out.push_str(&format!(
        r#"<g transform="{}"{} opacity="{}">"#,
        transform,
        filter,
        num(shadow.opacity)
    ));
    if shape.parts.is_empty() {
        if shape.image.is_some() {
            out.push_str(&format!(
                r#"<rect x="0" y="0" width="{}" height="{}" fill="{}"/>"#,
                num(shape.w),
                num(shape.h),
                escape(&shadow.color)
            ));
        }
    } else {
        for part in &shape.parts {
            let filled = !part.fill.eq_ignore_ascii_case("none");
            let stroked = !part.stroke.eq_ignore_ascii_case("none") && part.stroke_width > 0.0;
            if filled {
                out.push_str(&format!(
                    r#"<path d="{}" fill="{}" fill-rule="evenodd" stroke="none"/>"#,
                    escape(&part.d),
                    escape(&shadow.color)
                ));
            } else if stroked {
                out.push_str(&format!(
                    r#"<path d="{}" fill="none" stroke="{}" stroke-width="{}"/>"#,
                    escape(&part.d),
                    escape(&shadow.color),
                    num(part.stroke_width)
                ));
            }
        }
    }
    out.push_str("</g>");
}

/// Visio's inner shadow (type 3, measured with `probe-inner.ps1`): the part of the shape that a copy of it moved AGAINST the
/// offset does not cover is painted in the shadow colour on top of the fill (an offset to the lower right darkens the right and
/// bottom edges); the scale and the angle are not used; the blur softens the inner edge (half the standard deviation of an outer shadow's blur). Done with a mask: the shape's paths in
/// the shadow colour, minus the (blurred) copy.
fn inner_shadow_overlay(shape: &SceneShape, shadow: &SceneShadow, index: usize) -> String {
    if shadow.opacity <= 0.0 {
        return String::new();
    }
    let paths: Vec<&str> = if shape.parts.is_empty() {
        Vec::new()
    } else {
        shape.parts.iter().filter(|part| !part.fill.eq_ignore_ascii_case("none")).map(|part| part.d.as_str()).collect()
    };
    let rect;
    let paths = if paths.is_empty() {
        rect = format!("M0 0H{0}V{1}H0Z", num(shape.w), num(shape.h));
        vec![rect.as_str()]
    } else {
        paths
    };
    let blur = shadow.blur.max(0.0);
    let (filter_def, filter_attr) = if blur > 0.05 {
        (
            format!(r#"<filter id="innerBlur{index}" x="-50%" y="-50%" width="200%" height="200%"><feGaussianBlur stdDeviation="{}"/></filter>"#, num(blur * 0.5)),
            format!(r#" filter="url(#innerBlur{index})""#),
        )
    } else {
        (String::new(), String::new())
    };
    let pad = shape.w.max(shape.h) + shadow.dx.abs() + shadow.dy.abs() + blur * 3.0;
    let mut hole = String::new();
    let mut body = String::new();
    for d in &paths {
        hole.push_str(&format!(r##"<path d="{}" fill="#000"/>"##, escape(d)));
        body.push_str(&format!(r#"<path d="{}" fill="{}" fill-rule="evenodd" stroke="none"/>"#, escape(d), escape(&shadow.color)));
    }
    format!(
        r##"<defs>{filter_def}<mask id="innerMask{index}" maskUnits="userSpaceOnUse" x="{}" y="{}" width="{}" height="{}"><rect x="{}" y="{}" width="{}" height="{}" fill="#fff"/><g transform="translate({} {})"{filter_attr}>{hole}</g></mask></defs><g mask="url(#innerMask{index})" opacity="{}">{body}</g>"##,
        num(-pad),
        num(-pad),
        num(shape.w + 2.0 * pad),
        num(shape.h + 2.0 * pad),
        num(-pad),
        num(-pad),
        num(shape.w + 2.0 * pad),
        num(shape.h + 2.0 * pad),
        num(-shadow.dx),
        num(-shadow.dy),
        num(shadow.opacity)
    )
}

/// One wrapped line with its line box (`height`, the baseline distance of a block of such lines) and the extra space
/// that paragraph spacing puts in front of it.
struct LaidLine {
    align: String,
    runs: Vec<SceneRun>,
    height: f64,
    indent: f64,
    gap_before: f64,
    /// Left offset of a left-aligned line with a bullet / first-line indent (else the paragraph's `indent`), and the bullet.
    bullet: Option<(SceneRun, f64)>,
    /// For a centred / right-aligned first line with a bullet: where the text of a LEFT-aligned first line would start. The bullet then
    /// moves by as much as the centred (right-aligned) text moved from there (measured in Visio, step 206).
    bullet_ref: Option<f64>,
}

/// The lines of a text block with their spacing. Measured in Visio (PNG export of boxes with `SpLine`, `SpBefore`, `SpAfter`):
/// `SpLine` -k makes the line pitch k times the font size, a positive value is the pitch itself, no value (or 0) is 1.2 times
/// the font size. Between two paragraphs the spacing is the sum of the first one's `SpAfter` and the second one's `SpBefore`:
/// with an explicit `SpLine` the baseline distance is the line pitch plus the spacing; with automatic line spacing it is
/// `max(line pitch, spacing + 1.5 px)`.
fn layout_lines(text: &SceneText) -> Vec<LaidLine> {
    let mut lines: Vec<LaidLine> = Vec::new();
    let mut previous_after = 0.0;
    for (index, paragraph) in text.paragraphs.iter().enumerate() {
        let geometry = paragraph_geometry(paragraph, text);
        for (position, line) in wrap_paragraph(paragraph, text).into_iter().enumerate() {
            let size = line.iter().filter_map(|run| run.font_size).fold(text.font_size, f64::max);
            let height = if paragraph.line_spacing > 0.0 {
                paragraph.line_spacing.max(1.0)
            } else if paragraph.line_spacing < 0.0 {
                (size * -paragraph.line_spacing).max(1.0)
            } else {
                (size * 1.2).max(1.0)
            };
            let gap_before = if position == 0 && index > 0 {
                let spacing = previous_after + paragraph.space_before;
                // automatic line spacing absorbs a part of the paragraph spacing; an explicit SpLine adds it
                if paragraph.line_spacing == 0.0 { (spacing - 1.08 * size).max(0.0) } else { spacing }
            } else {
                0.0
            };
            let left_aligned = matches!(paragraph.align.as_str(), "left" | "start");
            let (indent, bullet, bullet_ref) = match &geometry {
                // centred / right-aligned (Visio, step 206): the first line is centred in the area right of its own start (first_x), the later lines in the area right of IndLeft; the bullet sits one bullet gap (first_x - IndLeft - IndFirst) before the first line's text
                Some(g) if position == 0 && !left_aligned => (g.first_x, g.bullet_x.map(|x| (bullet_run(paragraph, &paragraph.bullet_str), x)), Some(g.first_x)),
                Some(g) if position == 0 => (g.first_x, g.bullet_x.map(|x| (bullet_run(paragraph, &paragraph.bullet_str), x)), None),
                _ => (paragraph.indent, None, None),
            };
            lines.push(LaidLine { align: paragraph.align.clone(), runs: line, height, indent, gap_before, bullet, bullet_ref });
        }
        previous_after = paragraph.space_after;
    }
    lines
}

fn render_text(out: &mut String, text: &SceneText) {
    let lines = layout_lines(text);
    if lines.is_empty() {
        return;
    }
    if let Some(colour) = &text.background {
        out.push_str(&format!(
            r#"<rect x="{}" y="{}" width="{}" height="{}" fill="{}"/>"#,
            num(text.x),
            num(text.y),
            num(text.w),
            num(text.h),
            escape(colour)
        ));
    }
    let total_height: f64 = lines.iter().map(|line| line.height + line.gap_before).sum();
    let start_y = if text.v_align == "top" {
        text.y
    } else if text.v_align == "bottom" {
        text.y + text.h - total_height
    } else {
        text.y + (text.h - total_height) / 2.0
    };
    let mut y = start_y;
    for LaidLine { align, runs, height: line_height, indent, gap_before, bullet, bullet_ref } in lines {
        y += gap_before;
        let anchor = match align.as_str() {
            "left" => "start",
            "right" => "end",
            _ => "middle",
        };
        let x = match align.as_str() {
            "left" => text.x + indent,
            "right" => text.x + text.w,
            _ => text.x + indent + (text.w - indent) / 2.0,
        };
        let base_factor = baseline_factor(&text.font_family, text.outside);
        let mut baseline = y + line_height * base_factor;
        if let Some(script) = text.script {
            // Visio: a subscript sits `shift` of the full font size below the normal baseline; a superscript is raised by it, but its glyphs (smaller)
            // never rise above the top of the line (measured with Calibri and Kalinga at 60 pt)
            baseline += script.shift * text.font_size;
            if script.shift < 0.0 {
                baseline = baseline.max(y + text.font_size * script.scale * 1.2 * base_factor);
            }
        }
        // Shape-level weight/style/underline are inherited by the runs (a run only
        // states its own value when it differs), so they must be set here.
        let mut decor = String::new();
        if text.bold {
            decor.push_str(r#" font-weight="bold""#);
        }
        if text.italic {
            decor.push_str(r#" font-style="italic""#);
        }
        if text.underline {
            decor.push_str(r#" text-decoration="underline""#);
        }
        if FIT_TEXT.with(|flag| flag.get()) {
            let measured: f64 = runs.iter().map(|run| run_width(run, text)).sum();
            if measured > 0.5 {
                decor.push_str(&format!(r#" textLength="{}" lengthAdjust="spacingAndGlyphs""#, num(measured)));
            }
        }
        if let Some((run, bullet_x)) = &bullet {
            // a centred / right-aligned line: the bullet moves with the text, from where a left-aligned first line would start
            let bullet_x = match bullet_ref {
                Some(first_x) => {
                    let width: f64 = runs.iter().map(|run| run_width(run, text)).sum();
                    let text_start = if matches!(align.as_str(), "right" | "end") { x - width } else { x - width / 2.0 };
                    bullet_x + (text_start - text.x - first_x)
                }
                None => *bullet_x,
            };
            out.push_str(&format!(
                r#"<text x="{}" y="{}" text-anchor="start" font-family="{}" font-size="{}" fill="{}" xml:space="preserve">"#,
                num(text.x + bullet_x),
                num(baseline),
                escape(&text.font_family),
                num(text.font_size),
                escape(&text.color)
            ));
            out.push_str(&run_tspan(run, text));
            out.push_str("</text>");
        }
        out.push_str(&format!(
            r#"<text x="{}" y="{}" text-anchor="{}" font-family="{}" font-size="{}" fill="{}"{} xml:space="preserve">"#,
            num(x),
            num(baseline),
            anchor,
            escape(&text.font_family),
            num(base_size(text)),
            escape(&text.color),
            decor
        ));
        RTL_LINE.with(|flag| flag.set(runs.iter().any(|run| has_rtl(&run.text))));
        for run in &runs {
            if !run.text.is_empty() {
                out.push_str(&run_tspan(run, text));
            }
        }
        RTL_LINE.with(|flag| flag.set(false));
        out.push_str("</text>");
        y += line_height;
    }
}

/// Laid-out height of a text block using the same wrapping/line-height code as
/// the renderer. Scene bounds use this because a block's declared height can
/// be much shorter than its content (Visio stores one line's height for a
/// multi-line title), which would otherwise clip the first line.
pub(crate) fn text_layout_height(text: &SceneText) -> f64 {
    let total: f64 = layout_lines(text).iter().map(|line| line.height + line.gap_before).sum();
    total.max(text.font_size * 1.2)
}

/// The ink box of a laid-out text block, relative to the page: the widest line (placed by each
/// paragraph's alignment) and the laid-out height, positioned by the vertical alignment. Visio
/// exports a shape that paints nothing as just its text.
pub(crate) fn text_ink_extent(text: &SceneText) -> (f64, f64, f64, f64) {
    // Sub- and superscript text is smaller than its line box: Visio's export reaches only to the bottom of the text block (`TxtHeight`)
    let total = if text.script.is_some() { text_layout_height(text).min(text.h.max(text.glyph_size() * 1.2)) } else { text_layout_height(text) };
    let top = match text.v_align.as_str() {
        "top" => text.y,
        "bottom" => text.y + text.h - total,
        _ => text.y + (text.h - total) / 2.0,
    };
    let (mut left, mut right) = (f64::INFINITY, f64::NEG_INFINITY);
    for paragraph in &text.paragraphs {
        for line in wrap_paragraph(paragraph, text) {
            let width: f64 = line.iter().map(|run| run_width(run, text)).sum();
            let (x0, x1) = match paragraph.align.as_str() {
                "left" | "start" => (text.x + paragraph.indent, text.x + paragraph.indent + width),
                "right" | "end" => (text.x + text.w - width, text.x + text.w),
                _ => {
                    let centre = text.x + paragraph.indent + (text.w - paragraph.indent) / 2.0;
                    (centre - width / 2.0, centre + width / 2.0)
                }
            };
            left = left.min(x0);
            right = right.max(x1);
        }
    }
    if !left.is_finite() {
        return (text.x, top, text.x + text.w, top + total);
    }
    (left, top, right, top + total)
}

/// Where the lines of a left-aligned paragraph with a bullet or a first-line indent start (offsets from the text block's left
/// edge). Measured in Visio (PNG export, `make-bullets.mjs`): the bullet sits at `IndLeft + IndFirst`, the first line's text starts
/// at `IndLeft` when that is not 0 (else 0.573 in after the bullet), the other lines start at `IndLeft`; without a bullet the
/// first line starts at `IndLeft + IndFirst`.
struct ParaGeometry {
    bullet_x: Option<f64>,
    first_x: f64,
    rest_x: f64,
}

fn bullet_run(paragraph: &SceneParagraph, text: &str) -> SceneRun {
    let mut run = paragraph.runs.first().cloned().unwrap_or(SceneRun {
        text: String::new(),
        font_size: None,
        font_family: None,
        color: None,
        bold: None,
        italic: None,
        underline: None,
        strike: None,
        spacing: None,
        script: None,
    });
    run.text = text.to_string();
    run
}

fn paragraph_geometry(paragraph: &SceneParagraph, text: &SceneText) -> Option<ParaGeometry> {
    let left_aligned = matches!(paragraph.align.as_str(), "left" | "start");
    // centred / right-aligned paragraphs only take part with a bullet (a first-line indent alone is not measured for them)
    if (!left_aligned && !paragraph.bullet) || (!paragraph.bullet && paragraph.indent_first == 0.0) {
        return None;
    }
    let line0 = (paragraph.indent + paragraph.indent_first).max(0.0);
    if paragraph.bullet {
        let width = run_width(&bullet_run(paragraph, &paragraph.bullet_str), text);
        let first_x = if paragraph.indent > 0.5 { paragraph.indent.max(line0 + width) } else { line0 + 55.0 };
        Some(ParaGeometry { bullet_x: Some(line0), first_x, rest_x: paragraph.indent })
    } else {
        Some(ParaGeometry { bullet_x: None, first_x: line0, rest_x: paragraph.indent })
    }
}

fn wrap_paragraph(paragraph: &SceneParagraph, text: &SceneText) -> Vec<Vec<SceneRun>> {
    let geometry = paragraph_geometry(paragraph, text);
    let rest_width = if text.fit_line { f64::INFINITY } else { (text.w - paragraph.indent).max(10.0) };
    let mut max_width = if text.fit_line {
        f64::INFINITY
    } else {
        geometry.as_ref().map_or(rest_width, |g| (text.w - g.first_x).max(10.0))
    };
    let mut lines: Vec<Vec<SceneRun>> = Vec::new();
    let mut current: Vec<SceneRun> = Vec::new();
    let mut width = 0.0;
    let default_run = SceneRun {
        text: String::new(),
        font_size: None,
        font_family: None,
        color: None,
        bold: None,
        italic: None,
        underline: None,
        strike: None,
        spacing: None,
        script: None,
    };
    if paragraph.bullet && geometry.is_none() {
        let mut run = paragraph
            .runs
            .first()
            .cloned()
            .unwrap_or(default_run.clone());
        run.text = format!("{} ", paragraph.bullet_str);
        width = run_width(&run, text);
        current.push(run);
    }
    for run in &paragraph.runs {
        let segments: Vec<&str> = run.text.split('\n').collect();
        for (segment_index, segment) in segments.iter().enumerate() {
            if segment_index > 0 {
                lines.push(std::mem::take(&mut current));
                max_width = rest_width;
                width = 0.0;
            }
            for word in segment.split_whitespace() {
                let size = run_size(run, text);
                let family = run.font_family.as_deref().unwrap_or(&text.font_family);
                let bold = run_bold(run, text);
                let italic = run.italic.unwrap_or(text.italic);
                let word_width = text_width(word, family, size, bold, italic);
                let space_width = text_width(" ", family, size, bold, italic);
                if !current.is_empty() && width + space_width + word_width > max_width {
                    lines.push(std::mem::take(&mut current));
                    max_width = rest_width;
                    width = 0.0;
                }
                // A single word wider than the block breaks between characters (Visio wraps
                // "MachinesAzureArc" as "MachinesAzure" / "Arc" in a 1 in label).
                if current.is_empty() && word_width > max_width && word.chars().count() > 1 && !word.contains(|c| c == '\\' || c == '/') {
                    let mut piece = String::new();
                    for ch in word.chars() {
                        piece.push(ch);
                        if text_width(&piece, family, size, bold, italic) > max_width && piece.chars().count() > 1 {
                            piece.pop();
                            let mut chunk = run.clone();
                            chunk.text = std::mem::take(&mut piece);
                            lines.push(vec![chunk]);
                            max_width = rest_width;
                            piece.push(ch);
                        }
                    }
                    let mut tail = run.clone();
                    width = text_width(&piece, family, size, bold, italic);
                    tail.text = piece;
                    current.push(tail);
                    continue;
                }
                let prefix = if current.is_empty() { "" } else { " " };
                let mut next = run.clone();
                next.text = format!("{prefix}{word}");
                width += if current.is_empty() {
                    word_width
                } else {
                    space_width + word_width
                };
                current.push(next);
            }
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(Vec::new());
    }
    lines
}

/// The size the glyphs of the block are drawn at (the font size, smaller for sub- and superscript text).
fn base_size(text: &SceneText) -> f64 {
    text.font_size * text.script.map_or(1.0, |script| script.scale)
}

fn run_size(run: &SceneRun, text: &SceneText) -> f64 {
    run.font_size.unwrap_or(text.font_size) * run.script.or(text.script).map_or(1.0, |script| script.scale)
}

fn run_bold(run: &SceneRun, text: &SceneText) -> bool {
    run.bold.unwrap_or(text.bold)
}

fn run_width(run: &SceneRun, text: &SceneText) -> f64 {
    text_width(
        &run.text,
        run.font_family.as_deref().unwrap_or(&text.font_family),
        run_size(run, text),
        run_bold(run, text),
        run.italic.unwrap_or(text.italic),
    )
}

fn text_width(value: &str, family: &str, size: f64, bold: bool, italic: bool) -> f64 {
    crate::text::measure(value, family, size, bold, italic)
}

/// True when `text` has letters of a right-to-left script (Hebrew, Arabic, Syriac, Thaana, N'Ko and their presentation forms).
fn has_rtl(text: &str) -> bool {
    text.chars().any(|c| matches!(c as u32, 0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF))
}

/// Visio keeps a number that follows right-to-left text as a left-to-right run right after it (measured: "abc <Hebrew> 123"
/// has the number at the right end, `make-rtl.mjs`), where the Unicode algorithm would put it to the left of the Hebrew. Each
/// run of digits (with . , : / - between digits) in a line that has right-to-left letters is isolated as a left-to-right run.
fn isolate_numbers(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() {
            let mut j = i;
            while j < chars.len() && (chars[j].is_ascii_digit() || (matches!(chars[j], '.' | ',' | ':' | '/' | '-') && j + 1 < chars.len() && chars[j + 1].is_ascii_digit())) {
                j += 1;
            }
            let number: String = chars[i..j].iter().collect();
            out.push_str(&format!("<tspan direction=\"ltr\" style=\"unicode-bidi:isolate\">{}</tspan>", escape(&number)));
            i = j;
        } else {
            let start = i;
            while i < chars.len() && !chars[i].is_ascii_digit() {
                i += 1;
            }
            let piece: String = chars[start..i].iter().collect();
            out.push_str(&escape(&piece));
        }
    }
    out
}

fn run_tspan(run: &SceneRun, text: &SceneText) -> String {
    let size = run_size(run, text);
    let family = run.font_family.as_deref().unwrap_or(&text.font_family);
    let color = run.color.as_deref().unwrap_or(&text.color);
    let bold = run.bold.unwrap_or(text.bold);
    let italic = run.italic.unwrap_or(text.italic);
    let underline = run.underline.unwrap_or(text.underline);
    let mut attrs = String::new();
    if (size - base_size(text)).abs() > 1e-9 {
        attrs.push_str(&format!(r#" font-size="{}""#, num(size)));
    }
    if let (Some(script), None) = (run.script, text.script) {
        // a sub/superscript run inside normal text: SVG's baseline-shift is positive upwards
        attrs.push_str(&format!(r#" baseline-shift="{}""#, num(-script.shift * run.font_size.unwrap_or(text.font_size))));
    }
    if family != text.font_family {
        attrs.push_str(&format!(r#" font-family="{}""#, escape(family)));
    }
    if color != text.color {
        attrs.push_str(&format!(r#" fill="{}""#, escape(color)));
    }
    if bold != text.bold {
        attrs.push_str(&format!(
            r#" font-weight="{}""#,
            if bold { "bold" } else { "normal" }
        ));
    }
    if italic != text.italic {
        attrs.push_str(&format!(
            r#" font-style="{}""#,
            if italic { "italic" } else { "normal" }
        ));
    }
    let strike = run.strike.unwrap_or(false);
    if underline != text.underline || strike {
        let mut decoration = Vec::new();
        if underline {
            decoration.push("underline");
        }
        if strike {
            decoration.push("line-through");
        }
        attrs.push_str(&format!(
            r#" text-decoration="{}""#,
            if decoration.is_empty() { "none".to_string() } else { decoration.join(" ") }
        ));
    }
    if let Some(spacing) = run.spacing.filter(|spacing| spacing.abs() > 1e-6) {
        attrs.push_str(&format!(r#" letter-spacing="{}""#, num(spacing)));
    }
    let content = if RTL_LINE.with(|flag| flag.get()) { isolate_numbers(&run.text) } else { escape(&run.text) };
    format!(r#"<tspan{attrs}>{}</tspan>"#, content)
}

/// Where the baseline sits in a line box (as a share of the 1.2 em line height). 0.8 was measured
/// on Calibri documents and on text centred in its shape (HVACC best 0.80). The label of an icon
/// (centre outside the shape box) in Cambria, Visio's fallback face for unthemed stencils, sits
/// higher (fitted against ADO, ALARM, AWS exports: best 0.72-0.76, taken 0.73).
fn baseline_factor(font_family: &str, outside: bool) -> f64 {
    if outside && font_family.eq_ignore_ascii_case("Cambria") {
        0.73
    } else {
        0.8
    }
}

/// Append the line from `a` to `b`, or the hop Visio stored for that stretch (an arc through its
/// apex, or a polyline) when the connector crosses another one there.
fn push_segment(d: &mut String, a: Point, b: Point, hops: &[SceneHop]) {
    let near = |p: Point, q: Point| (p.x - q.x).abs() < 0.05 && (p.y - q.y).abs() < 0.05;
    let Some(hop) = hops.iter().find(|h| near(h.from, a) && near(h.to, b)) else {
        d.push_str(&format!("L{} {}", num(b.x), num(b.y)));
        return;
    };
    if hop.gap {
        d.push_str(&format!("M{} {}", num(b.x), num(b.y)));
        return;
    }
    if let Some(apex) = hop.apex {
        let (cx, cy) = (b.x - a.x, b.y - a.y);
        let chord = cx.hypot(cy);
        let (mx, my) = ((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
        let sagitta = (apex.x - mx).hypot(apex.y - my);
        if chord > 1e-6 && sagitta > 1e-6 {
            let radius = (chord * chord / 4.0 + sagitta * sagitta) / (2.0 * sagitta);
            // Clockwise on screen (sweep 1) when the apex lies to the left of the chord in y-down
            // coordinates, that is when the cross product is negative.
            let sweep = if cx * (apex.y - a.y) - cy * (apex.x - a.x) < 0.0 { 1 } else { 0 };
            d.push_str(&format!("A{} {} 0 0 {} {} {}", num(radius), num(radius), sweep, num(b.x), num(b.y)));
            return;
        }
    }
    for p in &hop.via {
        d.push_str(&format!("L{} {}", num(p.x), num(p.y)));
    }
    d.push_str(&format!("L{} {}", num(b.x), num(b.y)));
}

fn render_connector(out: &mut String, connector: &SceneConnector) {
    let mut points = vec![connector.from];
    points.extend_from_slice(&connector.waypoints);
    points.push(connector.to);
    // Elbow auto-routes can repeat the endpoints as waypoints. Drop
    // consecutive duplicates, otherwise the final segment is zero-length and
    // the arrowhead is skipped.
    let mut unique: Vec<Point> = Vec::with_capacity(points.len());
    for point in points {
        if unique
            .last()
            .is_none_or(|last| (point.x - last.x).hypot(point.y - last.y) > 0.01)
        {
            unique.push(point);
        }
    }
    let points = unique;
    if points.len() < 2 {
        return;
    }
    let mut d = String::new();
    if let Some((c1, c2)) = connector.curve {
        d.push_str(&format!(
            "M{} {}C{} {} {} {} {} {}",
            num(connector.from.x),
            num(connector.from.y),
            num(c1.x),
            num(c1.y),
            num(c2.x),
            num(c2.y),
            num(connector.to.x),
            num(connector.to.y)
        ));
    } else {
        d.push_str(&format!("M{} {}", num(points[0].x), num(points[0].y)));
        for pair in points.windows(2) {
            push_segment(&mut d, pair[0], pair[1], &connector.hops);
        }
    }
    out.push_str(&format!(
        r#"<path d="{}"{}"#,
        escape(&d),
        paint_attrs(
            "none",
            1.0,
            &connector.stroke,
            1.0,
            connector.stroke_width,
            connector.dash.as_deref()
        )
    ));
    out.push_str("/>");
    let weight_pt = connector.stroke_width * 0.75;
    let paint = escape(&connector.stroke);
    for (end, tip, from) in [
        (connector.begin_line_end, points[0], connector.curve.map_or(points[1], |(c1, _)| c1)),
        (connector.end_line_end, points[points.len() - 1], connector.curve.map_or(points[points.len() - 2], |(_, c2)| c2)),
    ] {
        let Some((kind, size)) = end else { continue };
        let (dx, dy) = (tip.x - from.x, tip.y - from.y);
        let len = dx.hypot(dy);
        if len > 1e-9 {
            draw_gallery_end(out, kind, size as usize, (tip.x, tip.y), (dx / len, dy / len), weight_pt, connector.stroke_width, &paint);
        }
    }
    if connector.begin_line_end.is_none() && arrow_kind(connector.arrow_start.as_deref()) {
        let start_from = connector.curve.map_or(points[1], |(c1, _)| c1);
        render_arrow(out, start_from, points[0], connector, arrow_filled(connector.arrow_start.as_deref()));
    }
    if connector.end_line_end.is_none() && arrow_kind(connector.arrow_end.as_deref()) {
        let end_from = connector
            .curve
            .map_or(points[points.len() - 2], |(_, c2)| c2);
        render_arrow(out, end_from, points[points.len() - 1], connector, arrow_filled(connector.arrow_end.as_deref()));
    }
    if !connector.label.is_empty() {
        let lines: Vec<&str> = connector
            .label
            .split(['\n', '\r'])
            .filter(|line| !line.is_empty())
            .collect();
        let line_height = 12.5;
        let total = lines.len().max(1) as f64 * line_height;
        let start = connector.label_point.y - total / 2.0 + line_height * 0.8;
        for (index, line) in lines.iter().enumerate() {
            out.push_str(&format!(
                r##"<text x="{}" y="{}" text-anchor="middle" font-family="sans-serif" font-size="11" fill="#1b2733">{}</text>"##,
                num(connector.label_point.x),
                num(start + index as f64 * line_height),
                escape(line)
            ));
        }
    }
}

/// Solid unless the arrow type is one of the hollow variants. (The start arrow used to
/// be drawn hollow whatever its type, the end arrow always solid.)
fn arrow_filled(value: Option<&str>) -> bool {
    !matches!(value, Some("hollowTriangle") | Some("hollowDiamond"))
}

fn arrow_kind(value: Option<&str>) -> bool {
    !matches!(value, None | Some("none") | Some(""))
}

fn render_arrow(
    out: &mut String,
    from: Point,
    tip: Point,
    connector: &SceneConnector,
    filled: bool,
) {
    let dx = tip.x - from.x;
    let dy = tip.y - from.y;
    let len = dx.hypot(dy);
    if len < 1e-9 {
        return;
    }
    let ux = dx / len;
    let uy = dy / len;
    let size = (connector.stroke_width * 4.0).max(6.0);
    let half = size * 0.45;
    let base_x = tip.x - ux * size;
    let base_y = tip.y - uy * size;
    let p1 = (base_x - uy * half, base_y + ux * half);
    let p2 = (base_x + uy * half, base_y - ux * half);
    out.push_str(&format!(
        r#"<path d="M{} {}L{} {}L{} {}Z" fill="{}" stroke="{}" stroke-width="1"/>"#,
        num(tip.x),
        num(tip.y),
        num(p1.0),
        num(p1.1),
        num(p2.0),
        num(p2.1),
        if filled {
            escape(&connector.stroke)
        } else {
            "#ffffff".to_string()
        },
        escape(&connector.stroke)
    ));
}

#[derive(Clone, Copy)]
enum ArrowStyle {
    Fill,
    Hollow,
    Stroke,
}

enum ArrowItem {
    Path(&'static str, ArrowStyle),
    Circle(f64, f64, ArrowStyle),
}

/// Visio's line-end gallery (EndArrow/BeginArrow 1-35) in units of the arrow size, tip at the origin,
/// +x pointing along the line, +y to its right; measured from Visio 16.
fn arrow_items(kind: u32) -> Option<&'static [ArrowItem]> {
    use ArrowItem::{Circle, Path};
    use ArrowStyle::{Fill, Hollow, Stroke};
    Some(match kind {
        1 => &[Path("M-0.6,-0.5L0,0L-0.6,0.5", Stroke)],
        2 => &[Path("M0,0L-0.45,-0.5L-0.45,0.5Z", Fill)],
        3 => &[Path("M-1.15,-0.52L0,0L-1.15,0.52", Stroke)],
        4 | 13 => &[Path("M0,0L-1.5,-0.5L-1.5,0.5Z", Fill)],
        5 => &[Path("M0,0L-1,-0.5Q-0.8,0 -1,0.5Z", Fill)],
        6 => &[Path("M0,0L-0.9,-0.5Q-1.2,0 -0.9,0.5Z", Fill)],
        7 => &[Path("M-1.15,-0.58Q-0.35,-0.45 0,0Q-0.35,0.45 -1.15,0.58", Stroke)],
        8 => &[Path("M0,0L-0.95,-0.45Q-0.65,0 -0.95,0.45Z", Fill)],
        9 => &[Path("M-0.55,0.55L0.55,-0.55", Stroke)],
        10 => &[Circle(0.0, 0.41, Fill)],
        11 => &[Path("M-0.41,-0.41L0.41,-0.41L0.41,0.41L-0.41,0.41Z", Fill)],
        12 => &[Path("M-1.7,-0.55L0,0L-1.7,0.55", Stroke)],
        14 => &[Path("M0,0L-1.65,-0.55L-1.65,0.55Z", Hollow)],
        15 => &[Path("M0,0L-0.6,-0.55L-0.6,0.55Z", Hollow)],
        16 => &[Path("M0,0L-1.15,-0.55L-1.15,0.55Z", Hollow)],
        17 => &[Path("M0,0L-1.15,-0.55Q-0.85,0 -1.15,0.55Z", Hollow)],
        18 => &[Path("M0,0L-1.2,-0.55Q-1.45,0 -1.2,0.55Z", Hollow)],
        19 => &[Path("M0,0L-1.15,-0.58Q-0.7,0 -1.15,0.58Z", Hollow)],
        20 => &[Circle(0.0, 0.38, Hollow)],
        21 => &[Path("M-0.38,-0.38L0.38,-0.38L0.38,0.38L-0.38,0.38Z", Hollow)],
        22 => &[Path("M0,0L-1.1,-0.6L-2.2,0L-1.1,0.6Z", Hollow)],
        23 => &[Path("M-1.65,-0.55L-0.55,0.55", Stroke)],
        24 => &[Path("M-0.77,-0.4L-0.77,0.4", Stroke)],
        25 => &[Path("M-0.76,-0.4L-0.76,0.4M-1.17,-0.4L-1.17,0.4", Stroke)],
        26 => &[Path("M-0.73,-0.4L-0.73,0.4M-1.15,-0.4L-1.15,0.4M-1.57,-0.4L-1.57,0.4", Stroke)],
        27 => &[Path("M0,-0.45L-0.85,0L0,0.45", Stroke)],
        28 => &[Path("M0,-0.45L-0.8,0L0,0.45M-1.17,-0.4L-1.17,0.4", Stroke)],
        29 => &[Path("M0,-0.45L-0.8,0L0,0.45", Stroke), Circle(-1.20, 0.38, Hollow)],
        30 => &[Path("M-0.38,-0.4L-0.38,0.4", Stroke), Circle(-1.15, 0.38, Hollow)],
        31 => &[Path("M-1.17,-0.4L-1.17,0.4", Stroke), Circle(-0.48, 0.38, Hollow)],
        32 => &[Path("M-1.58,-0.4L-1.58,0.4M-1.20,-0.4L-1.20,0.4", Stroke), Circle(-0.54, 0.38, Hollow)],
        33 => &[
            Path("M-1.96,-0.4L-1.96,0.4M-1.57,-0.4L-1.57,0.4M-1.14,-0.4L-1.14,0.4", Stroke),
            Circle(-0.44, 0.38, Hollow),
        ],
        34 => &[Path("M-0.9,0L-2,-0.57L-3.1,0L-2,0.57Z", Hollow), Circle(-0.44, 0.38, Hollow)],
        35 => &[Path("M-1.17,-0.4L-1.17,0.4", Stroke), Circle(-0.47, 0.42, Fill)],
        _ => return None,
    })
}

/// One gallery line end (kind 1-35, size index `index`) with its tip at `tip`, pointing along the unit vector `dir`.
#[allow(clippy::too_many_arguments)]
fn draw_gallery_end(out: &mut String, kind: u32, index: usize, tip: (f64, f64), dir: (f64, f64), weight_pt: f64, stroke_width: f64, fill: &str) {
    const ARROW_BASE: [f64; 7] = [11.5, 14.0, 20.0, 26.0, 31.5, 74.0, 149.0];
    let Some(items) = arrow_items(kind) else { return };
    let size_px = (ARROW_BASE[index.min(6)] + 8.0 * weight_pt) / 300.0 * 96.0;
    let angle = dir.1.atan2(dir.0).to_degrees();
    out.push_str(&format!(r#"<g transform="translate({} {}) rotate({}) scale({})">"#, num(tip.0), num(tip.1), num(angle), num(size_px)));
    let sw = stroke_width / size_px;
    for item in items {
        let (shape, style) = match item {
            ArrowItem::Path(d, style) => (format!(r#"<path d="{d}""#), *style),
            ArrowItem::Circle(cx, r, style) => (format!(r#"<circle cx="{cx}" cy="0" r="{r}""#), *style),
        };
        let paint = match style {
            ArrowStyle::Fill => format!(r#" fill="{fill}" stroke="none""#),
            ArrowStyle::Hollow => format!(r##" fill="#ffffff" stroke="{fill}" stroke-width="{}" stroke-linejoin="round""##, num(sw)),
            ArrowStyle::Stroke => format!(r#" fill="none" stroke="{fill}" stroke-width="{}" stroke-linejoin="round" stroke-linecap="butt""#, num(sw)),
        };
        out.push_str(&shape);
        out.push_str(&paint);
        out.push_str("/>");
    }
    out.push_str("</g>");
}

/// Line-end arrowheads of an open part, measured from Visio's own export: a filled arrow (type 4) is an
/// isosceles triangle as long as it is wide, a dot (type 42) a circle ending at the line's end; both
/// grow by a fixed amount per point of line weight on top of a base size per size index.
fn render_part_arrows(out: &mut String, part: &crate::scene::ScenePart) {
    const ARROW_BASE: [f64; 7] = [11.5, 14.0, 20.0, 26.0, 31.5, 74.0, 149.0];
    const DOT_BASE: [f64; 7] = [10.0, 11.0, 16.0, 21.0, 25.0, 56.0, 112.0];
    let Some([(start, after_start), (end, before_end)]) = crate::scene::path_ends(&part.d) else {
        return;
    };
    // Sizes are measured at 300 dpi; the scene is 96 dpi and the weight is in points.
    let weight_pt = part.stroke_width * 0.75;
    for (arrow, tip, from) in [(part.begin_arrow, start, after_start), (part.end_arrow, end, before_end)] {
        let Some((kind, size)) = arrow else { continue };
        let index = (size as usize).min(6);
        let (dx, dy) = (tip.0 - from.0, tip.1 - from.1);
        let len = dx.hypot(dy);
        if len < 1e-9 {
            continue;
        }
        let (ux, uy) = (dx / len, dy / len);
        let fill = escape(&part.stroke);
        match kind {
            4 => {
                let size_px = (ARROW_BASE[index] + 8.0 * weight_pt) / 300.0 * 96.0;
                let (bx, by) = (tip.0 - ux * size_px, tip.1 - uy * size_px);
                let half = size_px / 2.0;
                out.push_str(&format!(
                    r#"<path d="M{} {}L{} {}L{} {}Z" fill="{}" stroke="none"/>"#,
                    num(tip.0),
                    num(tip.1),
                    num(bx - uy * half),
                    num(by + ux * half),
                    num(bx + uy * half),
                    num(by - ux * half),
                    fill
                ));
            }
            1..=35 => draw_gallery_end(out, kind, index, tip, (ux, uy), weight_pt, part.stroke_width, &fill),
            42 => {
                let diameter = (DOT_BASE[index] + 6.0 * weight_pt) / 300.0 * 96.0;
                out.push_str(&format!(
                    r#"<circle cx="{}" cy="{}" r="{}" fill="{}" stroke="none"/>"#,
                    num(tip.0 - ux * diameter / 2.0),
                    num(tip.1 - uy * diameter / 2.0),
                    num(diameter / 2.0),
                    fill
                ));
            }
            _ => {}
        }
    }
}

fn paint_attrs(
    fill: &str,
    fill_opacity: f64,
    stroke: &str,
    stroke_opacity: f64,
    stroke_width: f64,
    dash: Option<&str>,
) -> String {
    let mut out = format!(r#" fill="{}""#, escape(fill));
    if fill != "none" {
        // Visio's geometry engine fills compound contours with the even-odd
        // rule; legacy stencils (electrical resistor bodies, rings) rely on
        // keyhole contours that only leave their hole under even-odd.
        out.push_str(r#" fill-rule="evenodd""#);
    }
    if fill_opacity < 1.0 {
        out.push_str(&format!(r#" fill-opacity="{}""#, num(fill_opacity)));
    }
    if stroke == "none" || stroke_width <= 0.0 || stroke_opacity <= 0.0 {
        out.push_str(r#" stroke="none""#);
    } else {
        // Visio rasterises hairlines at a minimum of one device pixel.
        out.push_str(&format!(
            r#" stroke="{}" stroke-width="{}" stroke-linejoin="round" stroke-linecap="{}""#,
            escape(stroke),
            num(stroke_width.max(1.0 / DEVICE_SCALE.with(|value| value.get()))),
            match dash.and_then(|value| value.split_once('+')).map(|(_, cap)| cap) {
                // Visio's numbered dash patterns never extend their dashes with the line cap.
                _ if dash.is_some_and(|value| value.starts_with("pat")) => "butt",
                Some("flat") => "butt",
                Some("square") => "square",
                _ => "round",
            }
        ));
        if stroke_opacity < 1.0 {
            out.push_str(&format!(r#" stroke-opacity="{}""#, num(stroke_opacity)));
        }
        if let Some(pattern) = dash_pattern(dash, stroke_width.max(1.0 / DEVICE_SCALE.with(|value| value.get()))) {
            out.push_str(&format!(r#" stroke-dasharray="{pattern}""#));
        }
    }
    out
}

/// The corner points of a closed polygon written with M/L/Z only (absolute coordinates).
fn polygon_points(d: &str) -> Option<Vec<(f64, f64)>> {
    if d.matches('M').count() != 1 || !d.contains(['Z', 'z']) || d.contains(['C', 'Q', 'A', 'c', 'q', 'a', 'S', 'T']) {
        return None;
    }
    let numbers: Vec<f64> = d
        .split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
        .filter(|t| !t.is_empty())
        .filter_map(|t| t.parse().ok())
        .collect();
    if numbers.len() < 6 || numbers.len() % 2 != 0 {
        return None;
    }
    let mut points: Vec<(f64, f64)> = numbers.chunks(2).map(|c| (c[0], c[1])).collect();
    if points.len() > 3 && points.first() == points.last() {
        points.pop();
    }
    (points.len() >= 3).then_some(points)
}

/// The polygon with every corner rounded by up to `radius` (limited to half of each edge).
fn round_polygon(d: &str, radius: f64) -> String {
    let Some(points) = polygon_points(d) else {
        return d.to_string();
    };
    const K: f64 = 0.552_284_749_830_793_4;
    let n = points.len();
    let mut out = String::new();
    for i in 0..n {
        let (prev, cur, next) = (points[(i + n - 1) % n], points[i], points[(i + 1) % n]);
        let (dx0, dy0) = (prev.0 - cur.0, prev.1 - cur.1);
        let (dx1, dy1) = (next.0 - cur.0, next.1 - cur.1);
        let (l0, l1) = (dx0.hypot(dy0).max(1e-9), dx1.hypot(dy1).max(1e-9));
        let cut = radius.min(l0 / 2.0).min(l1 / 2.0);
        let start = (cur.0 + dx0 / l0 * cut, cur.1 + dy0 / l0 * cut);
        let end = (cur.0 + dx1 / l1 * cut, cur.1 + dy1 / l1 * cut);
        let c1 = (start.0 + (cur.0 - start.0) * K, start.1 + (cur.1 - start.1) * K);
        let c2 = (end.0 + (cur.0 - end.0) * K, end.1 + (cur.1 - end.1) * K);
        out.push_str(&format!("{}{} {}", if i == 0 { "M" } else { "L" }, num(start.0), num(start.1)));
        out.push_str(&format!("C{} {} {} {} {} {}", num(c1.0), num(c1.1), num(c2.0), num(c2.1), num(end.0), num(end.1)));
    }
    out.push('Z');
    out
}

/// True when the path is a single axis-aligned rectangle that covers the whole `w` x `h` box.
/// Whether some point of the path lies at (or within half a pixel of) a corner of the `w` x `h` box.
fn path_reaches_box_corner(d: &str, w: f64, h: f64) -> bool {
    let numbers: Vec<f64> = d
        .split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
        .filter(|t| !t.is_empty())
        .filter_map(|t| t.parse().ok())
        .collect();
    let near = |v: f64, edge: f64| v.abs() < 0.5 || (v - edge).abs() < 0.5;
    numbers.chunks(2).any(|c| c.len() == 2 && near(c[0], w) && near(c[1], h))
}

fn path_is_box(d: &str, w: f64, h: f64) -> bool {
    let numbers: Vec<f64> = d
        .split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
        .filter(|t| !t.is_empty())
        .filter_map(|t| t.parse().ok())
        .collect();
    if d.matches('M').count() != 1 || numbers.len() < 8 || numbers.len() % 2 != 0 {
        return false;
    }
    let tol = 0.01;
    let on = |v: f64, edge: f64| (v - 0.0).abs() < tol || (v - edge).abs() < tol;
    let pts: Vec<(f64, f64)> = numbers.chunks(2).map(|c| (c[0], c[1])).collect();
    pts.iter().all(|&(x, y)| on(x, w) && on(y, h))
        && pts.iter().any(|&(x, _)| (x - w).abs() < tol)
        && pts.iter().any(|&(_, y)| (y - h).abs() < tol)
}

/// Visio's built-in line patterns 2-23 as run/gap lengths in line-weight units, measured from Visio 16
///; the pattern scales with the line weight.
fn line_pattern_units(pattern: u32) -> Option<&'static [f64]> {
    Some(match pattern {
        2 => &[8.0, 4.0],
        3 => &[1.0, 4.0],
        4 => &[8.0, 4.0, 1.0, 4.0],
        5 => &[8.0, 4.0, 1.0, 4.0, 1.0, 4.0],
        6 => &[8.0, 4.0, 8.0, 4.0, 1.0, 4.0],
        7 => &[20.0, 4.0, 8.0, 4.0],
        8 => &[20.0, 4.0, 8.0, 4.0, 8.0, 4.0],
        9 => &[4.0, 2.0],
        10 => &[1.0, 2.0],
        11 => &[4.0, 2.0, 1.0, 2.0],
        12 => &[4.0, 2.0, 1.0, 2.0, 1.0, 2.0],
        13 => &[4.0, 2.0, 4.0, 2.0, 1.0, 2.0],
        14 => &[10.0, 2.0, 4.0, 2.0],
        15 => &[10.0, 2.0, 4.0, 2.0, 4.0, 2.0],
        16 => &[16.0, 8.0],
        17 => &[1.0, 8.0],
        18 => &[16.0, 8.0, 1.0, 8.0],
        19 => &[16.0, 8.0, 1.0, 8.0, 1.0, 8.0],
        20 => &[16.0, 8.0, 16.0, 8.0, 1.0, 8.0],
        21 => &[40.0, 8.0, 16.0, 8.0],
        22 => &[40.0, 8.0, 16.0, 8.0, 16.0, 8.0],
        23 => &[2.0, 1.0],
        _ => return None,
    })
}

fn dash_pattern(value: Option<&str>, weight: f64) -> Option<String> {
    // A `+flat` / `+square` suffix is the line cap, not part of the dash name.
    let name = value.unwrap_or("solid").split('+').next().unwrap_or("solid");
    if let Some(number) = name.strip_prefix("pat").and_then(|n| n.parse::<u32>().ok()) {
        let units = line_pattern_units(number)?;
        return Some(
            units
                .iter()
                .map(|unit| num((unit * weight).max(0.01)))
                .collect::<Vec<_>>()
                .join(" "),
        );
    }
    match name {
        "dashed" => Some("6 3".to_string()),
        "dotted" => Some("1 3".to_string()),
        "dashFine" => Some("2 2".to_string()),
        _ => None,
    }
}

fn num(value: f64) -> String {
    let rounded = (value * 1000.0).round() / 1000.0;
    if rounded == 0.0 {
        return "0".to_string();
    }
    if rounded.fract() == 0.0 {
        return format!("{}", rounded as i64);
    }
    let mut text = format!("{rounded:.3}");
    while text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    text
}

fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}
