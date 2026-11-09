//! A small LaTeX math renderer drawn with egui's painter.
//! shortcut: covers the math people write in notes (fractions, roots, scripts, limits, greek, operators,
//! \left…\right, matrices/cases/aligned, accents, \text, \mathbb); swap in a full TeX engine if users need more.

use eframe::egui::{self, text::LayoutJob, Color32, FontId, Galley, Painter, Pos2, Stroke, TextFormat, Vec2};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Sym(String, bool), // text, italic
    Row(Vec<Node>),
    Frac(Box<Node>, Box<Node>, bool), // numerator, denominator, draw bar
    Sqrt(Box<Node>, Option<Box<Node>>),
    Script { base: Box<Node>, sup: Option<Box<Node>>, sub: Option<Box<Node>> },
    Space(f32),
    Big(String),
    Fenced(String, Box<Node>, String),
    Matrix { rows: Vec<Vec<Node>>, left: String, right: String, align: Align },
    Accent(Box<Node>, char),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Align { Center, Left, Alternate }

fn symbol(name: &str) -> Option<&'static str> {
    Some(match name {
        "alpha" => "α", "beta" => "β", "gamma" => "γ", "delta" => "δ", "epsilon" => "ϵ", "varepsilon" => "ε",
        "zeta" => "ζ", "eta" => "η", "theta" => "θ", "vartheta" => "ϑ", "iota" => "ι", "kappa" => "κ",
        "lambda" => "λ", "mu" => "μ", "nu" => "ν", "xi" => "ξ", "pi" => "π", "varpi" => "ϖ", "rho" => "ρ", "sigma" => "σ",
        "tau" => "τ", "upsilon" => "υ", "phi" => "ϕ", "varphi" => "φ", "chi" => "χ", "psi" => "ψ", "omega" => "ω",
        "Gamma" => "Γ", "Delta" => "Δ", "Theta" => "Θ", "Lambda" => "Λ", "Xi" => "Ξ", "Pi" => "Π",
        "Sigma" => "Σ", "Phi" => "Φ", "Psi" => "Ψ", "Omega" => "Ω", "Upsilon" => "Υ",
        "cdot" => "·", "times" => "×", "div" => "÷", "pm" => "±", "mp" => "∓", "infty" => "∞", "ast" => "∗", "star" => "⋆",
        "leq" | "le" => "≤", "geq" | "ge" => "≥", "neq" | "ne" => "≠", "approx" => "≈", "equiv" => "≡", "cong" => "≅",
        "sim" => "∼", "simeq" => "≃", "propto" => "∝", "ll" => "≪", "gg" => "≫", "to" | "rightarrow" => "→", "leftarrow" | "gets" => "←",
        "Rightarrow" | "implies" => "⇒", "Leftarrow" => "⇐", "leftrightarrow" => "↔", "Leftrightarrow" | "iff" => "⇔", "mapsto" => "↦",
        "uparrow" => "↑", "downarrow" => "↓", "longrightarrow" => "⟶", "Longrightarrow" => "⟹",
        "in" => "∈", "notin" => "∉", "ni" => "∋", "subset" => "⊂", "subseteq" => "⊆", "supset" => "⊃", "supseteq" => "⊇", "cup" => "∪",
        "cap" => "∩", "setminus" => "∖", "emptyset" | "varnothing" => "∅", "forall" => "∀", "exists" => "∃", "nexists" => "∄", "neg" | "lnot" => "¬",
        "wedge" | "land" => "∧", "vee" | "lor" => "∨", "oplus" => "⊕", "otimes" => "⊗", "partial" => "∂", "nabla" => "∇",
        "ldots" | "dots" => "…", "cdots" => "⋯", "vdots" => "⋮", "ddots" => "⋱",
        "circ" => "∘", "bullet" => "∙", "degree" => "°", "angle" => "∠", "perp" => "⊥", "parallel" => "∥", "mid" => "∣", "hbar" => "ħ",
        "ell" => "ℓ", "Re" => "ℜ", "Im" => "ℑ", "aleph" => "ℵ", "prime" => "′", "therefore" => "∴", "because" => "∵",
        "langle" => "⟨", "rangle" => "⟩", "lfloor" => "⌊", "rfloor" => "⌋", "lceil" => "⌈", "rceil" => "⌉",
        "lvert" | "rvert" | "vert" => "|", "lVert" | "rVert" | "Vert" | "|" => "‖",
        "{" | "lbrace" => "{", "}" | "rbrace" => "}", "%" => "%", "$" => "$", "#" => "#", "&" => "&", "_" => "_",
        _ => return None,
    })
}

fn big_op(name: &str) -> Option<&'static str> {
    Some(match name {
        "sum" => "∑", "prod" => "∏", "coprod" => "∐", "int" => "∫", "iint" => "∬", "iiint" => "∭", "oint" => "∮",
        "bigcup" => "⋃", "bigcap" => "⋂", "bigoplus" => "⨁", "bigotimes" => "⨂",
        _ => return None,
    })
}

const FUNCS: &[&str] = &["sin", "cos", "tan", "cot", "sec", "csc", "log", "ln", "lg", "exp", "lim", "liminf", "limsup", "max", "min",
    "det", "arcsin", "arccos", "arctan", "sinh", "cosh", "tanh", "gcd", "deg", "dim", "ker", "sup", "inf", "mod", "arg", "Pr", "sgn"];

/// Operators whose scripts sit above/below rather than to the side.
fn takes_limits(n: &Node) -> bool {
    match n {
        Node::Big(s) => !matches!(s.as_str(), "∫" | "∬" | "∭" | "∮"),
        Node::Sym(s, false) => matches!(s.as_str(), "lim" | "liminf" | "limsup" | "max" | "min" | "sup" | "inf"),
        _ => false,
    }
}

fn blackboard(c: char) -> char {
    match c { 'R' => 'ℝ', 'N' => 'ℕ', 'Z' => 'ℤ', 'Q' => 'ℚ', 'C' => 'ℂ', 'P' => 'ℙ', 'H' => 'ℍ', 'E' => '𝔼', _ => c }
}

#[derive(Debug, PartialEq, Clone, Copy)]
enum End { Eof, Brace, Amp, NewRow, EnvEnd, Right }

struct Parser { s: Vec<char>, i: usize }

impl Parser {
    fn peek(&self) -> Option<char> { self.s.get(self.i).copied() }
    fn skip_ws(&mut self) { while matches!(self.peek(), Some(' ' | '\n' | '\t' | '\r')) { self.i += 1; } }
    fn at_cmd(&self, name: &str) -> bool {
        let n: Vec<char> = name.chars().collect();
        self.peek() == Some('\\') && self.s.get(self.i + 1..self.i + 1 + n.len()) == Some(&n[..])
            && !self.s.get(self.i + 1 + n.len()).is_some_and(|c| c.is_ascii_alphabetic())
    }

    fn row(&mut self, in_group: bool) -> (Vec<Node>, End) {
        let mut out: Vec<Node> = Vec::new();
        loop {
            self.skip_ws();
            let Some(c) = self.peek() else { return (out, End::Eof) };
            match c {
                '}' => { self.i += 1; if in_group { return (out, End::Brace) } }
                '&' if !in_group => { self.i += 1; return (out, End::Amp) }
                '\\' if self.s.get(self.i + 1) == Some(&'\\') => { self.i += 2; return (out, End::NewRow) }
                '\\' if self.at_cmd("end") => { self.i += 4; self.raw_group(); return (out, End::EnvEnd) }
                '\\' if self.at_cmd("right") => { self.i += 6; return (out, End::Right) }
                '^' | '_' => {
                    self.i += 1;
                    let arg = self.arg();
                    let base = out.pop().unwrap_or(Node::Row(vec![]));
                    let (base, mut sup, mut sub) = match base {
                        Node::Script { base, sup, sub } => (base, sup, sub),
                        other => (Box::new(other), None, None),
                    };
                    if c == '^' { sup = Some(Box::new(arg)) } else { sub = Some(Box::new(arg)) }
                    out.push(Node::Script { base, sup, sub });
                }
                '\'' => {
                    self.i += 1;
                    let base = out.pop().unwrap_or(Node::Row(vec![]));
                    out.push(Node::Script { base: Box::new(base), sup: Some(Box::new(Node::Sym("′".into(), false))), sub: None });
                }
                _ => { if let Some(n) = self.atom() { out.push(n) } }
            }
        }
    }

    fn arg(&mut self) -> Node { self.skip_ws(); self.atom().unwrap_or(Node::Row(vec![])) }

    fn raw_group(&mut self) -> String {
        self.skip_ws();
        if self.peek() != Some('{') { let c = self.peek(); self.i += 1; return c.map(String::from).unwrap_or_default(); }
        self.i += 1;
        let (mut depth, mut s) = (1, String::new());
        while let Some(c) = self.peek() {
            self.i += 1;
            match c { '{' => depth += 1, '}' => { depth -= 1; if depth == 0 { break } } _ => {} }
            s.push(c);
        }
        s
    }

    fn name(&mut self) -> String {
        let mut name = String::new();
        while let Some(n) = self.peek() { if n.is_ascii_alphabetic() { name.push(n); self.i += 1; } else { break } }
        if name.is_empty() { if let Some(c) = self.peek() { name.push(c); self.i += 1; } }
        name
    }

    fn delim(&mut self) -> String {
        self.skip_ws();
        match self.peek() {
            Some('\\') => { self.i += 1; let n = self.name(); symbol(&n).unwrap_or("").into() }
            Some('.') => { self.i += 1; String::new() }
            Some(c) => { self.i += 1; c.to_string() }
            None => String::new(),
        }
    }

    fn matrix(&mut self, env: &str) -> Node {
        if env == "array" { self.raw_group(); } // column spec
        let mut rows = vec![vec![]];
        loop {
            let (cell, end) = self.row(false);
            rows.last_mut().unwrap().push(Node::Row(cell));
            match end {
                End::Amp => {}
                End::NewRow => rows.push(vec![]),
                _ => break,
            }
        }
        if rows.len() > 1 && rows.last().is_some_and(|r| r.len() == 1 && r[0] == Node::Row(vec![])) { rows.pop(); }
        let (l, r) = match env {
            "pmatrix" => ("(", ")"), "bmatrix" => ("[", "]"), "Bmatrix" => ("{", "}"), "vmatrix" => ("|", "|"),
            "Vmatrix" => ("‖", "‖"), "cases" => ("{", ""), _ => ("", ""),
        };
        let align = match env { "cases" | "array" => Align::Left, "align" | "align*" | "aligned" | "split" | "eqnarray" => Align::Alternate, _ => Align::Center };
        Node::Matrix { rows, left: l.into(), right: r.into(), align }
    }

    fn atom(&mut self) -> Option<Node> {
        let c = self.peek()?;
        self.i += 1;
        Some(match c {
            '{' => Node::Row(self.row(true).0),
            '\\' => {
                let name = self.name();
                match name.as_str() {
                    "frac" | "dfrac" | "tfrac" | "cfrac" => { let a = self.arg(); let b = self.arg(); Node::Frac(Box::new(a), Box::new(b), true) }
                    "binom" | "dbinom" => { let a = self.arg(); let b = self.arg(); Node::Fenced("(".into(), Box::new(Node::Frac(Box::new(a), Box::new(b), false)), ")".into()) }
                    "sqrt" => {
                        self.skip_ws();
                        let index = if self.peek() == Some('[') {
                            self.i += 1;
                            let start = self.i;
                            while self.peek().is_some_and(|c| c != ']') { self.i += 1; }
                            let src: String = self.s[start..self.i].iter().collect();
                            self.i += 1;
                            Some(Box::new(parse_row(&src)))
                        } else { None };
                        Node::Sqrt(Box::new(self.arg()), index)
                    }
                    "text" | "mathrm" | "operatorname" | "textrm" | "textbf" | "textit" | "mbox" => Node::Sym(self.raw_group(), false),
                    "mathbb" => Node::Sym(self.raw_group().chars().map(blackboard).collect(), false),
                    "mathbf" | "boldsymbol" | "mathit" | "mathcal" | "mathsf" | "mathfrak" | "bm" => self.arg(),
                    "left" => {
                        let l = self.delim();
                        let (inner, _) = self.row(false);
                        let r = self.delim();
                        Node::Fenced(l, Box::new(Node::Row(inner)), r)
                    }
                    "right" => Node::Row(vec![]),
                    "big" | "Big" | "bigg" | "Bigg" | "bigl" | "bigr" | "Bigl" | "Bigr" | "biggl" | "biggr" => Node::Sym(self.delim(), false),
                    "begin" => { let env = self.raw_group(); self.matrix(&env) }
                    "hat" | "widehat" => Node::Accent(Box::new(self.arg()), '^'),
                    "bar" | "overline" => Node::Accent(Box::new(self.arg()), '‾'),
                    "underline" => Node::Accent(Box::new(self.arg()), '_'),
                    "vec" | "overrightarrow" => Node::Accent(Box::new(self.arg()), '→'),
                    "dot" => Node::Accent(Box::new(self.arg()), '·'),
                    "ddot" => Node::Accent(Box::new(self.arg()), '¨'),
                    "tilde" | "widetilde" => Node::Accent(Box::new(self.arg()), '~'),
                    "overbrace" | "underbrace" | "boxed" | "displaystyle" | "textstyle" | "limits" | "nolimits" | "mathop" => {
                        if matches!(name.as_str(), "displaystyle" | "textstyle" | "limits" | "nolimits") { Node::Space(0.0) } else { self.arg() }
                    }
                    "," | ":" | ">" => Node::Space(0.17), ";" => Node::Space(0.28), "quad" => Node::Space(1.0),
                    "qquad" => Node::Space(2.0), " " => Node::Space(0.33), "!" => Node::Space(-0.1),
                    n if FUNCS.contains(&n) => Node::Sym(n.to_string(), false),
                    n => if let Some(s) = big_op(n) { Node::Big(s.into()) }
                         else if let Some(s) = symbol(n) { Node::Sym(s.into(), false) }
                         else { Node::Sym(format!("\\{n}"), false) },
                }
            }
            c if c.is_alphabetic() => Node::Sym(c.to_string(), true),
            c => Node::Sym(c.to_string(), false),
        })
    }
}

fn parse_row(src: &str) -> Node { Node::Row(Parser { s: src.chars().collect(), i: 0 }.row(true).0) }

/// Parse a LaTeX math expression. Top-level `\\` and `&` lay out as aligned lines.
pub fn parse(src: &str) -> Node {
    let src = src.trim().trim_start_matches("$$").trim_end_matches("$$").trim_matches('$');
    // the parser and layout recurse per level of nesting: absurdly deep input is shown as plain text
    // rather than overflowing the stack (it's drawn every frame)
    let deepest = src.chars().scan(0i32, |d, c| { *d += match c { '{' => 1, '}' => -1, _ => 0 }; Some(*d) }).max().unwrap_or(0);
    if deepest > 64 || src.len() > 8000 || src.matches('\\').count() > 1500 { return Node::Sym(src.chars().take(2000).collect(), false); }
    let mut p = Parser { s: src.chars().collect(), i: 0 };
    let mut rows = vec![vec![]];
    loop {
        let (cell, end) = p.row(false);
        rows.last_mut().unwrap().push(Node::Row(cell));
        match end {
            End::Amp => {}
            End::NewRow => rows.push(vec![]),
            End::Eof => break,
            _ => {} // stray \end or \right: keep going
        }
    }
    if rows.len() == 1 && rows[0].len() == 1 { return rows.pop().unwrap().pop().unwrap(); }
    Node::Matrix { rows, left: String::new(), right: String::new(), align: Align::Alternate }
}

enum Item { Text(Vec2, Arc<Galley>), Line(Vec2, Vec2, f32) }

/// Box with origin at the left end of the baseline; y grows downward.
struct MBox { w: f32, asc: f32, desc: f32, items: Vec<Item> }

impl MBox {
    fn empty(size: f32) -> Self { MBox { w: 0.0, asc: size * 0.6, desc: size * 0.2, items: vec![] } }
    fn append(&mut self, other: MBox, dx: f32, dy: f32) {
        for it in other.items {
            self.items.push(match it {
                Item::Text(p, g) => Item::Text(p + Vec2::new(dx, dy), g),
                Item::Line(a, b, w) => Item::Line(a + Vec2::new(dx, dy), b + Vec2::new(dx, dy), w),
            });
        }
    }
}

fn text_box(p: &Painter, s: &str, size: f32, italic: bool, color: Color32) -> MBox {
    let mut job = LayoutJob::default();
    job.append(s, 0.0, TextFormat { font_id: FontId::proportional(size), color, italics: italic, ..Default::default() });
    let g = p.layout_job(job);
    let h = g.size().y;
    let asc = h * 0.78;
    MBox { w: g.size().x + if italic { size * 0.06 } else { 0.0 }, asc, desc: h - asc, items: vec![Item::Text(Vec2::new(0.0, -asc), g)] }
}

/// A delimiter glyph stretched to cover a box of the given ascent/descent.
fn delim_box(p: &Painter, d: &str, size: f32, asc: f32, desc: f32, color: Color32) -> MBox {
    if d.is_empty() { return MBox { w: size * 0.1, asc: 0.0, desc: 0.0, items: vec![] }; }
    let h = asc + desc;
    let s = (h / 1.12).max(size);
    let mut b = text_box(p, d, s, false, color);
    let dy = (desc - asc) / 2.0 - (b.desc - b.asc) / 2.0;
    let mut out = MBox { w: b.w, asc: b.asc - dy, desc: b.desc + dy, items: vec![] };
    b.w = 0.0;
    out.append(b, 0.0, dy);
    out
}

fn layout(p: &Painter, n: &Node, size: f32, color: Color32) -> MBox {
    let lw = (size * 0.06).max(1.0);
    match n {
        Node::Sym(s, it) => {
            let pad = if matches!(s.as_str(), "=" | "+" | "−" | "-" | "<" | ">" | "≤" | "≥" | "≠" | "≈" | "≡" | "→" | "⇒" | "⇔" | "×" | "·" | "±" | "∈" | "⊂" | "⊆" | "∼" | "∝" | "↦") { size * 0.18 } else { 0.0 };
            let s = if s == "-" { "−" } else { s.as_str() };
            let mut b = text_box(p, s, size, *it, color);
            if pad > 0.0 { let mut o = MBox { w: b.w + 2.0 * pad, asc: b.asc, desc: b.desc, items: vec![] }; o.append(b, pad, 0.0); b = o; }
            b
        }
        Node::Big(s) => {
            let mut b = text_box(p, s, size * 1.45, false, color);
            let shift = size * 0.12; // centre the big operator on the math axis
            let mut o = MBox { w: b.w + size * 0.08, asc: b.asc - shift, desc: b.desc + shift, items: vec![] };
            b.w = 0.0;
            o.append(b, 0.0, shift);
            o
        }
        Node::Space(em) => MBox { w: em * size, asc: 0.0, desc: 0.0, items: vec![] },
        Node::Row(items) => {
            let mut out = MBox::empty(size);
            if !items.is_empty() { out.asc = 0.0; out.desc = 0.0; }
            for it in items {
                let b = layout(p, it, size, color);
                out.asc = out.asc.max(b.asc);
                out.desc = out.desc.max(b.desc);
                let x = out.w;
                out.w += b.w;
                out.append(b, x, 0.0);
            }
            out
        }
        Node::Frac(a, b, bar) => {
            let s = (size * 0.85).max(9.0);
            let (num, den) = (layout(p, a, s, color), layout(p, b, s, color));
            let w = num.w.max(den.w) + size * 0.3;
            let axis = size * 0.28;
            let gap = size * 0.12;
            let mut out = MBox { w, asc: axis + gap + num.asc + num.desc, desc: den.asc + den.desc + gap - axis, items: vec![] };
            if *bar { out.items.push(Item::Line(Vec2::new(size * 0.05, -axis), Vec2::new(w - size * 0.05, -axis), lw)); }
            let (nw, dw, ndesc, dasc) = (num.w, den.w, num.desc, den.asc);
            out.append(num, (w - nw) / 2.0, -axis - gap - ndesc);
            out.append(den, (w - dw) / 2.0, -axis + gap + dasc);
            out
        }
        Node::Sqrt(inner, index) => {
            let b = layout(p, inner, size, color);
            let pad = size * 0.12;
            let hook = size * 0.55;
            let (asc, desc, bw) = (b.asc + pad * 2.0, b.desc, b.w);
            let mut out = MBox { w: hook + bw + pad, asc, desc, items: vec![] };
            out.items.push(Item::Line(Vec2::new(0.0, -asc * 0.35), Vec2::new(hook * 0.35, -asc * 0.45), lw));
            out.items.push(Item::Line(Vec2::new(hook * 0.35, -asc * 0.45), Vec2::new(hook * 0.6, desc), lw));
            out.items.push(Item::Line(Vec2::new(hook * 0.6, desc), Vec2::new(hook, -asc + lw), lw));
            out.items.push(Item::Line(Vec2::new(hook, -asc + lw), Vec2::new(hook + bw + pad, -asc + lw), lw));
            out.append(b, hook + pad * 0.5, 0.0);
            if let Some(ix) = index {
                let ib = layout(p, ix, (size * 0.55).max(7.0), color);
                let dy = -asc * 0.45 - ib.desc;
                out.asc = out.asc.max(-dy + ib.asc);
                out.append(ib, 0.0, dy);
            }
            out
        }
        Node::Script { base, sup, sub } if takes_limits(base) => {
            let b = layout(p, base, size, color);
            let s = (size * 0.68).max(8.0);
            let (sup, sub) = (sup.as_ref().map(|n| layout(p, n, s, color)), sub.as_ref().map(|n| layout(p, n, s, color)));
            let w = b.w.max(sup.as_ref().map_or(0.0, |x| x.w)).max(sub.as_ref().map_or(0.0, |x| x.w));
            let gap = size * 0.05;
            let mut out = MBox { w: w + size * 0.1, asc: b.asc, desc: b.desc, items: vec![] };
            // Big glyph boxes include line spacing; tuck the limits in to the visible glyph.
            let tight = if matches!(**base, Node::Big(_)) { (0.66, 0.45) } else { (1.0, 1.0) };
            let (bw, basc, bdesc) = (b.w, b.asc * tight.0, b.desc * tight.1);
            out.append(b, (w - bw) / 2.0, 0.0);
            if let Some(x) = sup {
                let dy = -basc - gap - x.desc;
                out.asc = -dy + x.asc;
                let xw = x.w;
                out.append(x, (w - xw) / 2.0, dy);
            }
            if let Some(x) = sub {
                let dy = bdesc + gap + x.asc;
                out.desc = dy + x.desc;
                let xw = x.w;
                out.append(x, (w - xw) / 2.0, dy);
            }
            out
        }
        Node::Script { base, sup, sub } => {
            let mut out = layout(p, base, size, color);
            let s = (size * 0.68).max(8.0);
            let x = out.w;
            let mut extra: f32 = 0.0;
            if let Some(sup) = sup {
                let b = layout(p, sup, s, color);
                let shift = (out.asc * 0.55).max(size * 0.38);
                out.asc = out.asc.max(shift + b.asc);
                extra = extra.max(b.w);
                out.append(b, x + size * 0.03, -shift);
            }
            if let Some(sub) = sub {
                let b = layout(p, sub, s, color);
                let shift = (out.desc + size * 0.05).max(size * 0.25);
                out.desc = out.desc.max(shift + b.desc);
                extra = extra.max(b.w);
                out.append(b, x + size * 0.03, shift);
            }
            out.w += extra + size * 0.05;
            out
        }
        Node::Fenced(l, inner, r) => {
            let mut b = layout(p, inner, size, color);
            b.asc = b.asc.max(size * 0.7);
            b.desc = b.desc.max(size * 0.2);
            layout_fence_box(p, l, b, r, size, color)
        }
        Node::Matrix { rows, left, right, align } => {
            let cells: Vec<Vec<MBox>> = rows.iter().map(|r| r.iter().map(|c| layout(p, c, size, color)).collect()).collect();
            let ncols = cells.iter().map(|r| r.len()).max().unwrap_or(0);
            let mut colw = vec![0f32; ncols];
            for r in &cells { for (j, c) in r.iter().enumerate() { colw[j] = colw[j].max(c.w); } }
            let row_gap = size * 0.3;
            let col_gap = if *align == Align::Alternate { size * 0.1 } else { size * 0.9 };
            let heights: Vec<(f32, f32)> = cells.iter().map(|r| r.iter().fold((size * 0.7, size * 0.25), |a, c| (a.0.max(c.asc), a.1.max(c.desc)))).collect();
            let total: f32 = heights.iter().map(|h| h.0 + h.1).sum::<f32>() + row_gap * (heights.len().saturating_sub(1)) as f32;
            let axis = size * 0.28;
            let top = -axis - total / 2.0;
            let width: f32 = colw.iter().sum::<f32>() + col_gap * (ncols.saturating_sub(1)) as f32;
            let mut body = MBox { w: width, asc: -top, desc: top + total, items: vec![] };
            let mut y = top;
            for (r, (ra, rd)) in cells.into_iter().zip(heights) {
                let base = y + ra;
                let mut x = 0.0;
                for (j, c) in r.into_iter().enumerate() {
                    let slack = colw[j] - c.w;
                    let dx = match align { Align::Center => slack / 2.0, Align::Left => 0.0, Align::Alternate => if j % 2 == 0 { slack } else { 0.0 } };
                    body.append(c, x + dx, base);
                    x += colw[j] + col_gap;
                }
                y += ra + rd + row_gap;
            }
            if left.is_empty() && right.is_empty() { return body; }
            layout_fence_box(p, left, body, right, size, color)
        }
        Node::Accent(inner, a) => {
            let b = layout(p, inner, size, color);
            let mut out = MBox { w: b.w, asc: b.asc, desc: b.desc, items: vec![] };
            let (bw, basc, bdesc) = (b.w, b.asc, b.desc);
            out.append(b, 0.0, 0.0);
            match a {
                '‾' => { out.items.push(Item::Line(Vec2::new(0.0, -basc - lw * 2.0), Vec2::new(bw, -basc - lw * 2.0), lw)); out.asc += lw * 3.0; }
                '_' => { out.items.push(Item::Line(Vec2::new(0.0, bdesc + lw), Vec2::new(bw, bdesc + lw), lw)); out.desc += lw * 2.0; }
                c => {
                    let g = text_box(p, &c.to_string(), size * 0.7, false, color);
                    let (gw, gdesc, gasc) = (g.w, g.desc, g.asc);
                    let dy = -basc + size * 0.32 - gdesc;
                    out.asc = out.asc.max(-dy + gasc - size * 0.25);
                    out.append(g, (bw - gw) / 2.0 + size * 0.04, dy);
                }
            }
            out
        }
    }
}

fn layout_fence_box(p: &Painter, l: &str, b: MBox, r: &str, size: f32, color: Color32) -> MBox {
    let (asc, desc) = (b.asc, b.desc);
    let (lb, rb) = (delim_box(p, l, size, asc, desc, color), delim_box(p, r, size, asc, desc, color));
    let mut out = MBox { w: 0.0, asc: asc.max(lb.asc).max(rb.asc), desc: desc.max(lb.desc).max(rb.desc), items: vec![] };
    let (lw, bw, rw) = (lb.w, b.w, rb.w);
    out.append(lb, 0.0, 0.0);
    out.append(b, lw + size * 0.15, 0.0);
    out.append(rb, lw + bw + size * 0.3, 0.0);
    out.w = lw + bw + rw + size * 0.3;
    out
}

/// Lay out and draw `src` as math; returns the widget response.
pub fn math_label(ui: &mut egui::Ui, src: &str, size: f32) -> egui::Response {
    let color = ui.visuals().text_color();
    let node = parse(src);
    let mut b = layout(ui.painter(), &node, size, color);
    let avail = ui.available_width();
    if b.w > avail && avail > 60.0 {
        // Shrink wide equations to fit their note (down to 55%) instead of spilling over neighbours.
        b = layout(ui.painter(), &node, size * (avail / b.w).max(0.55), color);
    }
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(b.w.max(8.0), b.asc + b.desc), egui::Sense::click());
    let origin = Pos2::new(rect.left(), rect.top() + b.asc);
    let painter = ui.painter_at(rect.expand(4.0));
    for it in b.items {
        match it {
            Item::Text(pos, g) => painter.galley(origin + pos, g, color),
            Item::Line(a, c, w) => { painter.line_segment([origin + a, origin + c], Stroke::new(w, color)); }
        }
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_common_math() {
        let n = parse(r"\frac{a}{b} + x^2_i - \sqrt{\alpha}");
        let Node::Row(items) = n else { panic!() };
        assert!(matches!(items[0], Node::Frac(..)));
        assert!(matches!(&items[2], Node::Script { sup: Some(_), sub: Some(_), .. }));
        assert_eq!(items[4], Node::Sqrt(Box::new(Node::Row(vec![Node::Sym("α".into(), false)])), None));
        assert_eq!(parse(r"\mathbb{R}"), Node::Row(vec![Node::Sym("ℝ".into(), false)]));
        assert_eq!(parse(r"\sum"), Node::Row(vec![Node::Big("∑".into())]));
    }
    #[test]
    fn deep_nesting_stays_plain() {
        let deep = "{".repeat(5000) + "x" + &"}".repeat(5000);
        assert!(matches!(parse(&deep), Node::Sym(..)));
        assert!(matches!(parse(&"\\sqrt".repeat(3000)), Node::Sym(..)));
    }
    #[test]
    fn parses_structures() {
        let Node::Row(m) = parse(r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}") else { panic!() };
        let Node::Matrix { rows, left, .. } = &m[0] else { panic!("{m:?}") };
        assert_eq!((rows.len(), rows[0].len(), left.as_str()), (2, 2, "("));
        let Node::Row(f) = parse(r"\left( \frac{1}{2} \right]") else { panic!() };
        assert!(matches!(&f[0], Node::Fenced(l, _, r) if l == "(" && r == "]"));
        assert!(matches!(parse(r"a &= b \\ c &= d"), Node::Matrix { ref rows, .. } if rows.len() == 2));
        let Node::Row(h) = parse(r"\hat{x} + \sqrt[3]{y}") else { panic!() };
        assert!(matches!(h[0], Node::Accent(_, '^')));
        assert!(matches!(&h[2], Node::Sqrt(_, Some(_))));
        let Node::Row(c) = parse(r"f(x) = \begin{cases} 1 & x > 0 \\ 0 & \text{otherwise} \end{cases}") else { panic!() };
        assert!(matches!(c.last(), Some(Node::Matrix { align: Align::Left, .. })));
    }
}
