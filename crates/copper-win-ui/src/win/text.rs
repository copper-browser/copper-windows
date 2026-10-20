//! DirectWrite: the faces, text formats and the per-node layout cache.
//!
//! Text is set in Segoe UI Variable — its Text optical size below 20 DIPs,
//! its Display cut from 20 up — falling back to Segoe UI on Windows 10, and
//! glyphs come from Segoe Fluent Icons (Segoe MDL2 Assets on Windows 10).
//! Anything those faces lack (CJK, Arabic, emoji) comes from DirectWrite's
//! system font fallback; colour emoji are drawn in colour by the painter.
//!
//! Every text node keeps one `IDWriteTextLayout` keyed by its id and text
//! revision; resizing only changes the layout's maximum width, so a steady
//! frame builds no layouts at all.

use std::collections::HashMap;

use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT, DWRITE_HIT_TEST_METRICS, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_PARAGRAPH_ALIGNMENT_NEAR, DWRITE_READING_DIRECTION_LEFT_TO_RIGHT,
    DWRITE_READING_DIRECTION_RIGHT_TO_LEFT, DWRITE_TEXT_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TEXT_ALIGNMENT_TRAILING, DWRITE_TEXT_METRICS,
    DWRITE_TRIMMING, DWRITE_TRIMMING_GRANULARITY_CHARACTER, DWRITE_WORD_WRAPPING_NO_WRAP,
    DWRITE_WORD_WRAPPING_WRAP, DWriteCreateFactory, IDWriteFactory, IDWriteFontCollection,
    IDWriteTextFormat, IDWriteTextLayout,
};
use windows::core::{BOOL, HSTRING, PCWSTR, Result, w};

use crate::edit::{byte_of_utf16, first_strong_rtl, utf16_of};
use crate::geom::{Rect, Size};
use crate::layout::Measure;
use crate::node::{Content, Field, Text, TextAlign, Trim, Weight};
use crate::tokens::font;
use crate::tree::{NodeId, Tree};

/// The DirectWrite factory, made off the UI thread at startup.
pub struct Factory(pub IDWriteFactory);

// SAFETY: the shared DirectWrite factory is thread-safe by contract.
unsafe impl Send for Factory {}

/// Make the shared factory and warm the system font collection, so the
/// first frame does not pay for either.
pub fn create_factory() -> Result<Factory> {
    // SAFETY: plain factory creation.
    let f: IDWriteFactory = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)? };
    let mut collection = None;
    // SAFETY: out-pointer is a local Option.
    unsafe { f.GetSystemFontCollection(&mut collection, false)? };
    Ok(Factory(f))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct FormatKey {
    /// Size in eighths of a DIP.
    size: u32,
    weight: Weight,
    icon: bool,
    /// `None`: one line, never cut (fields).
    trim: Option<Trim>,
    align: TextAlign,
    rtl: bool,
}

struct Placeholder {
    text: String,
    key: FormatKey,
    layout: IDWriteTextLayout,
    max_w: f32,
    max_h: f32,
}

struct Entry {
    revision: u32,
    key: FormatKey,
    layout: IDWriteTextLayout,
    max_w: f32,
    max_h: f32,
    /// For fields: the drawn string (text with the composition spliced in),
    /// its caret, and the composition's byte range.
    display: String,
    caret: usize,
    composition: Option<std::ops::Range<usize>>,
}

/// The faces and every cached format and layout.
pub struct Fonts {
    factory: IDWriteFactory,
    text: HSTRING,
    display: HSTRING,
    icons: HSTRING,
    formats: HashMap<FormatKey, IDWriteTextFormat>,
    layouts: HashMap<(u32, NodeId), Entry>,
    placeholders: HashMap<(u32, NodeId), Placeholder>,
    /// Fields' ghost completions, laid out like placeholders.
    ghosts: HashMap<(u32, NodeId), Placeholder>,
    /// Which tree the node ids belong to (the window's, a menu's, a
    /// tooltip's): their ids overlap, their layouts must not.
    space: u32,
    line_heights: HashMap<(u32, Weight), f32>,
    scratch: Vec<u16>,
    pruned_rev: u32,
}

fn has_family(c: &IDWriteFontCollection, name: &str) -> bool {
    let mut index = 0;
    let mut exists = BOOL(0);
    let name = HSTRING::from(name);
    // SAFETY: valid out-pointers.
    unsafe { c.FindFamilyName(&name, &mut index, &mut exists).is_ok() && exists.as_bool() }
}

impl Fonts {
    pub fn new(factory: Factory) -> Result<Fonts> {
        let f = factory.0;
        let mut collection = None;
        // SAFETY: out-pointer is a local Option.
        unsafe { f.GetSystemFontCollection(&mut collection, false)? };
        let (text, display, icons) = match &collection {
            Some(c) => {
                let variable = has_family(c, font::TEXT);
                let icons = if has_family(c, font::ICONS) {
                    font::ICONS
                } else {
                    font::ICONS_FALLBACK
                };
                if variable {
                    (font::TEXT, font::DISPLAY, icons)
                } else {
                    (font::FALLBACK, font::FALLBACK, icons)
                }
            }
            None => (font::FALLBACK, font::FALLBACK, font::ICONS_FALLBACK),
        };
        Ok(Fonts {
            factory: f,
            text: HSTRING::from(text),
            display: HSTRING::from(display),
            icons: HSTRING::from(icons),
            formats: HashMap::new(),
            layouts: HashMap::new(),
            placeholders: HashMap::new(),
            ghosts: HashMap::new(),
            space: 0,
            line_heights: HashMap::new(),
            scratch: Vec::with_capacity(256),
            pruned_rev: u32::MAX,
        })
    }

    /// Use the layouts of tree `space` from now on (see the field).
    pub fn set_space(&mut self, space: u32) {
        self.space = space;
    }

    /// Forget every layout of tree `space` (a popup closed).
    pub fn forget_space(&mut self, space: u32) {
        self.layouts.retain(|k, _| k.0 != space);
        self.placeholders.retain(|k, _| k.0 != space);
        self.ghosts.retain(|k, _| k.0 != space);
    }

    fn format(&mut self, key: FormatKey) -> Result<IDWriteTextFormat> {
        if let Some(f) = self.formats.get(&key) {
            return Ok(f.clone());
        }
        let size = key.size as f32 / 8.0;
        let family = if key.icon {
            &self.icons
        } else if size >= font::DISPLAY_FROM {
            &self.display
        } else {
            &self.text
        };
        // SAFETY: valid strings and enums; the format is owned by the cache.
        let f = unsafe {
            let f = self.factory.CreateTextFormat(
                family,
                None,
                DWRITE_FONT_WEIGHT(key.weight.value() as i32),
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                size,
                w!(""),
            )?;
            f.SetTextAlignment(match key.align {
                TextAlign::Leading => DWRITE_TEXT_ALIGNMENT_LEADING,
                TextAlign::Center => DWRITE_TEXT_ALIGNMENT_CENTER,
                TextAlign::Trailing => DWRITE_TEXT_ALIGNMENT_TRAILING,
            })?;
            f.SetReadingDirection(if key.rtl {
                DWRITE_READING_DIRECTION_RIGHT_TO_LEFT
            } else {
                DWRITE_READING_DIRECTION_LEFT_TO_RIGHT
            })?;
            if key.trim == Some(Trim::Wrap) {
                f.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP)?;
                f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_NEAR)?;
            } else {
                f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
                f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
                if let Some(trim) = key.trim.filter(|_| !key.icon) {
                    let sign = self.factory.CreateEllipsisTrimmingSign(&f)?;
                    let (delimiter, delimiter_count) = match trim {
                        // Keep the end of a host in sight: "…example.com".
                        Trim::Middle => (u32::from('.'), 1),
                        _ => (0, 0),
                    };
                    let t = DWRITE_TRIMMING {
                        granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                        delimiter,
                        delimiterCount: delimiter_count,
                    };
                    f.SetTrimming(&t, &sign)?;
                }
            }
            f
        };
        self.formats.insert(key, f.clone());
        Ok(f)
    }

    fn make_layout(
        &mut self,
        s: &str,
        key: FormatKey,
        max_w: f32,
        max_h: f32,
    ) -> Result<IDWriteTextLayout> {
        let format = self.format(key)?;
        self.scratch.clear();
        self.scratch.extend(s.encode_utf16());
        // SAFETY: the string slice outlives the call; DirectWrite copies it.
        unsafe {
            self.factory
                .CreateTextLayout(&self.scratch, &format, max_w.max(0.0), max_h.max(0.0))
        }
    }

    fn key_of(t: &Text) -> FormatKey {
        FormatKey {
            size: (t.size * 8.0).round() as u32,
            weight: t.weight,
            icon: t.icon,
            trim: Some(t.trim),
            align: t.align,
            rtl: !t.icon && first_strong_rtl(&t.text),
        }
    }

    fn key_of_field(f: &Field, rtl: bool) -> FormatKey {
        FormatKey {
            size: (f.size * 8.0).round() as u32,
            weight: f.weight,
            icon: false,
            trim: None,
            align: TextAlign::Leading,
            rtl,
        }
    }

    /// The layout for a text node, sized to `max_w` × `max_h`.
    pub fn text_layout(
        &mut self,
        id: NodeId,
        t: &Text,
        max_w: f32,
        max_h: f32,
    ) -> Option<IDWriteTextLayout> {
        let key = Self::key_of(t);
        let fresh = matches!(self.layouts.get(&(self.space, id)), Some(e) if e.revision == t.revision && e.key == key);
        if !fresh {
            let layout = self.make_layout(&t.text, key, max_w, max_h).ok()?;
            self.layouts.insert(
                (self.space, id),
                Entry {
                    revision: t.revision,
                    key,
                    layout,
                    max_w,
                    max_h,
                    display: String::new(),
                    caret: 0,
                    composition: None,
                },
            );
        }
        let e = self.layouts.get_mut(&(self.space, id))?;
        resize(e, max_w, max_h);
        Some(e.layout.clone())
    }

    /// The layout for a field's drawn string (text plus composition), and
    /// that string's caret and composition range.
    pub fn field_layout(
        &mut self,
        id: NodeId,
        f: &Field,
        max_w: f32,
        max_h: f32,
    ) -> Option<FieldLayout> {
        let fresh =
            matches!(self.layouts.get(&(self.space, id)), Some(e) if e.revision == f.revision);
        if !fresh {
            let (display, caret, composition) = f.shown();
            let rtl = first_strong_rtl(&display);
            let key = Self::key_of_field(f, rtl);
            let layout = self.make_layout(&display, key, max_w, max_h).ok()?;
            self.layouts.insert(
                (self.space, id),
                Entry {
                    revision: f.revision,
                    key,
                    layout,
                    max_w,
                    max_h,
                    display,
                    caret,
                    composition,
                },
            );
        }
        let e = self.layouts.get_mut(&(self.space, id))?;
        resize(e, max_w, max_h);
        Some(FieldLayout {
            layout: e.layout.clone(),
            caret16: utf16_of(&e.display, e.caret) as u32,
        })
    }

    /// A byte offset in a field's drawn string as UTF-16 units (after
    /// [`Fonts::field_layout`] for the same field).
    pub fn field_utf16(&self, id: NodeId, display_byte: usize) -> u32 {
        self.layouts.get(&(self.space, id)).map_or(0, |e| {
            utf16_of(&e.display, display_byte.min(e.display.len())) as u32
        })
    }

    /// Byte offset in the field's text under `x` (field-local DIPs, after
    /// scrolling), snapped to a grapheme boundary.
    pub fn field_hit(&mut self, id: NodeId, f: &Field, x: f32, w: f32, h: f32) -> usize {
        let Some(fl) = self.field_layout(id, f, w, h) else {
            return f.edit.caret();
        };
        let mut trailing = BOOL(0);
        let mut inside = BOOL(0);
        let mut m = DWRITE_HIT_TEST_METRICS::default();
        // SAFETY: valid out-pointers on a live layout.
        let ok = unsafe {
            fl.layout
                .HitTestPoint(x, h / 2.0, &mut trailing, &mut inside, &mut m)
                .is_ok()
        };
        if !ok {
            return f.edit.caret();
        }
        let mut pos16 = m.textPosition as usize;
        if trailing.as_bool() {
            pos16 += m.length as usize;
        }
        // Hits inside a composition land on its start.
        let Some(e) = self.layouts.get(&(self.space, id)) else {
            return f.edit.caret();
        };
        let shown = byte_of_utf16(&e.display, pos16);
        let mut byte = f.display_byte(shown);
        if let Some(r) = &e.composition {
            let r = f.display_byte(r.start)..f.display_byte(r.end);
            if byte >= r.end {
                byte -= r.end - r.start;
            } else if byte > r.start {
                byte = r.start;
            }
        }
        f.edit.snap(byte)
    }

    /// The natural line height of a field at its size.
    pub fn line_height(&mut self, size: f32, weight: Weight) -> f32 {
        let k = ((size * 8.0).round() as u32, weight);
        if let Some(h) = self.line_heights.get(&k) {
            return *h;
        }
        let key = FormatKey {
            size: k.0,
            weight,
            icon: false,
            trim: None,
            align: TextAlign::Leading,
            rtl: false,
        };
        let h = self
            .make_layout("Ag", key, 1.0e6, 1.0e6)
            .ok()
            .and_then(|l| metrics(&l))
            .map_or(size * 1.33, |m| m.height.ceil());
        self.line_heights.insert(k, h);
        h
    }

    /// A field's placeholder, laid out in `w`×`h`.
    pub fn placeholder_layout(
        &mut self,
        id: NodeId,
        f: &Field,
        w: f32,
        h: f32,
    ) -> Option<IDWriteTextLayout> {
        let key = FormatKey {
            size: (f.size * 8.0).round() as u32,
            weight: f.weight,
            icon: false,
            trim: Some(Trim::End),
            align: TextAlign::Leading,
            rtl: first_strong_rtl(&f.placeholder),
        };
        let fresh = matches!(self.placeholders.get(&(self.space, id)), Some(p) if p.key == key && p.text == f.placeholder);
        if !fresh {
            let layout = self.make_layout(&f.placeholder, key, w, h).ok()?;
            self.placeholders.insert(
                (self.space, id),
                Placeholder {
                    text: f.placeholder.clone(),
                    key,
                    layout,
                    max_w: w,
                    max_h: h,
                },
            );
        }
        let p = self.placeholders.get_mut(&(self.space, id))?;
        // SAFETY: setters on a live layout.
        unsafe {
            if p.max_w != w {
                let _ = p.layout.SetMaxWidth(w.max(0.0));
                p.max_w = w;
            }
            if p.max_h != h {
                let _ = p.layout.SetMaxHeight(h.max(0.0));
                p.max_h = h;
            }
        }
        Some(p.layout.clone())
    }

    /// Natural size of a text node in `room`.
    pub fn measure_text(&mut self, id: NodeId, t: &Text, room: Size) -> Size {
        let w = if room.w.is_finite() { room.w } else { 1.0e6 };
        let Some(l) = self.text_layout(id, t, w, 1.0e6) else {
            return Size::ZERO;
        };
        match metrics(&l) {
            Some(m) => {
                let width = if t.trim == Trim::Wrap {
                    m.width
                } else {
                    m.widthIncludingTrailingWhitespace
                };
                Size::new(width.ceil().min(w), m.height.ceil())
            }
            None => Size::ZERO,
        }
    }

    /// Drop layouts of the window tree's nodes that are gone (when its
    /// shape changed).
    pub fn prune(&mut self, tree: &Tree) {
        // Popup trees live briefly and are forgotten whole on close.
        let rev = tree.structure_rev();
        if self.space != 0 || rev == self.pruned_rev {
            return;
        }
        self.pruned_rev = rev;
        let space = self.space;
        self.layouts
            .retain(|k, _| k.0 != space || tree.contains(k.1));
        self.placeholders
            .retain(|k, _| k.0 != space || tree.contains(k.1));
        self.ghosts
            .retain(|k, _| k.0 != space || tree.contains(k.1));
    }

    /// A field's ghost completion, on one line of height `h`, never cut
    /// (the field's clip ends it).
    pub fn ghost_layout(&mut self, id: NodeId, f: &Field, h: f32) -> Option<IDWriteTextLayout> {
        let key = FormatKey {
            size: (f.size * 8.0).round() as u32,
            weight: f.weight,
            icon: false,
            trim: None,
            align: TextAlign::Leading,
            rtl: false,
        };
        let fresh = matches!(self.ghosts.get(&(self.space, id)), Some(p) if p.key == key && p.text == f.ghost && p.max_h == h);
        if !fresh {
            let layout = self.make_layout(&f.ghost, key, 1.0e6, h).ok()?;
            self.ghosts.insert(
                (self.space, id),
                Placeholder {
                    text: f.ghost.clone(),
                    key,
                    layout,
                    max_w: 1.0e6,
                    max_h: h,
                },
            );
        }
        self.ghosts.get(&(self.space, id)).map(|p| p.layout.clone())
    }
}

fn resize(e: &mut Entry, max_w: f32, max_h: f32) {
    // SAFETY: setters on a live layout.
    unsafe {
        if e.max_w != max_w {
            let _ = e.layout.SetMaxWidth(max_w.max(0.0));
            e.max_w = max_w;
        }
        if e.max_h != max_h {
            let _ = e.layout.SetMaxHeight(max_h.max(0.0));
            e.max_h = max_h;
        }
    }
}

/// A layout's overall metrics.
pub fn metrics(l: &IDWriteTextLayout) -> Option<DWRITE_TEXT_METRICS> {
    let mut m = DWRITE_TEXT_METRICS::default();
    // SAFETY: valid out-pointer.
    unsafe { l.GetMetrics(&mut m).ok().map(|_| m) }
}

/// A field's drawn layout and where things are in it (UTF-16 units).
pub struct FieldLayout {
    pub layout: IDWriteTextLayout,
    pub caret16: u32,
}

impl FieldLayout {
    /// The caret's x and the line's top and height, layout-local.
    pub fn caret(&self) -> (f32, f32, f32) {
        let (mut x, mut y) = (0.0, 0.0);
        let mut m = DWRITE_HIT_TEST_METRICS::default();
        // SAFETY: valid out-pointers on a live layout.
        unsafe {
            let _ = self
                .layout
                .HitTestTextPosition(self.caret16, false, &mut x, &mut y, &mut m);
        }
        (x, m.top, m.height)
    }

    /// The rectangles covering UTF-16 range `r`, layout-local, into `out`.
    pub fn range_rects(&self, r: std::ops::Range<u32>, out: &mut Vec<Rect>) {
        out.clear();
        if r.is_empty() {
            return;
        }
        let mut buf = [DWRITE_HIT_TEST_METRICS::default(); 8];
        let mut n = 0u32;
        // SAFETY: the buffer outlives the call; count is written back.
        let ok = unsafe {
            self.layout
                .HitTestTextRange(r.start, r.end - r.start, 0.0, 0.0, Some(&mut buf), &mut n)
                .is_ok()
        };
        if ok {
            for m in buf.iter().take(n as usize) {
                out.push(Rect::new(m.left, m.top, m.width, m.height));
            }
        }
    }
}

/// Measures leaves with DirectWrite for [`crate::layout`].
pub struct Measurer<'a> {
    pub fonts: &'a mut Fonts,
}

impl Measure for Measurer<'_> {
    fn measure(&mut self, tree: &Tree, id: NodeId, room: Size) -> Size {
        let Some(n) = tree.get(id) else {
            return Size::ZERO;
        };
        match &n.content {
            Content::Text(t) => self.fonts.measure_text(id, t, room),
            Content::Field(f) => Size::new(0.0, self.fonts.line_height(f.size, f.weight)),
            _ => Size::ZERO,
        }
    }
}

/// A NUL-terminated UTF-16 copy of `s` for Win32 calls.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// `PCWSTR` over a buffer from [`wide`].
pub fn pcwstr(buf: &[u16]) -> PCWSTR {
    PCWSTR(buf.as_ptr())
}
