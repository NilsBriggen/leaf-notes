//! Offline handwriting recognition: a small CNN (trained by tools/handwriting/train.py on EMNIST,
//! HASYv2 and synthetic marks) reads single characters; geometry splits ink into lines, words and
//! characters, and lays out math (fractions, scripts, roots).
//! shortcut: reads printed (non-cursive) handwriting one character at a time; joined-up cursive
//! needs a sequence model — upgrade to one if people mostly write cursive.

use crate::model::Stroke;
use std::sync::OnceLock;

static MODEL_BYTES: &[u8] = include_bytes!("../assets/hw-model.bin");
/// A second, independently trained model; the two vote, which smooths out each one's quirks.
static MODEL_B_BYTES: &[u8] = include_bytes!("../assets/hw-model-a.bin");

/// A small CNN: 3×3 conv layers (ReLU), 2×2 max-pools after some of them, then two dense layers.
/// Format v1: 4 convs, pools after conv 2 and 4. Format v2 (batch norm folded in): 5 convs, pools after 2, 4 and 5.
struct Model { labels: Vec<String>, convs: Vec<(usize, usize, Vec<f32>, Vec<f32>)>, pool_after: Vec<usize>, dense: Vec<(Vec<f32>, Vec<f32>)> }

fn parse_model(b: &[u8]) -> Model {
    let v2 = &b[..5] == b"LNHW2";
    assert!(v2 || &b[..5] == b"LNHW1", "handwriting model file is damaged");
    let n = u32::from_le_bytes(b[5..9].try_into().unwrap()) as usize;
    let ll = u32::from_le_bytes(b[9..13].try_into().unwrap()) as usize;
    let labels: Vec<String> = std::str::from_utf8(&b[13..13 + ll]).unwrap().split('\n').map(String::from).collect();
    assert_eq!(labels.len(), n);
    let chans: &[(usize, usize)] = if v2 { &[(1, 32), (32, 32), (32, 64), (64, 64), (64, 128)] } else { &[(1, 32), (32, 32), (32, 64), (64, 64)] };
    let (flat, hidden) = if v2 { (128 * 3 * 3, 256) } else { (64 * 7 * 7, 128) };
    let mut off = 13 + ll;
    let mut take = |len: usize| -> Vec<f32> {
        let v = b[off..off + len * 4].chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect();
        off += len * 4;
        v
    };
    let convs = chans.iter().map(|&(ci, co)| { let w = take(co * ci * 9); let bias = take(co); (ci, co, w, bias) }).collect();
    let dense = vec![(take(hidden * flat), take(hidden)), (take(n * hidden), take(n))];
    Model { labels, convs, pool_after: if v2 { vec![1, 3, 4] } else { vec![1, 3] }, dense }
}

fn model() -> &'static Model {
    static M: OnceLock<Model> = OnceLock::new();
    M.get_or_init(|| parse_model(MODEL_BYTES))
}

fn model_b() -> &'static Model {
    static M: OnceLock<Model> = OnceLock::new();
    M.get_or_init(|| { let m = parse_model(MODEL_B_BYTES); assert_eq!(m.labels, model().labels, "both handwriting models must read the same symbols"); m })
}

fn conv3x3_relu(x: &[f32], cin: usize, hw: usize, w: &[f32], b: &[f32], cout: usize) -> Vec<f32> {
    let mut out = vec![0f32; cout * hw * hw];
    for o in 0..cout {
        let plane = &mut out[o * hw * hw..(o + 1) * hw * hw];
        plane.iter_mut().for_each(|v| *v = b[o]);
        for i in 0..cin {
            let k = &w[(o * cin + i) * 9..(o * cin + i) * 9 + 9];
            let src = &x[i * hw * hw..(i + 1) * hw * hw];
            for ky in 0..3 {
                for kx in 0..3 {
                    let kv = k[ky * 3 + kx];
                    if kv == 0.0 { continue; }
                    for y in 0..hw {
                        let sy = y as isize + ky as isize - 1;
                        if sy < 0 || sy >= hw as isize { continue; }
                        let row = &src[sy as usize * hw..sy as usize * hw + hw];
                        let dst = &mut plane[y * hw..y * hw + hw];
                        let (x0, x1) = (if kx == 0 { 1 } else { 0 }, if kx == 2 { hw - 1 } else { hw });
                        for xx in x0..x1 { dst[xx] += kv * row[xx + kx - 1]; }
                    }
                }
            }
        }
        plane.iter_mut().for_each(|v| *v = v.max(0.0));
    }
    out
}

fn maxpool2(x: &[f32], c: usize, hw: usize) -> Vec<f32> {
    let h2 = hw / 2;
    let mut out = vec![0f32; c * h2 * h2];
    for ch in 0..c { for y in 0..h2 { for xx in 0..h2 {
        let at = |dy: usize, dx: usize| x[ch * hw * hw + (2 * y + dy) * hw + 2 * xx + dx];
        out[ch * h2 * h2 + y * h2 + xx] = at(0, 0).max(at(0, 1)).max(at(1, 0)).max(at(1, 1));
    }}}
    out
}

fn linear(x: &[f32], w: &[f32], b: &[f32], relu: bool) -> Vec<f32> {
    b.iter().enumerate().map(|(o, bo)| {
        let v = bo + w[o * x.len()..(o + 1) * x.len()].iter().zip(x).map(|(a, c)| a * c).sum::<f32>();
        if relu { v.max(0.0) } else { v }
    }).collect()
}

/// Class probabilities for a normalised 28x28 image.
fn probs(img: &[f32]) -> Vec<f32> {
    let (a, b) = (probs_with(model(), img), probs_with(model_b(), img));
    a.iter().zip(&b).map(|(x, y)| (x + y) * 0.5).collect()
}

fn probs_with(m: &Model, img: &[f32]) -> Vec<f32> {
    let (mut x, mut hw) = (img.to_vec(), 28);
    for (k, (ci, co, w, b)) in m.convs.iter().enumerate() {
        x = conv3x3_relu(&x, *ci, hw, w, b, *co);
        if m.pool_after.contains(&k) { x = maxpool2(&x, *co, hw); hw /= 2; }
    }
    let x = linear(&x, &m.dense[0].0, &m.dense[0].1, true);
    let z = linear(&x, &m.dense[1].0, &m.dense[1].1, false);
    let mx = z.iter().cloned().fold(f32::MIN, f32::max);
    let e: Vec<f32> = z.iter().map(|v| (v - mx).exp()).collect();
    let s: f32 = e.iter().sum();
    e.into_iter().map(|v| v / s).collect()
}

/// Draw strokes the way training samples were normalised: 1-px lines fitted into a 20x20 box,
/// centred in 28x28, thickened by a 3x3 dilation, lightly blurred.
fn render(strokes: &[&Stroke]) -> Vec<f32> {
    let pts = strokes.iter().flat_map(|s| s.points.iter());
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for p in pts { x0 = x0.min(p[0]); y0 = y0.min(p[1]); x1 = x1.max(p[0]); y1 = y1.max(p[1]); }
    let s = 20.0 / (x1 - x0).max(y1 - y0).max(1.0);
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let mut grid = [false; 784];
    let mut put = |x: f32, y: f32| {
        let (u, v) = (((x - cx) * s + 13.5).round(), ((y - cy) * s + 13.5).round());
        if (0.0..28.0).contains(&u) && (0.0..28.0).contains(&v) { grid[v as usize * 28 + u as usize] = true; }
    };
    for st in strokes {
        put(st.points[0][0], st.points[0][1]);
        for w in st.points.windows(2) {
            let n = (((w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]) * s) * 4.0).ceil().max(1.0) as usize;
            for k in 1..=n { let t = k as f32 / n as f32; put(w[0][0] + (w[1][0] - w[0][0]) * t, w[0][1] + (w[1][1] - w[0][1]) * t); }
        }
    }
    let mut dil = [0f32; 784];
    for y in 0..28i32 { for x in 0..28i32 {
        let on = (-1..=1).any(|dy| (-1..=1).any(|dx| { let (a, b) = (x + dx, y + dy); (0..28).contains(&a) && (0..28).contains(&b) && grid[(b * 28 + a) as usize] }));
        if on { dil[(y * 28 + x) as usize] = 1.0; }
    }}
    // separable gaussian, sigma 0.6 (matches scipy's gaussian_filter with truncate 4 → radius 2)
    let k: Vec<f32> = (-2..=2).map(|i: i32| (-(i * i) as f32 / (2.0 * 0.36)).exp()).collect();
    let ks: f32 = k.iter().sum();
    let blur = |src: &[f32; 784], horiz: bool| {
        let mut out = [0f32; 784];
        for y in 0..28i32 { for x in 0..28i32 {
            let mut v = 0.0;
            for (j, kv) in k.iter().enumerate() {
                let d = j as i32 - 2;
                let (a, b) = if horiz { ((x + d).clamp(0, 27), y) } else { (x, (y + d).clamp(0, 27)) };
                // scipy's default 'reflect' border ≈ clamping for these mostly-empty edges
                v += kv * src[(b * 28 + a) as usize];
            }
            out[(y * 28 + x) as usize] = v / ks;
        }}
        out
    };
    blur(&blur(&dil, true), false).to_vec()
}

#[derive(Clone, Copy, Debug)]
struct BBox { x0: f32, y0: f32, x1: f32, y1: f32 }

impl BBox {
    fn of(strokes: &[&Stroke]) -> BBox {
        let mut b = BBox { x0: f32::MAX, y0: f32::MAX, x1: f32::MIN, y1: f32::MIN };
        for p in strokes.iter().flat_map(|s| s.points.iter()) { b.x0 = b.x0.min(p[0]); b.y0 = b.y0.min(p[1]); b.x1 = b.x1.max(p[0]); b.y1 = b.y1.max(p[1]); }
        b
    }
    fn w(&self) -> f32 { self.x1 - self.x0 }
    fn h(&self) -> f32 { self.y1 - self.y0 }
    fn cx(&self) -> f32 { (self.x0 + self.x1) / 2.0 }
    fn cy(&self) -> f32 { (self.y0 + self.y1) / 2.0 }
    fn union(&self, o: &BBox) -> BBox { BBox { x0: self.x0.min(o.x0), y0: self.y0.min(o.y0), x1: self.x1.max(o.x1), y1: self.y1.max(o.y1) } }
}

fn median(mut v: Vec<f32>) -> f32 {
    if v.is_empty() { return 1.0; }
    v.sort_by(f32::total_cmp);
    v[v.len() / 2]
}

/// Typical letter height of a set of strokes (strokes of dots and dashes don't count).
fn unit(strokes: &[&Stroke]) -> f32 {
    let hs: Vec<f32> = strokes.iter().map(|s| { let b = BBox::of(&[s]); b.h().max(b.w() * 0.6) }).collect();
    let big = hs.iter().cloned().fold(0.0, f32::max);
    median(hs.into_iter().filter(|&h| h > big * 0.3).collect()).max(4.0)
}

/// Split strokes into horizontal lines of writing.
fn lines<'a>(strokes: &[&'a Stroke], gap: f32, u: f32) -> Vec<Vec<&'a Stroke>> {
    let mut items: Vec<(BBox, &Stroke)> = strokes.iter().map(|s| (BBox::of(&[s]), *s)).collect();
    items.sort_by(|a, b| a.0.cy().total_cmp(&b.0.cy()));
    let mut out: Vec<(f32, f32, Vec<&Stroke>)> = vec![];
    for (b, s) in items {
        // shrink each stroke's band so ascenders/descenders don't glue lines together
        // (flat marks like '-' or '=' still count as a third of a letter tall)
        let half = (b.h() * 0.25).max(u * 0.3);
        let (t, btm) = (b.cy() - half, b.cy() + half);
        match out.last_mut() {
            Some(l) if t <= l.1 + gap => { l.1 = l.1.max(btm); l.2.push(s); }
            _ => out.push((t, btm, vec![s])),
        }
    }
    out.into_iter().map(|l| l.2).collect()
}

/// Group a line's strokes into characters: strokes that touch, or stack above each other
/// (the dot of an i, the bars of =), belong to one character.
fn chars<'a>(strokes: &[&'a Stroke]) -> Vec<(BBox, Vec<&'a Stroke>)> {
    let n = strokes.len();
    let boxes: Vec<BBox> = strokes.iter().map(|s| BBox::of(&[s])).collect();
    let u = unit(strokes);
    let mut parent: Vec<usize> = (0..n).collect();
    fn root(p: &mut Vec<usize>, i: usize) -> usize { let mut r = i; while p[r] != r { r = p[r]; } p[i] = r; r }
    for i in 0..n {
        for j in i + 1..n {
            let (a, b) = (&boxes[i], &boxes[j]);
            let overlap = a.x1.min(b.x1) - a.x0.max(b.x0);
            let stacked = overlap >= 0.5 * a.w().min(b.w()).max(1.0) || (b.cx() >= a.x0 && b.cx() <= a.x1) || (a.cx() >= b.x0 && a.cx() <= b.x1);
            let near = overlap > -u * 0.1 && a.y1.min(b.y1) - a.y0.max(b.y0) > -u * 0.1 && touches(strokes[i], strokes[j], u * 0.08);
            if stacked || near { let (ri, rj) = (root(&mut parent, i), root(&mut parent, j)); parent[ri] = rj; }
        }
    }
    let mut groups: std::collections::BTreeMap<usize, (BBox, Vec<&Stroke>)> = Default::default();
    for i in 0..n {
        let r = root(&mut parent, i);
        let e = groups.entry(r).or_insert((boxes[i], vec![]));
        e.0 = e.0.union(&boxes[i]);
        e.1.push(strokes[i]);
    }
    let mut out: Vec<(BBox, Vec<&Stroke>)> = groups.into_values().collect();
    out.sort_by(|a, b| a.0.x0.total_cmp(&b.0.x0));
    out
}

/// True if two strokes come within `d` of each other anywhere (segments, so crossings count).
fn touches(a: &Stroke, b: &Stroke, d: f32) -> bool {
    let d = d.max(1.5);
    let (ba, bb) = (BBox::of(&[a]), BBox::of(&[b]));
    if ba.x0 - d > bb.x1 || bb.x0 - d > ba.x1 || ba.y0 - d > bb.y1 || bb.y0 - d > ba.y1 { return false; }
    let segs = |s: &Stroke| -> Vec<([f32; 2], [f32; 2])> {
        if s.points.len() == 1 { return vec![([s.points[0][0], s.points[0][1]], [s.points[0][0], s.points[0][1]])]; }
        s.points.windows(2).map(|w| ([w[0][0], w[0][1]], [w[1][0], w[1][1]])).collect()
    };
    let (sa, sb) = (segs(a), segs(b));
    sa.iter().any(|x| sb.iter().any(|y| seg_dist(x.0, x.1, y.0, y.1) <= d))
}

fn seg_dist(p1: [f32; 2], p2: [f32; 2], q1: [f32; 2], q2: [f32; 2]) -> f32 {
    let cross = |o: [f32; 2], a: [f32; 2], b: [f32; 2]| (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
    let (d1, d2, d3, d4) = (cross(q1, q2, p1), cross(q1, q2, p2), cross(p1, p2, q1), cross(p1, p2, q2));
    if (d1 > 0.0) != (d2 > 0.0) && (d3 > 0.0) != (d4 > 0.0) { return 0.0; }
    let pt = |p: [f32; 2], a: [f32; 2], b: [f32; 2]| {
        let (vx, vy) = (b[0] - a[0], b[1] - a[1]);
        let t = (((p[0] - a[0]) * vx + (p[1] - a[1]) * vy) / (vx * vx + vy * vy).max(1e-6)).clamp(0.0, 1.0);
        (p[0] - a[0] - t * vx).hypot(p[1] - a[1] - t * vy)
    };
    pt(p1, q1, q2).min(pt(p2, q1, q2)).min(pt(q1, p1, p2)).min(pt(q2, p1, p2))
}

/// Join neighbouring pieces when the recognizer reads them better as one character than as two.
fn refine<'a>(mut cs: Vec<(BBox, Vec<&'a Stroke>)>, hu: f32) -> Vec<(BBox, Vec<&'a Stroke>)> {
    let conf = |g: &[&Stroke]| best(&probs(&render(g)), |_| true).1;
    let mut i = 0;
    while i + 1 < cs.len() {
        let (a, b) = (&cs[i], &cs[i + 1]);
        let joined = a.0.union(&b.0);
        if b.0.x0 - a.0.x1 < hu * 0.2 && joined.w() < hu * 1.3 && a.0.w().max(b.0.w()) < hu * 0.9 {
            let both: Vec<&Stroke> = a.1.iter().chain(b.1.iter()).copied().collect();
            if conf(&both) > conf(&a.1).min(conf(&b.1)) + 0.15 {
                cs[i] = (joined, both);
                cs.remove(i + 1);
                continue;
            }
        }
        i += 1;
    }
    cs
}

fn is_digit(l: &str) -> bool { l.len() == 1 && l.as_bytes()[0].is_ascii_digit() }
fn is_letter(l: &str) -> bool { l.chars().count() == 1 && l.chars().next().unwrap().is_ascii_alphabetic() }
/// EMNIST merges these upper/lower-case look-alikes; size decides the case.
const MERGED: &str = "CIJKLMOPSUVWXYZ";

/// Best label among those `allowed`, with its probability.
fn best(p: &[f32], allowed: impl Fn(&str) -> bool) -> (String, f32) {
    let m = model();
    p.iter().enumerate().filter(|(i, _)| allowed(&m.labels[*i])).max_by(|a, b| a.1.total_cmp(b.1))
        .map(|(i, v)| (m.labels[i].clone(), *v)).unwrap_or_default()
}

/// Recognise handwriting as plain text (lines, words, characters).
pub fn recognize_text(strokes: &[Stroke]) -> String {
    let strokes: Vec<&Stroke> = strokes.iter().filter(|s| !s.highlighter && !s.points.is_empty()).collect();
    if strokes.is_empty() { return String::new(); }
    let u = unit(&strokes);
    let text_class = |l: &str| l.chars().all(|c| c.is_ascii_alphanumeric() || "()/-+=".contains(c));
    let mut out_lines = vec![];
    for line in lines(&strokes, u * 0.15, u) {
        let cs = chars(&line);
        let hu = median(cs.iter().map(|c| c.0.h()).collect()).max(4.0);
        let cs = refine(cs, hu);
        let tall = cs.iter().map(|c| c.0.h()).fold(0.0, f32::max);
        let baseline = median(cs.iter().filter(|c| c.0.h() > hu * 0.5).map(|c| c.0.y1).collect());
        // words: split where the gap between characters is wide
        // words: a gap clearly wider than this writer's usual letter gap
        let gaps: Vec<f32> = cs.windows(2).map(|w| w[1].0.x0 - w[0].0.x1).collect();
        let word_gap = (hu * 0.5).max((median(gaps.into_iter().filter(|g| *g > 0.0).collect()) * 2.2).min(hu * 1.1));
        let mut words: Vec<Vec<(BBox, Vec<&Stroke>)>> = vec![];
        let mut prev_x1 = f32::MIN;
        for c in cs {
            if c.0.x0 - prev_x1 > word_gap || words.is_empty() { words.push(vec![]); }
            prev_x1 = prev_x1.max(c.0.x1);
            words.last_mut().unwrap().push(c);
        }
        let mut out_words = vec![];
        for word in words {
            let mut ps = vec![];
            for (b, g) in &word {
                let small = b.w().max(b.h()) < hu * 0.25;
                ps.push(if small { None } else { Some(probs(&render(g))) });
            }
            // decide digits vs letters for the whole word, then read each character in that mode
            let (mut dsum, mut lsum) = (0.0, 0.0);
            for p in ps.iter().flatten() { dsum += best(p, is_digit).1; lsum += best(p, is_letter).1; }
            let numeric = dsum > lsum;
            let mut w = String::new();
            for ((b, g), p) in word.iter().zip(&ps) {
                let Some(p) = p else {
                    w.push(if b.y1 > baseline + hu * 0.15 { ',' } else if b.y1 < baseline - hu * 0.5 { '\'' } else { '.' });
                    continue;
                };
                let (any, conf) = best(p, text_class);
                let label = if conf > 0.9 || !(is_digit(&any) || is_letter(&any)) { any } else if numeric { best(p, is_digit).0 } else { best(p, is_letter).0 };
                if MERGED.contains(&label) && !numeric {
                    // single straight stroke reads as l; other merged letters take case from size
                    let lower = b.h() < tall * 0.7 || (label == "I" || label == "L") && g.len() == 1;
                    if label == "I" && g.len() == 1 { w.push(if b.h() < tall * 0.7 { 'i' } else { 'l' }); continue; }
                    w.push_str(&if lower { label.to_lowercase() } else { label });
                } else {
                    w.push_str(&label);
                }
            }
            out_words.push(w);
        }
        out_lines.push(out_words.join(" "));
    }
    out_lines.join("\n")
}

fn latex_of(sym: &str) -> String {
    match sym {
        "∫" => r"\int ", "∑" => r"\sum ", "∏" => r"\prod ", "π" => r"\pi ", "∞" => r"\infty ", "α" => r"\alpha ", "β" => r"\beta ",
        "γ" => r"\gamma ", "δ" => r"\delta ", "θ" => r"\theta ", "λ" => r"\lambda ", "μ" => r"\mu ", "σ" => r"\sigma ", "φ" => r"\varphi ",
        "ω" => r"\omega ", "Δ" => r"\Delta ", "Ω" => r"\Omega ", "ρ" => r"\rho ", "τ" => r"\tau ", "ε" => r"\varepsilon ", "×" => r"\times ",
        "±" => r"\pm ", "≤" => r"\le ", "≥" => r"\ge ", "≠" => r"\ne ", "→" => r"\to ", "⇒" => r"\Rightarrow ", "∂" => r"\partial ",
        "∇" => r"\nabla ", "≈" => r"\approx ", "∈" => r"\in ", "÷" => r"\div ", "{" => r"\{", "}" => r"\}", "∼" => r"\sim ", "·" => r"\cdot ",
        s => return s.to_string(),
    }.to_string()
}

const OPERATORS: &[&str] = &["+", "-", "=", "×", "±", "≤", "≥", "≠", "→", "⇒", "≈", "∈", "÷", "∼", "<", ">", "·"];

struct Sym { b: BBox, label: String }

/// Recognise handwritten math as LaTeX.
pub fn recognize_math(strokes: &[Stroke]) -> String {
    let strokes: Vec<&Stroke> = strokes.iter().filter(|s| !s.highlighter && !s.points.is_empty()).collect();
    if strokes.is_empty() { return String::new(); }
    let u = unit(&strokes);
    let rows: Vec<String> = lines(&strokes, u * 0.9, u).iter().map(|l| expr(l, u).trim().to_string()).collect();
    let src = rows.join(r" \\ ");
    // read runs of letters that spell a function name as that function
    let mut s = src;
    for f in ["sin", "cos", "tan", "log", "ln", "lim", "exp", "max", "min"] { s = s.replace(f, &format!("\\{f} ")); }
    s.replace("  ", " ").trim().to_string()
}

fn expr(strokes: &[&Stroke], u: f32) -> String {
    if strokes.is_empty() { return String::new(); }
    // 1. fraction bars: a flat stroke with ink both above and below it
    let mut bar: Option<(usize, BBox)> = None;
    for (i, s) in strokes.iter().enumerate() {
        let b = BBox::of(&[s]);
        if b.w() < u * 0.8 || b.h() > b.w() * 0.2 { continue; }
        let inside = |o: &BBox| o.cx() > b.x0 && o.cx() < b.x1;
        let above = strokes.iter().any(|o| { let ob = BBox::of(&[o]); inside(&ob) && ob.y1 < b.cy() });
        let below = strokes.iter().any(|o| { let ob = BBox::of(&[o]); inside(&ob) && ob.y0 > b.cy() });
        if above && below && bar.is_none_or(|(_, bb)| b.w() > bb.w()) { bar = Some((i, b)); }
    }
    if let Some((i, b)) = bar {
        let (mut l, mut a, mut d, mut r) = (vec![], vec![], vec![], vec![]);
        for (j, s) in strokes.iter().enumerate() {
            if j == i { continue; }
            let ob = BBox::of(&[s]);
            if ob.cx() < b.x0 { l.push(*s) } else if ob.cx() > b.x1 { r.push(*s) } else if ob.cy() < b.cy() { a.push(*s) } else { d.push(*s) }
        }
        return format!("{}\\frac{{{}}}{{{}}}{}", expr(&l, u), expr(&a, u).trim(), expr(&d, u).trim(), expr(&r, u));
    }
    // 2. square roots: a wide stroke that reads as √ holds whatever sits under it
    for (i, s) in strokes.iter().enumerate() {
        let b = BBox::of(&[s]);
        if b.w() < u * 1.3 { continue; }
        let p = probs(&render(&[s]));
        if best(&p, |l| l == "√").1 > 0.3 && best(&p, |_| true).0 == "√" {
            let (mut l, mut inner, mut r) = (vec![], vec![], vec![]);
            for (j, o) in strokes.iter().enumerate() {
                if j == i { continue; }
                let ob = BBox::of(&[o]);
                if ob.cx() < b.x0 + b.w() * 0.15 { l.push(*o) } else if ob.cx() > b.x1 { r.push(*o) } else { inner.push(*o) }
            }
            return format!("{}\\sqrt{{{}}}{}", expr(&l, u), expr(&inner, u).trim(), expr(&r, u));
        }
    }
    // 3. symbols left to right, with super/subscripts by position and size
    let mut syms: Vec<Sym> = vec![];
    for (b, g) in chars(strokes) {
        let label = if b.w().max(b.h()) < u * 0.25 { "·".to_string() } else {
            best(&probs(&render(&g)), |l| l != "√").0
        };
        syms.push(Sym { b, label });
    }
    // × between letters is an x; a dot low on the line is a decimal point
    for i in 0..syms.len() {
        if syms[i].label == "×" {
            let num = |j: Option<&Sym>| j.is_some_and(|s| is_digit(&s.label) || s.label == ")");
            if !(num(i.checked_sub(1).and_then(|j| syms.get(j))) && num(syms.get(i + 1))) { syms[i].label = "x".into(); }
        }
        if MERGED.contains(&syms[i].label) && syms[i].b.h() < u * 0.8 { syms[i].label = syms[i].label.to_lowercase(); }
        if syms[i].label == "·" && i > 0 && syms[i].b.cy() > syms[i - 1].b.y0 + syms[i - 1].b.h() * 0.75 { syms[i].label = ".".into(); }
    }
    let mut out = String::new();
    let mut base: Option<BBox> = None;
    let mut i = 0;
    while i < syms.len() {
        let s = &syms[i];
        let is_op = OPERATORS.contains(&s.label.as_str());
        let script = base.and_then(|b| {
            if is_op { return (s.b.cy() < b.y0 + b.h() * 0.2).then_some('^'); }
            if s.b.h() > b.h() * 0.8 { return None; }
            if s.b.y1 < b.y0 + b.h() * 0.6 { Some('^') } else if s.b.y0 > b.y0 + b.h() * 0.45 && s.b.y1 > b.y1 + b.h() * 0.1 { Some('_') } else { None }
        });
        if let (Some(kind), Some(b)) = (script, base) {
            // gather the whole script run
            let mut run = String::new();
            while i < syms.len() {
                let t = &syms[i];
                let same = if kind == '^' { t.b.y1 < b.y0 + b.h() * 0.65 } else { t.b.y0 > b.y0 + b.h() * 0.4 };
                if !same || t.b.h() > b.h() * 0.85 { break; }
                run.push_str(&latex_of(&t.label));
                i += 1;
            }
            out.push_str(&format!("{kind}{{{}}}", run.trim()));
            continue;
        }
        out.push_str(&latex_of(&s.label));
        if is_op { out.push(' '); }
        if !is_op { base = Some(s.b); }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn stroke(pts: &[(f32, f32)]) -> Stroke {
        Stroke { id: 0, points: pts.iter().map(|p| [p.0, p.1, 1.0]).collect(), color: [0; 4], width: 2.0, highlighter: false, pen: Default::default(), t: 0.0 }
    }
    #[test]
    fn rust_port_matches_pytorch() {
        let b = include_bytes!("../assets/hw-parity.bin");
        let n = u32::from_le_bytes(b[..4].try_into().unwrap()) as usize;
        let imgs: Vec<f32> = b[4..4 + n * 784 * 4].chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect();
        let want: Vec<u32> = b[4 + n * 784 * 4..].chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
        let same = (0..n).filter(|&i| {
            let p = probs_with(model(), &imgs[i * 784..(i + 1) * 784]);
            p.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).unwrap().0 as u32 == want[i]
        }).count();
        assert!(same >= n - 1, "only {same}/{n} match PyTorch");
    }
    /// End-to-end check on real handwriting turned into strokes (made by a script; not run by default):
    /// LEAFNOTES_HW_EVAL=/path/eval.json cargo test --release -- --ignored hw_eval --nocapture
    #[test]
    #[ignore]
    fn hw_eval() {
        let Ok(path) = std::env::var("LEAFNOTES_HW_EVAL") else { return };
        let v: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let (mut cn, mut cok, mut wn, mut wok, mut ln, mut lok) = (0, 0, 0, 0, 0, 0);
        for item in v.as_array().unwrap() {
            let segs: Vec<Stroke> = item["segs"].as_array().unwrap().iter().map(|s| {
                let p: Vec<(f32, f32)> = s.as_array().unwrap().iter().map(|q| (q[0].as_f64().unwrap() as f32, q[1].as_f64().unwrap() as f32)).collect();
                stroke(&p)
            }).collect();
            let label = item["label"].as_str().unwrap();
            if item["kind"] != "char" {
                let got = recognize_text(&segs);
                let ok = got.eq_ignore_ascii_case(label);
                let k = if item["kind"] == "word" { &mut wn } else { &mut ln };
                *k += 1;
                if *k <= 5 { println!("{label} -> {got:?}"); }
                if ok { if item["kind"] == "word" { wok += 1 } else { lok += 1 } }
            } else {
                cn += 1;
                let refs: Vec<&Stroke> = segs.iter().collect();
                if best(&probs(&render(&refs)), |_| true).0.eq_ignore_ascii_case(label) { cok += 1; }
            }
        }
        println!("characters {cok}/{cn} = {:.1}%   4-digit numbers fully right {wok}/{wn}   4-letter words fully right {lok}/{ln}", 100.0 * cok as f32 / cn as f32);
    }
    #[test]
    fn layout_and_shapes() {
        // a ring reads as zero/o, a single stroke as one character
        let ring: Vec<(f32, f32)> = (0..=40).map(|i| { let t = i as f32 / 40.0 * std::f32::consts::TAU; (20.0 + 12.0 * t.sin(), 30.0 - 20.0 * t.cos()) }).collect();
        let t = recognize_text(&[stroke(&ring)]);
        assert!(["0", "o", "O"].contains(&t.as_str()), "ring read as {t:?}");
        // two marks far apart become two words
        let two = recognize_text(&[stroke(&ring), stroke(&ring.iter().map(|p| (p.0 + 200.0, p.1)).collect::<Vec<_>>())]);
        assert_eq!(two.split(' ').count(), 2, "{two:?}");
        // a bar with marks above and below becomes a fraction; '=' is two flat strokes
        let one = |x: f32, y: f32| stroke(&[(x, y), (x, y + 30.0)]);
        let frac = recognize_math(&[one(40.0, 0.0), stroke(&[(10.0, 40.0), (70.0, 40.0)]), one(40.0, 50.0)]);
        assert!(frac.starts_with(r"\frac{"), "{frac}");
        let eq = recognize_math(&[stroke(&[(0.0, 10.0), (25.0, 10.0)]), stroke(&[(0.0, 20.0), (25.0, 20.0)])]);
        assert_eq!(eq.trim(), "=", "{eq}");
        // a small mark up and to the right is a superscript
        let sup = recognize_math(&[stroke(&ring), stroke(&[(40.0, 0.0), (40.0, 14.0)])]);
        assert!(sup.contains("^{"), "{sup}");
        // separate lines stay separate
        let ml = recognize_text(&[stroke(&ring), stroke(&ring.iter().map(|p| (p.0, p.1 + 120.0)).collect::<Vec<_>>())]);
        assert_eq!(ml.lines().count(), 2, "{ml:?}");
    }
}
