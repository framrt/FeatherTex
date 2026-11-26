    use crate::theme;
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer::{self, Quad};
use iced::advanced::widget::{self, Tree, Widget};
use iced::advanced::{Clipboard, Shell};
use iced::event::{self, Event};
use iced::mouse;
use iced::{Background, Border, Color, Element, Length, Point, Rectangle, Shadow, Size, Theme};
use std::collections::HashMap;
use std::f32::consts::PI;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

const CLOUD_POINTS: usize = 48;
const GRID_DIM: usize = 16;
const GRID_LEN: usize = GRID_DIM * GRID_DIM;
const MAX_LEARNED_PER_SYMBOL: usize = 12;
const MAX_LEARNED_TOTAL: usize = 800;

#[derive(Debug, Clone)]
pub struct MatchCandidate {
    pub glyph: &'static str,
    pub display_cmd: &'static str,
    pub insert: &'static str,
    pub description: &'static str,
    pub package: Option<&'static str>,
    pub confidence: u8,
}

#[derive(Debug, Clone, Copy)]
struct CloudPoint {
    x: f32,
    y: f32,
    tx: f32,
    ty: f32,
}

/// Direction-invariant shape descriptor shared by user drawings and templates.
struct Features {
    is_dot: bool,
    stroke_count: usize,
    cloud: Vec<CloudPoint>,
    grid: [f32; GRID_LEN],
    /// Free stroke endpoints (closed loops contribute none) in normalized coordinates.
    endpoints: Vec<(f32, f32)>,
    /// ln(width / height) of the bounding box (clamped), distinguishes `-`, `|`, `<`, `⟨`...
    aspect: f32,
}

struct CompiledTemplate {
    glyph: &'static str,
    display_cmd: &'static str,
    insert: &'static str,
    description: &'static str,
    package: Option<&'static str>,
    learned: bool,
    feat: Features,
}

struct LearnedSample {
    norm_strokes: Vec<Vec<(f32, f32)>>,
    tpl: CompiledTemplate,
}

fn points_to_tuples(strokes: &[Vec<Point>]) -> Vec<Vec<(f32, f32)>> {
    strokes
        .iter()
        .filter(|s| !s.is_empty())
        .map(|s| s.iter().map(|p| (p.x, p.y)).collect())
        .collect()
}

fn score_template(user: &Features, tpl: &Features) -> f32 {
    if user.is_dot {
        return if tpl.is_dot { 0.96 } else { 0.15 * cosine_sim(&user.grid, &tpl.grid) };
    }
    if tpl.is_dot {
        return 0.10;
    }

    let g_sim = cosine_sim(&user.grid, &tpl.grid);
    let c_dist = bidirectional_cloud_distance(&user.cloud, &tpl.cloud);
    let c_sim = (-3.2 * c_dist).exp();
    let e_sim = endpoint_similarity(&user.endpoints, &tpl.endpoints);
    let a_sim = (-0.9 * (user.aspect - tpl.aspect).abs()).exp();
    let stroke_diff = user.stroke_count.abs_diff(tpl.stroke_count).min(4) as f32;
    let stroke_factor = 1.0 - 0.045 * stroke_diff;

    (0.30 * g_sim + 0.34 * c_sim + 0.21 * e_sim + 0.15 * a_sim) * stroke_factor
}

/// Recognizes freehand mouse strokes and returns the top matching LaTeX symbols.
/// Combines the built-in vector templates with samples learned from the user's own picks.
pub fn recognize(strokes: &[Vec<Point>], limit: usize) -> Vec<MatchCandidate> {
    let valid_strokes = points_to_tuples(strokes);
    if valid_strokes.is_empty() {
        return Vec::new();
    }

    let (norm_strokes, is_dot) = normalize_strokes(&valid_strokes);
    let user = extract_features(&norm_strokes, is_dot);

    type Best = (&'static str, &'static str, &'static str, Option<&'static str>, f32);
    let mut best_by_cmd: HashMap<&'static str, Best> = HashMap::new();

    let mut consider = |tpl: &CompiledTemplate| {
        let mut score = score_template(&user, &tpl.feat);
        if tpl.learned {
            // The user's own handwriting is the most reliable reference.
            score = (score * 1.06).min(1.0);
        }
        let entry = best_by_cmd.entry(tpl.display_cmd).or_insert((
            tpl.glyph,
            tpl.insert,
            tpl.description,
            tpl.package,
            f32::MIN,
        ));
        if score > entry.4 {
            entry.4 = score;
        }
    };

    for tpl in compiled_templates() {
        consider(tpl);
    }
    if let Ok(learned) = learned_store().lock() {
        for sample in learned.iter() {
            consider(&sample.tpl);
        }
    }

    let mut ranked: Vec<(&'static str, Best)> = best_by_cmd.into_iter().collect();
    ranked.sort_by(|a, b| b.1 .4.total_cmp(&a.1 .4));
    ranked.truncate(limit);

    let top_score = ranked.first().map(|(_, b)| b.4).unwrap_or(1.0).max(0.01);

    ranked
        .into_iter()
        .map(|(cmd, (glyph, insert, description, package, score))| {
            let calibrated = ((score * 0.72 + (score / top_score) * 0.28) * 100.0)
                .clamp(5.0, 99.0)
                .round() as u8;
            MatchCandidate {
                glyph,
                display_cmd: cmd,
                insert,
                description,
                package,
                confidence: calibrated,
            }
        })
        .collect()
}

/// Stores the user's drawing as an additional personal template for `display_cmd`
/// (called when the user picks a candidate), so recognition adapts to their handwriting.
pub fn learn(strokes: &[Vec<Point>], display_cmd: &str) {
    let Some(base) = compiled_templates()
        .iter()
        .find(|t| t.display_cmd == display_cmd)
    else {
        return;
    };
    let raw = points_to_tuples(strokes);
    if raw.is_empty() {
        return;
    }
    let (norm, is_dot) = normalize_strokes(&raw);
    if is_dot != base.feat.is_dot {
        return;
    }

    let sample = LearnedSample {
        tpl: CompiledTemplate {
            glyph: base.glyph,
            display_cmd: base.display_cmd,
            insert: base.insert,
            description: base.description,
            package: base.package,
            learned: true,
            feat: extract_features(&norm, is_dot),
        },
        norm_strokes: norm,
    };

    let Ok(mut store) = learned_store().lock() else {
        return;
    };
    store.push(sample);

    let same: Vec<usize> = store
        .iter()
        .enumerate()
        .filter(|(_, s)| s.tpl.display_cmd == base.display_cmd)
        .map(|(i, _)| i)
        .collect();
    if same.len() > MAX_LEARNED_PER_SYMBOL {
        store.remove(same[0]);
    }
    while store.len() > MAX_LEARNED_TOTAL {
        store.remove(0);
    }

    persist_learned(&store);
}

/// Number of personal samples learned so far.
pub fn learned_count() -> usize {
    learned_store().lock().map(|s| s.len()).unwrap_or(0)
}

fn learned_store() -> &'static Mutex<Vec<LearnedSample>> {
    static STORE: OnceLock<Mutex<Vec<LearnedSample>>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(load_learned()))
}

fn learned_path() -> Option<PathBuf> {
    if cfg!(test) {
        return None;
    }
    let base = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("feathertex").join("match_samples.txt"))
}

fn load_learned() -> Vec<LearnedSample> {
    let Some(path) = learned_path() else {
        return Vec::new();
    };
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };

    let templates = compiled_templates();
    let mut out = Vec::new();
    for line in content.lines() {
        let Some((cmd, body)) = line.split_once('\t') else {
            continue;
        };
        let Some(base) = templates.iter().find(|t| t.display_cmd == cmd) else {
            continue;
        };
        let strokes: Vec<Vec<(f32, f32)>> = body
            .split(';')
            .map(|stroke| {
                stroke
                    .split_whitespace()
                    .filter_map(|pt| {
                        let (x, y) = pt.split_once(',')?;
                        Some((x.parse().ok()?, y.parse().ok()?))
                    })
                    .collect::<Vec<(f32, f32)>>()
            })
            .filter(|s| !s.is_empty())
            .collect();
        if strokes.is_empty() {
            continue;
        }
        let is_dot = base.feat.is_dot;
        out.push(LearnedSample {
            tpl: CompiledTemplate {
                glyph: base.glyph,
                display_cmd: base.display_cmd,
                insert: base.insert,
                description: base.description,
                package: base.package,
                learned: true,
                feat: extract_features(&strokes, is_dot),
            },
            norm_strokes: strokes,
        });
    }
    out
}

fn persist_learned(store: &[LearnedSample]) {
    let Some(path) = learned_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut body = String::new();
    for sample in store {
        body.push_str(sample.tpl.display_cmd);
        body.push('\t');
        let strokes: Vec<String> = sample
            .norm_strokes
            .iter()
            .map(|s| {
                s.iter()
                    .map(|(x, y)| format!("{x:.3},{y:.3}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect();
        body.push_str(&strokes.join(";"));
        body.push('\n');
    }
    let _ = std::fs::write(path, body);
}

/// Interactive freehand drawing pad built with `iced::advanced::Widget` (no extra crates needed).
/// Renders smooth SDF circular brush strokes and emits a message when a stroke completes.
pub struct DrawingPad<'a, Message> {
    strokes: &'a [Vec<Point>],
    dark_mode: bool,
    on_stroke: Box<dyn Fn(Vec<Point>) -> Message + 'a>,
}

impl<'a, Message> DrawingPad<'a, Message> {
    pub fn new(
        strokes: &'a [Vec<Point>],
        dark_mode: bool,
        on_stroke: impl Fn(Vec<Point>) -> Message + 'a,
    ) -> Self {
        Self {
            strokes,
            dark_mode,
            on_stroke: Box::new(on_stroke),
        }
    }
}

#[derive(Default)]
struct PadState {
    active_stroke: Option<Vec<Point>>,
}

impl<'a, Message, Renderer> Widget<Message, Theme, Renderer> for DrawingPad<'a, Message>
where
    Renderer: renderer::Renderer,
{
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<PadState>()
    }

    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(PadState::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fixed(236.0))
    }

    fn layout(
        &self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let size = limits.resolve(Length::Fill, Length::Fixed(236.0), Size::new(280.0, 236.0));
        layout::Node::new(size)
    }

    fn on_event(
        &mut self,
        tree: &mut Tree,
        event: Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) -> event::Status {
        let state = tree.state.downcast_mut::<PadState>();
        let bounds = layout.bounds();

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(pos) = cursor.position_in(bounds) {
                    state.active_stroke = Some(vec![pos]);
                    return event::Status::Captured;
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if let Some(active) = &mut state.active_stroke {
                    if let Some(pos) = cursor.position() {
                        let local = Point::new(
                            (pos.x - bounds.x).clamp(2.0, bounds.width - 2.0),
                            (pos.y - bounds.y).clamp(2.0, bounds.height - 2.0),
                        );
                        let should_push = active
                            .last()
                            .map(|last| {
                                let dx = local.x - last.x;
                                let dy = local.y - last.y;
                                dx * dx + dy * dy >= 9.0
                            })
                            .unwrap_or(true);
                        if should_push {
                            active.push(local);
                        }
                        return event::Status::Captured;
                    }
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if let Some(finished) = state.active_stroke.take() {
                    if !finished.is_empty() {
                        shell.publish((self.on_stroke)(finished));
                    }
                    return event::Status::Captured;
                }
            }
            _ => {}
        }

        event::Status::Ignored
    }

    fn mouse_interaction(
        &self,
        _tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        if cursor.is_over(layout.bounds()) {
            mouse::Interaction::Crosshair
        } else {
            mouse::Interaction::default()
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<PadState>();
        let bounds = layout.bounds();
        let pal = theme::palette_for(self.dark_mode);

        // Background pad
        renderer.fill_quad(
            Quad {
                bounds,
                border: Border {
                    color: pal.border,
                    width: 1.0,
                    radius: 10.0.into(),
                },
                shadow: Shadow::default(),
            },
            Background::Color(pal.bg_base),
        );

        // Subtle center guide crosshair
        let guide_color = if self.dark_mode {
            Color::from_rgba(1.0, 1.0, 1.0, 0.06)
        } else {
            Color::from_rgba(0.35, 0.28, 0.18, 0.10)
        };

        let cx = (bounds.x + bounds.width * 0.5).round();
        let cy = (bounds.y + bounds.height * 0.5).round();

        renderer.fill_quad(
            Quad {
                bounds: Rectangle::new(
                    Point::new(bounds.x + 12.0, cy),
                    Size::new((bounds.width - 24.0).max(0.0), 1.0),
                ),
                ..Quad::default()
            },
            Background::Color(guide_color),
        );
        renderer.fill_quad(
            Quad {
                bounds: Rectangle::new(
                    Point::new(cx, bounds.y + 12.0),
                    Size::new(1.0, (bounds.height - 24.0).max(0.0)),
                ),
                ..Quad::default()
            },
            Background::Color(guide_color),
        );

        let ink = pal.text_primary;
        let radius = 2.6_f32;

        let draw_stroke = |renderer: &mut Renderer, stroke: &[Point]| {
            if stroke.is_empty() {
                return;
            }
            if stroke.len() == 1 {
                let p = stroke[0];
                stamp_dot(renderer, bounds.x + p.x, bounds.y + p.y, radius * 1.15, ink);
                return;
            }
            for seg in stroke.windows(2) {
                let a = seg[0];
                let b = seg[1];
                let dx = b.x - a.x;
                let dy = b.y - a.y;
                let dist = (dx * dx + dy * dy).sqrt();
                let steps = (dist / 1.6).ceil().max(1.0) as usize;
                for s in 0..=steps {
                    let t = s as f32 / steps as f32;
                    let x = bounds.x + a.x + dx * t;
                    let y = bounds.y + a.y + dy * t;
                    stamp_dot(renderer, x, y, radius, ink);
                }
            }
        };

        for stroke in self.strokes {
            draw_stroke(renderer, stroke);
        }
        if let Some(active) = &state.active_stroke {
            draw_stroke(renderer, active);
        }
    }
}

fn stamp_dot<R: renderer::Renderer>(renderer: &mut R, x: f32, y: f32, r: f32, color: Color) {
    renderer.fill_quad(
        Quad {
            bounds: Rectangle::new(Point::new(x - r, y - r), Size::new(r * 2.0, r * 2.0)),
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: r.into(),
            },
            shadow: Shadow::default(),
        },
        Background::Color(color),
    );
}

impl<'a, Message: 'a> From<DrawingPad<'a, Message>> for Element<'a, Message> {
    fn from(pad: DrawingPad<'a, Message>) -> Self {
        Element::new(pad)
    }
}

// ============================================================================
// Stroke Normalization, Oriented Point-Cloud ($P+) & Blurred Grid Descriptor
// ============================================================================

fn normalize_strokes(strokes: &[Vec<(f32, f32)>]) -> (Vec<Vec<(f32, f32)>>, bool) {
    let mut min_x = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_y = f32::NEG_INFINITY;

    for s in strokes {
        for &(x, y) in s {
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
        }
    }

    let w = max_x - min_x;
    let h = max_y - min_y;
    let max_dim = w.max(h);

    if max_dim < 1e-3 || (strokes.len() == 1 && strokes[0].len() <= 2 && max_dim < 6.0) {
        return (vec![vec![(0.0, 0.0)]], true);
    }

    let cx = (min_x + max_x) * 0.5;
    let cy = (min_y + max_y) * 0.5;
    let inv = 1.0 / max_dim;

    let norm = strokes
        .iter()
        .map(|s| {
            s.iter()
                .map(|&(x, y)| ((x - cx) * inv, (y - cy) * inv))
                .collect()
        })
        .collect();

    (norm, false)
}

fn stroke_length(s: &[(f32, f32)]) -> f32 {
    s.windows(2)
        .map(|w| ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt())
        .sum()
}

fn resample_stroke(s: &[(f32, f32)], count: usize) -> Vec<CloudPoint> {
    if s.is_empty() || count == 0 {
        return Vec::new();
    }
    if s.len() == 1 || count == 1 {
        return vec![CloudPoint {
            x: s[0].0,
            y: s[0].1,
            tx: 0.0,
            ty: 0.0,
        }];
    }

    let total_len = stroke_length(s);
    if total_len < 1e-6 {
        return vec![
            CloudPoint {
                x: s[0].0,
                y: s[0].1,
                tx: 0.0,
                ty: 0.0,
            };
            count
        ];
    }

    let step = total_len / ((count - 1) as f32);
    let mut pts: Vec<(f32, f32)> = Vec::with_capacity(count);
    pts.push(s[0]);

    let mut accum = 0.0_f32;
    let mut prev = s[0];
    let mut i = 1usize;

    while i < s.len() && pts.len() < count {
        let curr = s[i];
        let seg_len = ((curr.0 - prev.0).powi(2) + (curr.1 - prev.1).powi(2)).sqrt();
        if accum + seg_len >= step && seg_len > 1e-6 {
            let t = (step - accum) / seg_len;
            let nx = prev.0 + t * (curr.0 - prev.0);
            let ny = prev.1 + t * (curr.1 - prev.1);
            pts.push((nx, ny));
            prev = (nx, ny);
            accum = 0.0;
        } else {
            accum += seg_len;
            prev = curr;
            i += 1;
        }
    }

    while pts.len() < count {
        pts.push(*s.last().unwrap());
    }

    let mut out = Vec::with_capacity(count);
    for idx in 0..pts.len() {
        let p_prev = pts[idx.saturating_sub(1)];
        let p_next = pts[(idx + 1).min(pts.len() - 1)];
        let dx = p_next.0 - p_prev.0;
        let dy = p_next.1 - p_prev.1;
        let d = (dx * dx + dy * dy).sqrt();
        let (tx, ty) = if d > 1e-6 { (dx / d, dy / d) } else { (0.0, 0.0) };
        out.push(CloudPoint {
            x: pts[idx].0,
            y: pts[idx].1,
            tx,
            ty,
        });
    }
    out
}

fn resample_cloud(strokes: &[Vec<(f32, f32)>], total_n: usize) -> Vec<CloudPoint> {
    let lengths: Vec<f32> = strokes.iter().map(|s| stroke_length(s).max(0.05)).collect();
    let sum_len: f32 = lengths.iter().sum::<f32>().max(1e-6);

    let mut counts: Vec<usize> = lengths
        .iter()
        .map(|&l| ((l / sum_len) * (total_n as f32)).round().max(2.0) as usize)
        .collect();

    // Adjust total count to match `total_n`
    let mut sum_c: usize = counts.iter().sum();
    while sum_c > total_n {
        if let Some((idx, _)) = counts.iter().enumerate().max_by_key(|&(_, &c)| c) {
            if counts[idx] > 2 {
                counts[idx] -= 1;
                sum_c -= 1;
            } else {
                break;
            }
        }
    }
    while sum_c < total_n {
        if let Some((idx, _)) = counts.iter().enumerate().max_by_key(|&(_, &c)| c) {
            counts[idx] += 1;
            sum_c += 1;
        } else {
            break;
        }
    }

    let mut cloud = Vec::with_capacity(total_n);
    for (stroke, count) in strokes.iter().zip(counts) {
        cloud.extend(resample_stroke(stroke, count));
    }
    cloud
}

fn rasterize_grid(strokes: &[Vec<(f32, f32)>]) -> [f32; GRID_LEN] {
    let mut raw = [0.0_f32; GRID_LEN];

    let splat = |grid: &mut [f32; GRID_LEN], x: f32, y: f32| {
        // Map [-0.5, 0.5] into [1.5, GRID_DIM - 2.5] so strokes have clean margin for blur
        let gx = (x + 0.5) * ((GRID_DIM - 4) as f32) + 1.5;
        let gy = (y + 0.5) * ((GRID_DIM - 4) as f32) + 1.5;
        let ix = gx.floor() as isize;
        let iy = gy.floor() as isize;
        let fx = gx - ix as f32;
        let fy = gy - iy as f32;

        for dy in 0..=1 {
            for dx in 0..=1 {
                let px = ix + dx;
                let py = iy + dy;
                if px >= 0 && px < GRID_DIM as isize && py >= 0 && py < GRID_DIM as isize {
                    let wx = if dx == 0 { 1.0 - fx } else { fx };
                    let wy = if dy == 0 { 1.0 - fy } else { fy };
                    let cell = (py as usize) * GRID_DIM + (px as usize);
                    grid[cell] = (grid[cell] + wx * wy).min(1.0);
                }
            }
        }
    };

    for s in strokes {
        if s.len() == 1 {
            splat(&mut raw, s[0].0, s[0].1);
            continue;
        }
        for seg in s.windows(2) {
            let (x0, y0) = seg[0];
            let (x1, y1) = seg[1];
            let dist = ((x1 - x0).powi(2) + (y1 - y0).powi(2)).sqrt();
            let steps = (dist * 24.0).ceil().max(1.0) as usize;
            for k in 0..=steps {
                let t = k as f32 / steps as f32;
                splat(&mut raw, x0 + (x1 - x0) * t, y0 + (y1 - y0) * t);
            }
        }
    }

    // 3x3 Gaussian blur for generous tolerance to hand-drawn jitter
    let mut blurred = [0.0_f32; GRID_LEN];
    const K: [[f32; 3]; 3] = [
        [0.0625, 0.125, 0.0625],
        [0.125, 0.25, 0.125],
        [0.0625, 0.125, 0.0625],
    ];
    for y in 0..GRID_DIM {
        for x in 0..GRID_DIM {
            let mut sum = 0.0_f32;
            for ky in 0..3 {
                for kx in 0..3 {
                    let sx = (x as isize + kx as isize - 1).clamp(0, GRID_DIM as isize - 1) as usize;
                    let sy = (y as isize + ky as isize - 1).clamp(0, GRID_DIM as isize - 1) as usize;
                    sum += raw[sy * GRID_DIM + sx] * K[ky][kx];
                }
            }
            blurred[y * GRID_DIM + x] = sum;
        }
    }

    let norm: f32 = blurred.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm > 1e-6 {
        for v in &mut blurred {
            *v /= norm;
        }
    }
    blurred
}

fn cosine_sim(a: &[f32; GRID_LEN], b: &[f32; GRID_LEN]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum::<f32>().clamp(0.0, 1.0)
}

fn point_cost(a: &CloudPoint, b: &CloudPoint) -> f32 {
    let pos = ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt();
    let dot = (a.tx * b.tx + a.ty * b.ty).abs().min(1.0);
    pos + 0.20 * (1.0 - dot)
}

fn directed_cloud_distance(from: &[CloudPoint], to: &[CloudPoint]) -> f32 {
    if from.is_empty() || to.is_empty() {
        return 1.0;
    }
    let mut sum = 0.0_f32;
    for p in from {
        let mut best = f32::INFINITY;
        for q in to {
            let d = point_cost(p, q);
            if d < best {
                best = d;
            }
        }
        sum += best;
    }
    sum / (from.len() as f32)
}

fn bidirectional_cloud_distance(a: &[CloudPoint], b: &[CloudPoint]) -> f32 {
    0.5 * (directed_cloud_distance(a, b) + directed_cloud_distance(b, a))
}

fn extract_features(norm_strokes: &[Vec<(f32, f32)>], is_dot: bool) -> Features {
    let mut min_x = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for s in norm_strokes {
        for &(x, y) in s {
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
        }
    }
    let w = (max_x - min_x).max(0.08);
    let h = (max_y - min_y).max(0.08);
    let aspect = (w / h).ln().clamp(-2.5, 2.5);

    let mut endpoints = Vec::new();
    for s in norm_strokes {
        if s.len() < 2 || stroke_length(s) < 0.06 {
            continue; // dots have no meaningful endpoints
        }
        let a = s[0];
        let b = s[s.len() - 1];
        let gap = ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt();
        if gap < 0.16 {
            continue; // closed loop (∞, ○, □ ...)
        }
        endpoints.push(a);
        endpoints.push(b);
    }

    Features {
        is_dot,
        stroke_count: norm_strokes.len(),
        cloud: resample_cloud(norm_strokes, CLOUD_POINTS),
        grid: rasterize_grid(norm_strokes),
        endpoints,
        aspect,
    }
}

fn chamfer_points(from: &[(f32, f32)], to: &[(f32, f32)]) -> f32 {
    let mut sum = 0.0;
    for p in from {
        let best = to
            .iter()
            .map(|q| ((p.0 - q.0).powi(2) + (p.1 - q.1).powi(2)).sqrt())
            .fold(f32::INFINITY, f32::min);
        sum += best;
    }
    sum / from.len().max(1) as f32
}

fn endpoint_similarity(a: &[(f32, f32)], b: &[(f32, f32)]) -> f32 {
    match (a.is_empty(), b.is_empty()) {
        (true, true) => 1.0,
        (true, false) | (false, true) => 0.40,
        _ => {
            let d = 0.5 * (chamfer_points(a, b) + chamfer_points(b, a));
            let count_diff = (a.len().abs_diff(b.len()) / 2).min(4) as f32;
            (-3.5 * d).exp() * (1.0 - 0.08 * count_diff)
        }
    }
}

/// Chaikin corner-cutting: turns coarse polylines into smooth hand-like curves (keeps endpoints).
fn smooth(pts: &[(f32, f32)], iterations: usize) -> Vec<(f32, f32)> {
    let mut cur = pts.to_vec();
    for _ in 0..iterations {
        if cur.len() < 3 {
            break;
        }
        let mut next = Vec::with_capacity(cur.len() * 2);
        next.push(cur[0]);
        for w in cur.windows(2) {
            let (a, b) = (w[0], w[1]);
            next.push((0.75 * a.0 + 0.25 * b.0, 0.75 * a.1 + 0.25 * b.1));
            next.push((0.25 * a.0 + 0.75 * b.0, 0.25 * a.1 + 0.75 * b.1));
        }
        next.push(*cur.last().unwrap());
        cur = next;
    }
    cur
}

// ============================================================================
// Vector Template Builders & Curated LaTeX Symbol Library
// ============================================================================

fn line(x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<(f32, f32)> {
    vec![(x0, y0), (x1, y1)]
}

fn poly(pts: &[(f32, f32)]) -> Vec<(f32, f32)> {
    pts.to_vec()
}

fn arc(cx: f32, cy: f32, rx: f32, ry: f32, start_deg: f32, end_deg: f32, steps: usize) -> Vec<(f32, f32)> {
    (0..=steps)
        .map(|i| {
            let t = i as f32 / steps as f32;
            let rad = (start_deg + (end_deg - start_deg) * t) * PI / 180.0;
            (cx + rx * rad.cos(), cy + ry * rad.sin())
        })
        .collect()
}

fn compile_tpl(
    glyph: &'static str,
    display_cmd: &'static str,
    insert: &'static str,
    description: &'static str,
    package: Option<&'static str>,
    strokes: Vec<Vec<(f32, f32)>>,
) -> CompiledTemplate {
    let (norm, is_dot) = normalize_strokes(&strokes);
    CompiledTemplate {
        glyph,
        display_cmd,
        insert,
        description,
        package,
        learned: false,
        feat: extract_features(&norm, is_dot),
    }
}

fn compiled_templates() -> &'static [CompiledTemplate] {
    static TEMPLATES: OnceLock<Vec<CompiledTemplate>> = OnceLock::new();
    TEMPLATES.get_or_init(build_templates)
}

fn build_templates() -> Vec<CompiledTemplate> {
    let mut t = Vec::with_capacity(110);

    // ---- Logic ----
    // \forall (V + bar, 2-stroke and 3-stroke variants)
    t.push(compile_tpl(
        "∀", "\\forall", "\\forall ", "Per ogni (quantificatore universale)", None,
        vec![poly(&[(0.0, 0.0), (0.5, 1.0), (1.0, 0.0)]), line(0.22, 0.45, 0.78, 0.45)],
    ));
    t.push(compile_tpl(
        "∀", "\\forall", "\\forall ", "Per ogni (quantificatore universale)", None,
        vec![line(0.0, 0.0, 0.5, 1.0), line(1.0, 0.0, 0.5, 1.0), line(0.22, 0.45, 0.78, 0.45)],
    ));
    // \exists (backwards E)
    t.push(compile_tpl(
        "∃", "\\exists", "\\exists ", "Esiste (quantificatore esistenziale)", None,
        vec![poly(&[(0.1, 0.0), (0.9, 0.0), (0.9, 1.0), (0.1, 1.0)]), line(0.15, 0.5, 0.9, 0.5)],
    ));
    t.push(compile_tpl(
        "∃", "\\exists", "\\exists ", "Esiste (quantificatore esistenziale)", None,
        vec![line(0.1, 0.0, 0.9, 0.0), line(0.9, 0.0, 0.9, 1.0), line(0.1, 1.0, 0.9, 1.0), line(0.15, 0.5, 0.9, 0.5)],
    ));
    // \neg
    t.push(compile_tpl(
        "¬", "\\neg", "\\neg ", "Negazione logica", None,
        vec![poly(&[(0.0, 0.35), (1.0, 0.35), (1.0, 0.85)])],
    ));
    // \land
    t.push(compile_tpl(
        "∧", "\\land", "\\land ", "Congiunzione logica (AND)", None,
        vec![poly(&[(0.0, 1.0), (0.5, 0.0), (1.0, 1.0)])],
    ));
    // \lor
    t.push(compile_tpl(
        "∨", "\\lor", "\\lor ", "Disgiunzione logica (OR)", None,
        vec![poly(&[(0.0, 0.0), (0.5, 1.0), (1.0, 0.0)])],
    ));
    // \to
    t.push(compile_tpl(
        "→", "\\to", "\\to ", "Freccia a destra / implica", None,
        vec![line(0.0, 0.5, 1.0, 0.5), poly(&[(0.65, 0.2), (1.0, 0.5), (0.65, 0.8)])],
    ));
    // \leftarrow
    t.push(compile_tpl(
        "←", "\\leftarrow", "\\leftarrow ", "Freccia a sinistra", None,
        vec![line(1.0, 0.5, 0.0, 0.5), poly(&[(0.35, 0.2), (0.0, 0.5), (0.35, 0.8)])],
    ));
    // \leftrightarrow
    t.push(compile_tpl(
        "↔", "\\leftrightarrow", "\\leftrightarrow ", "Freccia bidirezionale", None,
        vec![
            line(0.0, 0.5, 1.0, 0.5),
            poly(&[(0.28, 0.25), (0.0, 0.5), (0.28, 0.75)]),
            poly(&[(0.72, 0.25), (1.0, 0.5), (0.72, 0.75)]),
        ],
    ));
    // \Rightarrow
    t.push(compile_tpl(
        "⇒", "\\Rightarrow", "\\Rightarrow ", "Implicazione logica", None,
        vec![
            line(0.0, 0.35, 0.82, 0.35),
            line(0.0, 0.65, 0.82, 0.65),
            poly(&[(0.62, 0.15), (1.0, 0.5), (0.62, 0.85)]),
        ],
    ));
    // \Leftrightarrow
    t.push(compile_tpl(
        "⇔", "\\Leftrightarrow", "\\Leftrightarrow ", "Se e solo se (equivalenza)", None,
        vec![
            line(0.18, 0.35, 0.82, 0.35),
            line(0.18, 0.65, 0.82, 0.65),
            poly(&[(0.32, 0.18), (0.0, 0.5), (0.32, 0.82)]),
            poly(&[(0.68, 0.18), (1.0, 0.5), (0.68, 0.82)]),
        ],
    ));
    // \mapsto
    t.push(compile_tpl(
        "↦", "\\mapsto", "\\mapsto ", "Mappa elemento (mapsto)", None,
        vec![
            line(0.0, 0.25, 0.0, 0.75),
            line(0.0, 0.5, 1.0, 0.5),
            poly(&[(0.7, 0.25), (1.0, 0.5), (0.7, 0.75)]),
        ],
    ));
    // \vdash
    t.push(compile_tpl(
        "⊢", "\\vdash", "\\vdash ", "Derivabilità sintattica", None,
        vec![line(0.1, 0.0, 0.1, 1.0), line(0.1, 0.5, 0.9, 0.5)],
    ));
    // \models
    t.push(compile_tpl(
        "⊨", "\\models", "\\models ", "Soddisfacibilità semantica", None,
        vec![line(0.1, 0.0, 0.1, 1.0), line(0.1, 0.35, 0.9, 0.35), line(0.1, 0.65, 0.9, 0.65)],
    ));
    // \top
    t.push(compile_tpl(
        "⊤", "\\top", "\\top ", "Vero / elemento massimo (top)", None,
        vec![line(0.0, 0.1, 1.0, 0.1), line(0.5, 0.1, 0.5, 1.0)],
    ));
    // \bot
    t.push(compile_tpl(
        "⊥", "\\bot", "\\bot ", "Falso / perpendicolare (bottom)", None,
        vec![line(0.5, 0.0, 0.5, 0.9), line(0.0, 0.9, 1.0, 0.9)],
    ));
    // \equiv
    t.push(compile_tpl(
        "≡", "\\equiv", "\\equiv ", "Equivalente / congruo", None,
        vec![line(0.0, 0.2, 1.0, 0.2), line(0.0, 0.5, 1.0, 0.5), line(0.0, 0.8, 1.0, 0.8)],
    ));
    // \Box
    t.push(compile_tpl(
        "□", "\\Box", "\\Box ", "Quadrato / scelta / QED", Some("amssymb"),
        vec![poly(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (0.0, 0.0)])],
    ));
    // \Diamond
    t.push(compile_tpl(
        "◇", "\\Diamond", "\\Diamond ", "Rombo / possibilità", Some("amssymb"),
        vec![poly(&[(0.5, 0.0), (1.0, 0.5), (0.5, 1.0), (0.0, 0.5), (0.5, 0.0)])],
    ));

    // ---- Sets & Relations ----
    // \in (C + horizontal bar)
    t.push(compile_tpl(
        "∈", "\\in", "\\in ", "Appartiene a", None,
        vec![arc(0.55, 0.5, 0.45, 0.48, 285.0, 75.0, 18), line(0.1, 0.5, 0.95, 0.5)],
    ));
    // \notin
    t.push(compile_tpl(
        "∉", "\\notin", "\\notin ", "Non appartiene a", None,
        vec![
            arc(0.55, 0.5, 0.45, 0.48, 285.0, 75.0, 18),
            line(0.1, 0.5, 0.95, 0.5),
            line(0.85, 0.05, 0.2, 0.95),
        ],
    ));
    // \subset
    t.push(compile_tpl(
        "⊂", "\\subset", "\\subset ", "Sottoinsieme proprio", None,
        vec![arc(0.6, 0.5, 0.5, 0.45, 290.0, 70.0, 18)],
    ));
    // \subseteq
    t.push(compile_tpl(
        "⊆", "\\subseteq", "\\subseteq ", "Sottoinsieme o uguale", None,
        vec![arc(0.6, 0.38, 0.5, 0.36, 290.0, 70.0, 18), line(0.12, 0.92, 0.92, 0.92)],
    ));
    // \supset
    t.push(compile_tpl(
        "⊃", "\\supset", "\\supset ", "Sovrainsieme proprio", None,
        vec![arc(0.4, 0.5, 0.5, 0.45, -110.0, 110.0, 18)],
    ));
    // \supseteq
    t.push(compile_tpl(
        "⊇", "\\supseteq", "\\supseteq ", "Sovrainsieme o uguale", None,
        vec![arc(0.4, 0.38, 0.5, 0.36, -110.0, 110.0, 18), line(0.08, 0.92, 0.88, 0.92)],
    ));
    // \cup
    t.push(compile_tpl(
        "∪", "\\cup", "\\cup ", "Unione insiemistica", None,
        vec![{
            let mut v = vec![(0.05, 0.0)];
            v.extend(arc(0.5, 0.55, 0.45, 0.45, 180.0, 0.0, 16));
            v.push((0.95, 0.0));
            v
        }],
    ));
    // \cap
    t.push(compile_tpl(
        "∩", "\\cap", "\\cap ", "Intersezione insiemistica", None,
        vec![{
            let mut v = vec![(0.05, 1.0)];
            v.extend(arc(0.5, 0.45, 0.45, 0.45, 180.0, 360.0, 16));
            v.push((0.95, 1.0));
            v
        }],
    ));
    // \emptyset
    t.push(compile_tpl(
        "∅", "\\emptyset", "\\emptyset", "Insieme vuoto", None,
        vec![arc(0.5, 0.5, 0.42, 0.42, 0.0, 360.0, 22), line(0.88, 0.05, 0.12, 0.95)],
    ));
    // \setminus
    t.push(compile_tpl(
        "∖", "\\setminus", "\\setminus ", "Differenza insiemistica", None,
        vec![line(0.15, 0.05, 0.85, 0.95)],
    ));
    // \sqsubseteq
    t.push(compile_tpl(
        "⊑", "\\sqsubseteq", "\\sqsubseteq ", "Ordine parziale / reticolo", Some("amssymb"),
        vec![poly(&[(0.9, 0.05), (0.1, 0.05), (0.1, 0.72), (0.9, 0.72)]), line(0.1, 0.95, 0.9, 0.95)],
    ));
    // \sqcup
    t.push(compile_tpl(
        "⊔", "\\sqcup", "\\sqcup ", "Estremo superiore (join)", None,
        vec![poly(&[(0.1, 0.05), (0.1, 0.95), (0.9, 0.95), (0.9, 0.05)])],
    ));
    // \sqcap
    t.push(compile_tpl(
        "⊓", "\\sqcap", "\\sqcap ", "Estremo inferiore (meet)", None,
        vec![poly(&[(0.1, 0.95), (0.1, 0.05), (0.9, 0.05), (0.9, 0.95)])],
    ));
    // \le
    t.push(compile_tpl(
        "≤", "\\le", "\\le ", "Minore o uguale", None,
        vec![poly(&[(0.9, 0.05), (0.1, 0.42), (0.9, 0.75)]), line(0.1, 0.95, 0.9, 0.95)],
    ));
    // \ge
    t.push(compile_tpl(
        "≥", "\\ge", "\\ge ", "Maggiore o uguale", None,
        vec![poly(&[(0.1, 0.05), (0.9, 0.42), (0.1, 0.75)]), line(0.1, 0.95, 0.9, 0.95)],
    ));
    // \neq
    t.push(compile_tpl(
        "≠", "\\neq", "\\neq ", "Diverso da", None,
        vec![line(0.05, 0.35, 0.95, 0.35), line(0.05, 0.65, 0.95, 0.65), line(0.75, 0.05, 0.25, 0.95)],
    ));
    // \approx
    t.push(compile_tpl(
        "≈", "\\approx", "\\approx ", "Circa uguale", None,
        vec![
            poly(&[(0.0, 0.32), (0.25, 0.15), (0.5, 0.32), (0.75, 0.48), (1.0, 0.32)]),
            poly(&[(0.0, 0.72), (0.25, 0.55), (0.5, 0.72), (0.75, 0.88), (1.0, 0.72)]),
        ],
    ));
    // \sim
    t.push(compile_tpl(
        "∼", "\\sim", "\\sim ", "Simile / equivalente", None,
        vec![poly(&[(0.0, 0.55), (0.25, 0.3), (0.5, 0.5), (0.75, 0.7), (1.0, 0.45)])],
    ));
    // \cong
    t.push(compile_tpl(
        "≅", "\\cong", "\\cong ", "Congruente / isomorfo", None,
        vec![
            poly(&[(0.0, 0.25), (0.25, 0.1), (0.5, 0.25), (0.75, 0.4), (1.0, 0.25)]),
            line(0.0, 0.58, 1.0, 0.58),
            line(0.0, 0.85, 1.0, 0.85),
        ],
    ));
    // \parallel
    t.push(compile_tpl(
        "∥", "\\parallel", "\\parallel ", "Parallelo / norma", None,
        vec![line(0.35, 0.0, 0.35, 1.0), line(0.65, 0.0, 0.65, 1.0)],
    ));
    // \ll and \gg
    t.push(compile_tpl(
        "≪", "\\ll", "\\ll ", "Molto minore", None,
        vec![poly(&[(0.55, 0.1), (0.05, 0.5), (0.55, 0.9)]), poly(&[(0.95, 0.1), (0.45, 0.5), (0.95, 0.9)])],
    ));
    t.push(compile_tpl(
        "≫", "\\gg", "\\gg ", "Molto maggiore", None,
        vec![poly(&[(0.05, 0.1), (0.55, 0.5), (0.05, 0.9)]), poly(&[(0.45, 0.1), (0.95, 0.5), (0.45, 0.9)])],
    ));

    // ---- Operators, Calculus & Blackboard ----
    // \sum
    t.push(compile_tpl(
        "∑", "\\sum", "\\sum_{i=1}^{n} ", "Sommatoria", None,
        vec![poly(&[(0.95, 0.0), (0.05, 0.0), (0.55, 0.5), (0.05, 1.0), (0.95, 1.0)])],
    ));
    // \prod
    t.push(compile_tpl(
        "∏", "\\prod", "\\prod_{i=1}^{n} ", "Produttoria", None,
        vec![line(0.0, 0.05, 1.0, 0.05), line(0.22, 0.05, 0.22, 1.0), line(0.78, 0.05, 0.78, 1.0)],
    ));
    // \int
    t.push(compile_tpl(
        "∫", "\\int", "\\int_{a}^{b} ", "Integrale", None,
        vec![poly(&[
            (0.72, 0.06), (0.60, 0.0), (0.50, 0.08), (0.48, 0.50), (0.46, 0.92), (0.36, 1.0), (0.24, 0.94),
        ])],
    ));
    // \oint
    t.push(compile_tpl(
        "∮", "\\oint", "\\oint_{C} ", "Integrale di linea chiuso", None,
        vec![
            poly(&[(0.72, 0.06), (0.60, 0.0), (0.50, 0.08), (0.48, 0.50), (0.46, 0.92), (0.36, 1.0), (0.24, 0.94)]),
            arc(0.48, 0.5, 0.24, 0.22, 0.0, 360.0, 18),
        ],
    ));
    // \infty (figure 8 horizontal)
    t.push(compile_tpl(
        "∞", "\\infty", "\\infty", "Infinito", None,
        vec![(0..=32)
            .map(|i| {
                let a = (i as f32 / 32.0) * 2.0 * PI;
                let denom = 1.0 + a.sin().powi(2);
                (0.5 + 0.48 * a.cos() / denom, 0.5 + 0.34 * a.sin() * a.cos() / denom)
            })
            .collect()],
    ));
    // \partial
    t.push(compile_tpl(
        "∂", "\\partial", "\\partial ", "Derivata parziale", None,
        vec![{
            let mut v = arc(0.5, 0.42, 0.36, 0.4, 210.0, 420.0, 14);
            v.extend(arc(0.46, 0.7, 0.32, 0.28, 30.0, 360.0, 16));
            v
        }],
    ));
    // \nabla
    t.push(compile_tpl(
        "∇", "\\nabla", "\\nabla ", "Gradiente / operatore nabla", None,
        vec![poly(&[(0.0, 0.05), (1.0, 0.05), (0.5, 0.95), (0.0, 0.05)])],
    ));
    // \sqrt
    t.push(compile_tpl(
        "√", "\\sqrt{}", "\\sqrt{}", "Radice quadrata", None,
        vec![poly(&[(0.0, 0.6), (0.22, 0.95), (0.45, 0.05), (1.0, 0.05)])],
    ));
    // \pm
    t.push(compile_tpl(
        "±", "\\pm", "\\pm ", "Più o meno", None,
        vec![line(0.5, 0.05, 0.5, 0.68), line(0.15, 0.36, 0.85, 0.36), line(0.15, 0.92, 0.85, 0.92)],
    ));
    // \times
    t.push(compile_tpl(
        "×", "\\times", "\\times ", "Prodotto cartesiano / per", None,
        vec![line(0.1, 0.1, 0.9, 0.9), line(0.9, 0.1, 0.1, 0.9)],
    ));
    // \div
    t.push(compile_tpl(
        "÷", "\\div", "\\div ", "Divisione", None,
        vec![line(0.05, 0.5, 0.95, 0.5), line(0.47, 0.15, 0.53, 0.21), line(0.47, 0.79, 0.53, 0.85)],
    ));
    // \cdot (dot)
    t.push(compile_tpl(
        "·", "\\cdot", "\\cdot ", "Prodotto scalare / punto centrale", None,
        vec![vec![(0.5, 0.5)]],
    ));
    // \circ
    t.push(compile_tpl(
        "∘", "\\circ", "\\circ ", "Composizione di funzioni", None,
        vec![arc(0.5, 0.5, 0.42, 0.42, 0.0, 360.0, 22)],
    ));
    // \oplus
    t.push(compile_tpl(
        "⊕", "\\oplus", "\\oplus ", "Somma diretta / XOR", None,
        vec![
            arc(0.5, 0.5, 0.45, 0.45, 0.0, 360.0, 22),
            line(0.5, 0.05, 0.5, 0.95),
            line(0.05, 0.5, 0.95, 0.5),
        ],
    ));
    // \otimes
    t.push(compile_tpl(
        "⊗", "\\otimes", "\\otimes ", "Prodotto tensoriale", None,
        vec![
            arc(0.5, 0.5, 0.45, 0.45, 0.0, 360.0, 22),
            line(0.18, 0.18, 0.82, 0.82),
            line(0.82, 0.18, 0.18, 0.82),
        ],
    ));
    // \mathbb{R}, \mathbb{N}, \mathbb{Z}
    t.push(compile_tpl(
        "ℝ", "\\mathbb{R}", "\\mathbb{R}", "Insieme dei numeri reali", Some("amssymb"),
        vec![
            line(0.15, 0.0, 0.15, 1.0),
            line(0.28, 0.0, 0.28, 1.0),
            poly(&[(0.15, 0.0), (0.72, 0.0), (0.85, 0.25), (0.72, 0.5), (0.28, 0.5), (0.88, 1.0)]),
        ],
    ));
    t.push(compile_tpl(
        "ℕ", "\\mathbb{N}", "\\mathbb{N}", "Insieme dei numeri naturali", Some("amssymb"),
        vec![
            line(0.1, 1.0, 0.1, 0.0),
            line(0.1, 0.0, 0.85, 1.0),
            line(0.22, 0.0, 0.9, 0.95),
            line(0.9, 1.0, 0.9, 0.0),
        ],
    ));
    t.push(compile_tpl(
        "ℤ", "\\mathbb{Z}", "\\mathbb{Z}", "Insieme dei numeri interi", Some("amssymb"),
        vec![
            poly(&[(0.1, 0.05), (0.9, 0.05), (0.1, 0.95), (0.9, 0.95)]),
            line(0.75, 0.05, 0.25, 0.95),
        ],
    ));
    // \llbracket \rrbracket
    t.push(compile_tpl(
        "⟦⟧", "\\llbracket \\rrbracket", "\\llbracket  \\rrbracket", "Parentesi semantiche", Some("stmaryrd"),
        vec![
            poly(&[(0.4, 0.0), (0.1, 0.0), (0.1, 1.0), (0.4, 1.0)]),
            line(0.25, 0.0, 0.25, 1.0),
        ],
    ));
    // \rightsquigarrow
    t.push(compile_tpl(
        "⇝", "\\rightsquigarrow", "\\rightsquigarrow ", "Freccia ondulata (leads to)", Some("amssymb"),
        vec![
            poly(&[(0.0, 0.5), (0.18, 0.35), (0.36, 0.65), (0.54, 0.35), (0.72, 0.65), (0.95, 0.5)]),
            poly(&[(0.72, 0.28), (1.0, 0.5), (0.72, 0.72)]),
        ],
    ));

    // ---- Greek Letters ----
    // \alpha (fish/loop shape, smooth)
    t.push(compile_tpl(
        "α", "\\alpha", "\\alpha", "Lettera greca alfa", None,
        vec![smooth(&[
            (0.96, 0.12), (0.80, 0.55), (0.60, 0.85), (0.35, 0.92), (0.12, 0.75),
            (0.10, 0.40), (0.30, 0.15), (0.55, 0.28), (0.75, 0.62), (0.86, 0.86), (0.98, 0.92),
        ], 2)],
    ));
    // \beta
    t.push(compile_tpl(
        "β", "\\beta", "\\beta", "Lettera greca beta", None,
        vec![poly(&[
            (0.2, 1.0), (0.25, 0.45), (0.35, 0.08), (0.65, 0.02), (0.82, 0.2),
            (0.65, 0.42), (0.42, 0.46), (0.78, 0.55), (0.85, 0.78), (0.58, 0.92), (0.25, 0.85),
        ])],
    ));
    // \gamma
    t.push(compile_tpl(
        "γ", "\\gamma", "\\gamma", "Lettera greca gamma", None,
        vec![poly(&[
            (0.08, 0.2), (0.28, 0.15), (0.48, 0.55), (0.52, 0.92), (0.42, 0.98),
            (0.38, 0.72), (0.58, 0.42), (0.88, 0.1),
        ])],
    ));
    // \delta
    t.push(compile_tpl(
        "δ", "\\delta", "\\delta", "Lettera greca delta", None,
        vec![poly(&[
            (0.75, 0.08), (0.42, 0.02), (0.25, 0.18), (0.55, 0.42), (0.82, 0.65),
            (0.72, 0.92), (0.38, 0.96), (0.18, 0.75), (0.32, 0.50), (0.62, 0.48),
        ])],
    ));
    // \varepsilon
    t.push(compile_tpl(
        "ε", "\\varepsilon", "\\varepsilon", "Lettera greca epsilon", None,
        vec![poly(&[
            (0.85, 0.18), (0.55, 0.05), (0.22, 0.22), (0.35, 0.48), (0.72, 0.5),
            (0.35, 0.52), (0.15, 0.75), (0.45, 0.95), (0.85, 0.85),
        ])],
    ));
    // \theta
    t.push(compile_tpl(
        "θ", "\\theta", "\\theta", "Lettera greca theta", None,
        vec![arc(0.5, 0.5, 0.34, 0.48, 0.0, 360.0, 22), line(0.16, 0.5, 0.84, 0.5)],
    ));
    // \lambda
    t.push(compile_tpl(
        "λ", "\\lambda", "\\lambda", "Lettera greca lambda", None,
        vec![line(0.18, 0.05, 0.85, 0.98), line(0.52, 0.50, 0.12, 0.98)],
    ));
    // \mu
    t.push(compile_tpl(
        "μ", "\\mu", "\\mu", "Lettera greca mu", None,
        vec![poly(&[
            (0.15, 1.0), (0.22, 0.15), (0.22, 0.68), (0.48, 0.80), (0.75, 0.62), (0.80, 0.18), (0.82, 0.78), (0.95, 0.80),
        ])],
    ));
    // \pi
    t.push(compile_tpl(
        "π", "\\pi", "\\pi", "Lettera greca pi greco", None,
        vec![line(0.05, 0.22, 0.95, 0.22), line(0.32, 0.22, 0.28, 0.92), line(0.70, 0.22, 0.72, 0.92)],
    ));
    // \rho
    t.push(compile_tpl(
        "ρ", "\\rho", "\\rho", "Lettera greca rho", None,
        vec![{
            let mut v = vec![(0.22, 1.0), (0.25, 0.45)];
            v.extend(arc(0.52, 0.36, 0.30, 0.30, 180.0, 540.0, 18));
            v
        }],
    ));
    // \sigma
    t.push(compile_tpl(
        "σ", "\\sigma", "\\sigma", "Lettera greca sigma", None,
        vec![{
            let mut v = vec![(0.95, 0.22), (0.48, 0.22)];
            v.extend(arc(0.45, 0.58, 0.34, 0.34, 270.0, 630.0, 18));
            v
        }],
    ));
    // \tau
    t.push(compile_tpl(
        "τ", "\\tau", "\\tau", "Lettera greca tau", None,
        vec![line(0.1, 0.22, 0.9, 0.22), poly(&[(0.52, 0.22), (0.48, 0.88), (0.68, 0.92)])],
    ));
    // \varphi / \phi
    t.push(compile_tpl(
        "φ", "\\varphi", "\\varphi", "Lettera greca phi", None,
        vec![arc(0.5, 0.48, 0.38, 0.32, 0.0, 360.0, 20), line(0.56, 0.0, 0.44, 1.0)],
    ));
    // \psi
    t.push(compile_tpl(
        "ψ", "\\psi", "\\psi", "Lettera greca psi", None,
        vec![
            poly(&[(0.12, 0.18), (0.22, 0.62), (0.50, 0.72), (0.78, 0.62), (0.88, 0.18)]),
            line(0.50, 0.02, 0.50, 1.0),
        ],
    ));
    // \omega
    t.push(compile_tpl(
        "ω", "\\omega", "\\omega", "Lettera greca omega", None,
        vec![poly(&[
            (0.12, 0.25), (0.08, 0.72), (0.28, 0.88), (0.50, 0.58),
            (0.50, 0.42), (0.50, 0.65), (0.72, 0.88), (0.92, 0.72), (0.88, 0.25),
        ])],
    ));
    // \Delta
    t.push(compile_tpl(
        "Δ", "\\Delta", "\\Delta", "Delta maiuscola", None,
        vec![poly(&[(0.5, 0.05), (0.05, 0.95), (0.95, 0.95), (0.5, 0.05)])],
    ));
    // \Gamma
    t.push(compile_tpl(
        "Γ", "\\Gamma", "\\Gamma", "Gamma maiuscola", None,
        vec![poly(&[(0.2, 0.98), (0.2, 0.05), (0.88, 0.05)])],
    ));
    // \Sigma
    t.push(compile_tpl(
        "Σ", "\\Sigma", "\\Sigma", "Sigma maiuscola", None,
        vec![poly(&[(0.9, 0.05), (0.1, 0.05), (0.52, 0.5), (0.1, 0.95), (0.9, 0.95)])],
    ));
    // \Omega
    t.push(compile_tpl(
        "Ω", "\\Omega", "\\Omega", "Omega maiuscola", None,
        vec![{
            let mut v = vec![(0.08, 0.92), (0.34, 0.92)];
            v.extend(arc(0.5, 0.45, 0.40, 0.40, 130.0, 410.0, 20));
            v.push((0.66, 0.92));
            v.push((0.92, 0.92));
            v
        }],
    ));
    // \Phi
    t.push(compile_tpl(
        "Φ", "\\Phi", "\\Phi", "Phi maiuscola", None,
        vec![arc(0.5, 0.5, 0.42, 0.30, 0.0, 360.0, 20), line(0.5, 0.0, 0.5, 1.0)],
    ));

    add_extended_templates(&mut t);
    t
}

fn add_extended_templates(t: &mut Vec<CompiledTemplate>) {
    // ---- Proportional & relations ----
    t.push(compile_tpl(
        "∝", "\\propto", "\\propto ", "Proporzionale a", None,
        vec![smooth(&[
            (0.96, 0.22), (0.70, 0.30), (0.50, 0.50), (0.30, 0.72), (0.12, 0.70),
            (0.04, 0.50), (0.12, 0.30), (0.30, 0.28), (0.50, 0.50), (0.70, 0.70), (0.96, 0.78),
        ], 2)],
    ));
    t.push(compile_tpl(
        "∝", "\\propto", "\\propto ", "Proporzionale a", None,
        vec![smooth(&[
            (0.98, 0.05), (0.75, 0.22), (0.50, 0.50), (0.30, 0.70), (0.10, 0.70),
            (0.02, 0.50), (0.10, 0.32), (0.30, 0.32), (0.50, 0.50), (0.75, 0.80), (0.98, 0.95),
        ], 2)],
    ));
    t.push(compile_tpl(
        "<", "<", "< ", "Minore", None,
        vec![poly(&[(0.9, 0.1), (0.1, 0.5), (0.9, 0.9)])],
    ));
    t.push(compile_tpl(
        ">", ">", "> ", "Maggiore", None,
        vec![poly(&[(0.1, 0.1), (0.9, 0.5), (0.1, 0.9)])],
    ));
    t.push(compile_tpl(
        "=", "=", "= ", "Uguale", None,
        vec![line(0.0, 0.35, 1.0, 0.35), line(0.0, 0.65, 1.0, 0.65)],
    ));
    t.push(compile_tpl(
        "≃", "\\simeq", "\\simeq ", "Asintoticamente uguale / omotopo", None,
        vec![
            smooth(&[(0.0, 0.40), (0.25, 0.18), (0.5, 0.35), (0.75, 0.52), (1.0, 0.30)], 2),
            line(0.0, 0.80, 1.0, 0.80),
        ],
    ));
    t.push(compile_tpl(
        "≺", "\\prec", "\\prec ", "Precede", None,
        vec![smooth(&[(0.95, 0.05), (0.55, 0.40), (0.05, 0.5), (0.55, 0.60), (0.95, 0.95)], 1)],
    ));
    t.push(compile_tpl(
        "≻", "\\succ", "\\succ ", "Segue", None,
        vec![smooth(&[(0.05, 0.05), (0.45, 0.40), (0.95, 0.5), (0.45, 0.60), (0.05, 0.95)], 1)],
    ));
    t.push(compile_tpl(
        "∋", "\\ni", "\\ni ", "Contiene come elemento", None,
        vec![arc(0.45, 0.5, 0.45, 0.48, -105.0, 105.0, 18), line(0.05, 0.5, 0.9, 0.5)],
    ));
    t.push(compile_tpl(
        "∣", "\\mid", "\\mid ", "Divide / tale che", None,
        vec![line(0.5, 0.0, 0.5, 1.0)],
    ));
    t.push(compile_tpl(
        "∓", "\\mp", "\\mp ", "Meno o più", None,
        vec![line(0.15, 0.08, 0.85, 0.08), line(0.5, 0.32, 0.5, 0.95), line(0.15, 0.64, 0.85, 0.64)],
    ));
    t.push(compile_tpl(
        "∠", "\\angle", "\\angle ", "Angolo", None,
        vec![poly(&[(0.95, 0.05), (0.05, 0.95), (0.95, 0.95)])],
    ));
    t.push(compile_tpl(
        "△", "\\triangle", "\\triangle ", "Triangolo", None,
        vec![poly(&[(0.5, 0.1), (0.05, 0.9), (0.95, 0.9), (0.5, 0.1)])],
    ));
    t.push(compile_tpl(
        "✓", "\\checkmark", "\\checkmark ", "Spunta", Some("amssymb"),
        vec![poly(&[(0.05, 0.55), (0.35, 0.92), (0.95, 0.05)])],
    ));
    t.push(compile_tpl(
        "†", "\\dagger", "\\dagger ", "Croce / aggiunto (dagger)", None,
        vec![line(0.5, 0.0, 0.5, 1.0), line(0.15, 0.28, 0.85, 0.28)],
    ));

    // ---- Arrows ----
    t.push(compile_tpl(
        "↑", "\\uparrow", "\\uparrow ", "Freccia in alto", None,
        vec![line(0.5, 1.0, 0.5, 0.0), poly(&[(0.2, 0.32), (0.5, 0.0), (0.8, 0.32)])],
    ));
    t.push(compile_tpl(
        "↓", "\\downarrow", "\\downarrow ", "Freccia in basso", None,
        vec![line(0.5, 0.0, 0.5, 1.0), poly(&[(0.2, 0.68), (0.5, 1.0), (0.8, 0.68)])],
    ));
    t.push(compile_tpl(
        "⇐", "\\Leftarrow", "\\Leftarrow ", "Implicazione inversa", None,
        vec![
            line(0.18, 0.35, 1.0, 0.35),
            line(0.18, 0.65, 1.0, 0.65),
            poly(&[(0.38, 0.15), (0.0, 0.5), (0.38, 0.85)]),
        ],
    ));
    // single-stroke right arrow (line + head drawn without lifting)
    t.push(compile_tpl(
        "→", "\\to", "\\to ", "Freccia a destra / implica", None,
        vec![poly(&[(0.0, 0.5), (1.0, 0.5), (0.68, 0.22)]), line(1.0, 0.5, 0.68, 0.78)],
    ));
    t.push(compile_tpl(
        "↪", "\\hookrightarrow", "\\hookrightarrow ", "Inclusione (hook arrow)", None,
        vec![
            {
                let mut v = arc(0.14, 0.38, 0.12, 0.12, 270.0, 90.0, 10);
                v.push((1.0, 0.5));
                v
            },
            poly(&[(0.72, 0.25), (1.0, 0.5), (0.72, 0.75)]),
        ],
    ));

    // ---- Stars, dots ----
    t.push(compile_tpl(
        "⋆", "\\star", "\\star ", "Stella", None,
        vec![(0..=5)
            .map(|k| {
                let a = (-90.0 + (k as f32) * 144.0) * PI / 180.0;
                (0.5 + 0.48 * a.cos(), 0.52 + 0.48 * a.sin())
            })
            .collect()],
    ));
    t.push(compile_tpl(
        "∗", "\\ast", "\\ast ", "Asterisco", None,
        vec![line(0.5, 0.0, 0.5, 1.0), line(0.07, 0.25, 0.93, 0.75), line(0.93, 0.25, 0.07, 0.75)],
    ));
    t.push(compile_tpl(
        "∴", "\\therefore", "\\therefore ", "Quindi", Some("amssymb"),
        vec![vec![(0.5, 0.1)], vec![(0.1, 0.9)], vec![(0.9, 0.9)]],
    ));
    t.push(compile_tpl(
        "∵", "\\because", "\\because ", "Poiché", Some("amssymb"),
        vec![vec![(0.1, 0.1)], vec![(0.9, 0.1)], vec![(0.5, 0.9)]],
    ));
    t.push(compile_tpl(
        "…", "\\ldots", "\\ldots", "Puntini orizzontali", None,
        vec![vec![(0.05, 0.5)], vec![(0.5, 0.5)], vec![(0.95, 0.5)]],
    ));
    t.push(compile_tpl(
        "⋮", "\\vdots", "\\vdots", "Puntini verticali", None,
        vec![vec![(0.5, 0.05)], vec![(0.5, 0.5)], vec![(0.5, 0.95)]],
    ));
    t.push(compile_tpl(
        "⋱", "\\ddots", "\\ddots", "Puntini diagonali", None,
        vec![vec![(0.05, 0.05)], vec![(0.5, 0.5)], vec![(0.95, 0.95)]],
    ));

    // ---- Delimiters ----
    t.push(compile_tpl(
        "⟨", "\\langle", "\\langle ", "Parentesi angolare sinistra", None,
        vec![poly(&[(0.7, 0.0), (0.3, 0.5), (0.7, 1.0)])],
    ));
    t.push(compile_tpl(
        "⟩", "\\rangle", "\\rangle ", "Parentesi angolare destra", None,
        vec![poly(&[(0.3, 0.0), (0.7, 0.5), (0.3, 1.0)])],
    ));
    t.push(compile_tpl(
        "⌊", "\\lfloor", "\\lfloor ", "Parte intera inferiore (sx)", None,
        vec![poly(&[(0.3, 0.0), (0.3, 1.0), (0.8, 1.0)])],
    ));
    t.push(compile_tpl(
        "⌋", "\\rfloor", "\\rfloor ", "Parte intera inferiore (dx)", None,
        vec![poly(&[(0.7, 0.0), (0.7, 1.0), (0.2, 1.0)])],
    ));
    t.push(compile_tpl(
        "⌈", "\\lceil", "\\lceil ", "Parte intera superiore (sx)", None,
        vec![poly(&[(0.8, 0.0), (0.3, 0.0), (0.3, 1.0)])],
    ));
    t.push(compile_tpl(
        "⌉", "\\rceil", "\\rceil ", "Parte intera superiore (dx)", None,
        vec![poly(&[(0.2, 0.0), (0.7, 0.0), (0.7, 1.0)])],
    ));

    // ---- Calculus / sets extras ----
    t.push(compile_tpl(
        "∬", "\\iint", "\\iint ", "Integrale doppio", Some("amsmath"),
        vec![
            smooth(&[(0.52, 0.06), (0.42, 0.0), (0.34, 0.08), (0.32, 0.50), (0.30, 0.92), (0.22, 1.0), (0.12, 0.94)], 1),
            smooth(&[(0.90, 0.06), (0.80, 0.0), (0.72, 0.08), (0.70, 0.50), (0.68, 0.92), (0.60, 1.0), (0.50, 0.94)], 1),
        ],
    ));
    t.push(compile_tpl(
        "∄", "\\nexists", "\\nexists ", "Non esiste", Some("amssymb"),
        vec![
            poly(&[(0.1, 0.05), (0.9, 0.05), (0.9, 0.95), (0.1, 0.95)]),
            line(0.15, 0.5, 0.9, 0.5),
            line(0.8, 0.0, 0.2, 1.0),
        ],
    ));
    t.push(compile_tpl(
        "∅", "\\varnothing", "\\varnothing", "Insieme vuoto (variante)", Some("amssymb"),
        vec![arc(0.5, 0.5, 0.40, 0.40, 0.0, 360.0, 22), line(0.95, 0.0, 0.05, 1.0)],
    ));
    t.push(compile_tpl(
        "ℚ", "\\mathbb{Q}", "\\mathbb{Q}", "Insieme dei numeri razionali", Some("amssymb"),
        vec![
            arc(0.5, 0.46, 0.42, 0.44, 0.0, 360.0, 22),
            line(0.24, 0.12, 0.24, 0.80),
            line(0.55, 0.78, 0.92, 1.0),
        ],
    ));
    t.push(compile_tpl(
        "ℂ", "\\mathbb{C}", "\\mathbb{C}", "Insieme dei numeri complessi", Some("amssymb"),
        vec![arc(0.55, 0.5, 0.45, 0.48, 300.0, 60.0, 18), line(0.24, 0.12, 0.24, 0.88)],
    ));
    t.push(compile_tpl(
        "ℓ", "\\ell", "\\ell", "Elle corsiva", None,
        vec![smooth(&[
            (0.20, 0.85), (0.50, 0.58), (0.72, 0.22), (0.66, 0.02), (0.50, 0.08),
            (0.40, 0.50), (0.44, 0.92), (0.78, 0.96),
        ], 2)],
    ));
    t.push(compile_tpl(
        "ℏ", "\\hbar", "\\hbar", "Costante di Planck ridotta", None,
        vec![
            line(0.25, 0.0, 0.25, 1.0),
            smooth(&[(0.25, 0.62), (0.50, 0.45), (0.74, 0.55), (0.76, 1.0)], 2),
            line(0.08, 0.28, 0.58, 0.16),
        ],
    ));
    t.push(compile_tpl(
        "ℵ", "\\aleph", "\\aleph", "Aleph (cardinalità)", None,
        vec![
            line(0.15, 0.08, 0.85, 0.92),
            smooth(&[(0.80, 0.05), (0.80, 0.38), (0.58, 0.48)], 1),
            smooth(&[(0.40, 0.52), (0.22, 0.65), (0.22, 0.95)], 1),
        ],
    ));

    // ---- More Greek ----
    t.push(compile_tpl(
        "ν", "\\nu", "\\nu", "Lettera greca nu", None,
        vec![smooth(&[(0.08, 0.12), (0.22, 0.12), (0.45, 0.92), (0.80, 0.45), (0.92, 0.08)], 1)],
    ));
    t.push(compile_tpl(
        "η", "\\eta", "\\eta", "Lettera greca eta", None,
        vec![smooth(&[
            (0.08, 0.25), (0.20, 0.18), (0.25, 0.70), (0.25, 0.35), (0.50, 0.18),
            (0.75, 0.30), (0.78, 1.0),
        ], 1)],
    ));
    t.push(compile_tpl(
        "ζ", "\\zeta", "\\zeta", "Lettera greca zeta", None,
        vec![smooth(&[
            (0.30, 0.0), (0.78, 0.05), (0.35, 0.35), (0.18, 0.62), (0.35, 0.82),
            (0.66, 0.86), (0.60, 1.0),
        ], 2)],
    ));
    t.push(compile_tpl(
        "κ", "\\kappa", "\\kappa", "Lettera greca kappa", None,
        vec![line(0.2, 0.1, 0.2, 0.9), poly(&[(0.8, 0.1), (0.25, 0.5), (0.85, 0.9)])],
    ));
    t.push(compile_tpl(
        "χ", "\\chi", "\\chi", "Lettera greca chi", None,
        vec![
            smooth(&[(0.05, 0.12), (0.22, 0.10), (0.78, 0.90), (0.95, 0.88)], 1),
            line(0.85, 0.10, 0.15, 0.90),
        ],
    ));
    t.push(compile_tpl(
        "Λ", "\\Lambda", "\\Lambda", "Lambda maiuscola", None,
        vec![poly(&[(0.18, 1.0), (0.5, 0.0), (0.82, 1.0)])],
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noisy_redraws_of_every_template_rank_in_top_3() {
        // Deterministic LCG noise
        let mut seed: u32 = 12345;
        let mut rnd = move || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5
        };

        let mut total = 0;
        let mut top1 = 0;
        let mut top3 = 0;
        let mut misses = Vec::new();
        for tpl in compiled_templates() {
            if tpl.feat.is_dot {
                continue;
            }
            // Redraw each template from its resampled points with scale/aspect/jitter noise,
            // optionally reversed, split into the template's stroke count.
            for trial in 0..3 {
                let sx = 120.0 * (1.0 + 0.15 * rnd());
                let sy = 120.0 * (1.0 + 0.15 * rnd());
                let reverse = trial == 1;
                let mut pts: Vec<Point> = tpl
                    .feat
                    .cloud
                    .iter()
                    .map(|p| Point::new(100.0 + p.x * sx + 3.0 * rnd(), 100.0 + p.y * sy + 3.0 * rnd()))
                    .collect();
                if reverse {
                    pts.reverse();
                }
                // Split into the template's stroke count by contiguous chunks
                let n = tpl.feat.stroke_count.max(1);
                let chunk = pts.len().div_ceil(n);
                let strokes: Vec<Vec<Point>> = pts.chunks(chunk).map(|c| c.to_vec()).collect();
                let res = recognize(&strokes, 3);
                total += 1;
                if res.first().map(|c| c.display_cmd) == Some(tpl.display_cmd) {
                    top1 += 1;
                }
                if res.iter().any(|c| c.display_cmd == tpl.display_cmd) {
                    top3 += 1;
                } else {
                    misses.push((tpl.display_cmd, res.iter().map(|c| c.display_cmd).collect::<Vec<_>>()));
                }
            }
        }
        let acc3 = top3 as f32 / total as f32;
        println!("top1 {top1}/{total}, top3 {top3}/{total}; misses: {misses:?}");
        assert!(acc3 >= 0.93, "top-3 accuracy too low: {acc3}; misses: {misses:?}");
    }

    fn pt_stroke(pts: &[(f32, f32)]) -> Vec<Point> {
        pts.iter()
            .map(|&(x, y)| Point::new(40.0 + x * 140.0, 30.0 + y * 140.0))
            .collect()
    }

    #[test]
    fn recognizes_common_hand_drawn_symbols() {
        // Draw \forall (inverted A)
        let forall_strokes = vec![
            pt_stroke(&[(0.05, 0.02), (0.48, 0.98), (0.96, 0.05)]),
            pt_stroke(&[(0.20, 0.48), (0.80, 0.46)]),
        ];
        let res = recognize(&forall_strokes, 5);
        assert!(
            res.iter().take(2).any(|c| c.display_cmd == "\\forall"),
            "Expected \\forall in top 2, got: {res:?}"
        );

        // Draw \Delta (triangle pointing up)
        let delta_strokes = vec![pt_stroke(&[(0.5, 0.05), (0.08, 0.92), (0.94, 0.93), (0.5, 0.05)])];
        let res = recognize(&delta_strokes, 5);
        assert!(
            res.iter().take(2).any(|c| c.display_cmd == "\\Delta"),
            "Expected \\Delta in top 2, got: {res:?}"
        );

        // Draw \in (curved C + middle bar)
        let mut c_arc = arc(0.55, 0.5, 0.42, 0.45, 285.0, 75.0, 14);
        c_arc.iter_mut().for_each(|(x, y)| {
            *x = 40.0 + *x * 140.0;
            *y = 30.0 + *y * 140.0;
        });
        let in_strokes = vec![
            c_arc.into_iter().map(|(x, y)| Point::new(x, y)).collect(),
            pt_stroke(&[(0.12, 0.52), (0.92, 0.50)]),
        ];
        let res = recognize(&in_strokes, 5);
        assert!(
            res.iter().take(2).any(|c| c.display_cmd == "\\in"),
            "Expected \\in in top 2, got: {res:?}"
        );

        // Draw \infty (horizontal figure 8)
        let inf_pts: Vec<Point> = (0..=30)
            .map(|i| {
                let a = (i as f32 / 30.0) * 2.0 * PI;
                let d = 1.0 + a.sin().powi(2);
                Point::new(
                    120.0 + 70.0 * a.cos() / d,
                    110.0 + 48.0 * a.sin() * a.cos() / d,
                )
            })
            .collect();
        let res = recognize(&[inf_pts], 5);
        assert!(
            res.iter().take(2).any(|c| c.display_cmd == "\\infty"),
            "Expected \\infty in top 2, got: {res:?}"
        );

        // Draw \le (< + underline)
        let le_strokes = vec![
            pt_stroke(&[(0.88, 0.08), (0.12, 0.44), (0.88, 0.74)]),
            pt_stroke(&[(0.12, 0.94), (0.88, 0.94)]),
        ];
        let res = recognize(&le_strokes, 5);
        assert!(
            res.iter().take(2).any(|c| c.display_cmd == "\\le"),
            "Expected \\le in top 2, got: {res:?}"
        );

        // Draw \propto like a real user: start top-right, loop on the left, end bottom-right
        // (wide aspect, slightly sloppy).
        let propto: Vec<Point> = [
            (1.0, 0.0), (0.80, 0.18), (0.62, 0.38), (0.48, 0.52), (0.30, 0.68), (0.14, 0.70),
            (0.02, 0.58), (0.04, 0.38), (0.18, 0.30), (0.36, 0.36), (0.52, 0.50), (0.70, 0.66),
            (0.86, 0.80), (1.02, 0.84),
        ]
        .iter()
        .map(|&(x, y)| Point::new(90.0 + x * 145.0, 160.0 + y * 90.0))
        .collect();
        let res = recognize(&[propto], 5);
        assert!(
            res.iter().take(2).any(|c| c.display_cmd == "\\propto"),
            "Expected \\propto in top 2, got: {res:?}"
        );
        assert_ne!(res[0].display_cmd, "\\infty", "∝ must not be classified as ∞");

        // Three taps in a row -> \ldots
        let dots = vec![
            vec![Point::new(40.0, 100.0)],
            vec![Point::new(90.0, 101.0)],
            vec![Point::new(140.0, 99.0)],
        ];
        let res = recognize(&dots, 5);
        assert!(
            res.iter().take(2).any(|c| c.display_cmd == "\\ldots"),
            "Expected \\ldots in top 2, got: {res:?}"
        );
    }
}

