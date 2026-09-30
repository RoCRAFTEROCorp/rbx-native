//! The accent colour's guard: whether a colour can be the accent, and the
//! nearest one that can when it can't.
//!
//! An accent paints text-bearing and state-bearing things, so it has to
//! clear WCAG's bars wherever it lands: dark button text on it (1.4.3,
//! 4.5:1), links in it on a panel (1.4.3, 4.5:1), and focus rings in it on
//! a raised card (1.4.11, 3:1). It also shouldn't pass for a status colour,
//! or a selection would read as an error; that one is advice, not a bar.

use gpui_kit::Rgba;

use crate::theme::Palette;
use crate::tokens;

/// The built-in accents, in the order the Appearance page offers them.
pub(crate) const PRESETS: [(&str, u32); 7] = [
    ("Indigo", 0x6C7FDB),
    ("Azure", 0x4C9BE8),
    ("Cyan", 0x3FB3C9),
    ("Violet", 0x9B7BE0),
    ("Orchid", 0xC877D6),
    ("Pink", 0xE27AB0),
    ("Graphite", 0xA0A8B8),
];

/// How far a hue may sit from a status colour's before it reads as one,
/// and from a preset's before a theme's accent stands in for it.
const STATUS_HUE_DEGREES: f32 = 18.;
/// Below this HLS saturation a colour is grey enough that its hue says
/// nothing.
const STATUS_SATURATION: f32 = 0.35;
/// The step the fix raises lightness by.
const FIX_STEP: f32 = 0.005;

pub(crate) fn rgb(hex: u32) -> Rgba {
    gpui_kit::rgb(hex)
}

/// `#RRGGBB`, upper case.
pub(crate) fn hex(color: Rgba) -> String {
    let [r, g, b] = [color.r, color.g, color.b].map(byte);
    format!("#{r:02X}{g:02X}{b:02X}")
}

/// `#RRGGBB` or `RRGGBB`, any case.
pub(crate) fn parse_hex(text: &str) -> Option<Rgba> {
    let digits = text.trim().trim_start_matches('#');
    if digits.len() != 6 {
        return None;
    }
    u32::from_str_radix(digits, 16).ok().map(rgb)
}

fn byte(channel: f32) -> u8 {
    (channel.clamp(0., 1.) * 255.).round() as u8
}

/// WCAG relative luminance.
fn luminance(color: Rgba) -> f32 {
    let linear = |c: f32| {
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(color.r) + 0.7152 * linear(color.g) + 0.0722 * linear(color.b)
}

pub(crate) fn contrast(a: Rgba, b: Rgba) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// One of the three bars, as measured for a colour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Check {
    pub(crate) label: &'static str,
    pub(crate) ratio: f32,
    pub(crate) min: f32,
}

impl Check {
    pub(crate) fn passes(self) -> bool {
        self.ratio >= self.min
    }
}

/// Button text on the accent, links in it on a panel, focus rings in it on
/// a raised card, on the active theme.
pub(crate) fn checks(accent: Rgba) -> [Check; 3] {
    checks_on(accent, &crate::theme::active())
}

/// [`checks`] on `theme`, which need not be the active one.
fn checks_on(accent: Rgba, theme: &Palette) -> [Check; 3] {
    [
        Check {
            label: "Button text",
            ratio: contrast(theme.color("black"), accent),
            min: 4.5,
        },
        Check {
            label: "Links",
            ratio: contrast(accent, theme.color("dock")),
            min: 4.5,
        },
        Check {
            label: "Focus rings",
            ratio: contrast(accent, theme.color("field_select")),
            min: 3.0,
        },
    ]
}

/// The presets as `theme` offers them: each at the nearest lightness that
/// clears the guard there (the guard's own fix, so on Default they are the
/// values above untouched), and the one whose hue sits within 18° of the
/// theme's own accent replaced by that accent, so a theme's accent reads
/// as a preset rather than as Custom.
pub(crate) fn presets(theme: &Palette) -> [(&'static str, Rgba); 7] {
    let passes = |color: Rgba| checks_on(color, theme).iter().all(|c| c.passes());
    let mut presets = PRESETS.map(|(name, value)| {
        let color = rgb(value);
        (name, lighten_until(color, passes).unwrap_or(color))
    });
    let own = theme.color("check_on");
    let saturated = |color: Rgba| hls(color).2 > STATUS_SATURATION;
    if saturated(own) && passes(own) {
        let apart = |color: Rgba| hue_apart(color, own);
        if let Some(slot) = presets
            .iter_mut()
            .filter(|(_, color)| saturated(*color))
            .min_by(|a, b| apart(a.1).total_cmp(&apart(b.1)))
            .filter(|(_, color)| apart(*color) <= STATUS_HUE_DEGREES)
        {
            slot.1 = own;
        }
    }
    presets
}

/// Degrees between two colours' hues, the short way round.
fn hue_apart(a: Rgba, b: Rgba) -> f32 {
    let apart = (hls(a).0 - hls(b).0).abs() * 360.;
    apart.min(360. - apart)
}

pub(crate) fn passes(accent: Rgba) -> bool {
    checks(accent).iter().all(|check| check.passes())
}

/// The selection wash's opacity for `accent`: the strongest, up to
/// `theme`'s own, that still leaves a selected row's label at 4.5:1 on the
/// dock. A lighter accent needs a thinner wash; legible text wins over the
/// wash's own 3:1, which every preset clears anyway. Measured on `theme`
/// rather than the active palette, which is still the old one while a
/// switch lays the user's accent over the new one.
pub(crate) fn selection_alpha(accent: Rgba, theme: &Palette) -> f32 {
    let (max, dock, text) = (
        theme.color("selection").a,
        theme.color("dock"),
        theme.color("text_full"),
    );
    let over_dock = |alpha: f32| {
        let mix = |a: f32, b: f32| a * alpha + b * (1. - alpha);
        Rgba {
            r: mix(accent.r, dock.r),
            g: mix(accent.g, dock.g),
            b: mix(accent.b, dock.b),
            a: 1.,
        }
    };
    let steps = (max * 255.).round() as u32;
    (0..=steps)
        .rev()
        .map(|step| step as f32 / 255.)
        .find(|alpha| contrast(text, over_dock(*alpha)) >= 4.5)
        .unwrap_or(0.)
}

/// A status colour an accent could be mistaken for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Status {
    Error,
    Warning,
    Success,
}

impl Status {
    /// As the warning names it: "Close to the error red".
    pub(crate) fn name(self) -> &'static str {
        match self {
            Status::Error => "error red",
            Status::Warning => "warning amber",
            Status::Success => "success green",
        }
    }

    fn color(self) -> Rgba {
        match self {
            Status::Error => tokens::text_error(),
            Status::Warning => tokens::warning(),
            Status::Success => tokens::diff_add(),
        }
    }
}

/// The status colour `accent`'s hue sits within 18° of, if it is saturated
/// enough for its hue to read.
pub(crate) fn near_status(accent: Rgba) -> Option<Status> {
    if hls(accent).2 <= STATUS_SATURATION {
        return None;
    }
    [Status::Error, Status::Warning, Status::Success]
        .into_iter()
        .find(|status| hue_apart(status.color(), accent) <= STATUS_HUE_DEGREES)
}

/// The colour to offer instead of one that fails a bar: the same hue and
/// saturation, lightness moved a step at a time until all three pass.
/// `None` when `accent` already passes, or no lightness would.
pub(crate) fn fix(accent: Rgba) -> Option<Rgba> {
    lighten_until(accent, passes)
}

/// `color`, lightness moved a step at a time until `ok` holds: `None` when
/// it already does, or no lightness would. Raised first, the only way that
/// helps on a dark theme; lowered when raising never passes, as on a light
/// one, where every bar is measured against a pale surface.
pub(crate) fn lighten_until(color: Rgba, ok: impl Fn(Rgba) -> bool) -> Option<Rgba> {
    if ok(color) {
        return None;
    }
    let (h, start, s) = hls(color);
    [FIX_STEP, -FIX_STEP].into_iter().find_map(|step| {
        let mut l = start;
        loop {
            l = (l + step).clamp(0., 1.);
            let candidate = quantize(from_hls(h, l, s));
            if ok(candidate) {
                return Some(candidate);
            }
            if l == 0. || l == 1. {
                return None;
            }
        }
    })
}

/// A transform tool's one bar: 3:1 against the ribbon tile it lights up
/// (WCAG 1.4.11).
pub(crate) fn tool_check(color: Rgba) -> Check {
    Check {
        label: "On the ribbon",
        ratio: contrast(color, tokens::tile()),
        min: 3.0,
    }
}

/// HSV (each 0..1) to RGB, for the picker's square and bar.
pub(crate) fn from_hsv(h: f32, s: f32, v: f32) -> Rgba {
    let i = (h.rem_euclid(1.) * 6.).floor();
    let f = h.rem_euclid(1.) * 6. - i;
    let (p, q, t) = (v * (1. - s), v * (1. - s * f), v * (1. - s * (1. - f)));
    let (r, g, b) = match i as u8 % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    Rgba { r, g, b, a: 1. }
}

pub(crate) fn hsv(color: Rgba) -> (f32, f32, f32) {
    let (r, g, b) = (color.r, color.g, color.b);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let d = max - min;
    let h = if d == 0. {
        0.
    } else if max == r {
        ((g - b) / d).rem_euclid(6.) / 6.
    } else if max == g {
        ((b - r) / d + 2.) / 6.
    } else {
        ((r - g) / d + 4.) / 6.
    };
    (h, if max == 0. { 0. } else { d / max }, max)
}

/// Rounded to what a hex code can say, so the fix is checked as it will be
/// stored.
fn quantize(color: Rgba) -> Rgba {
    let [r, g, b] = [color.r, color.g, color.b].map(|c| f32::from(byte(c)) / 255.);
    Rgba { r, g, b, a: 1. }
}

/// Hue (0..1), lightness, saturation — Python's `colorsys.rgb_to_hls`.
pub(crate) fn hls(color: Rgba) -> (f32, f32, f32) {
    let (r, g, b) = (color.r, color.g, color.b);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.;
    if max == min {
        return (0., l, 0.);
    }
    let d = max - min;
    let s = if l <= 0.5 {
        d / (max + min)
    } else {
        d / (2. - max - min)
    };
    let (rc, gc, bc) = ((max - r) / d, (max - g) / d, (max - b) / d);
    let h = if r == max {
        bc - gc
    } else if g == max {
        2. + rc - bc
    } else {
        4. + gc - rc
    };
    ((h / 6.).rem_euclid(1.), l, s)
}

/// `colorsys.hls_to_rgb`.
pub(crate) fn from_hls(h: f32, l: f32, s: f32) -> Rgba {
    if s == 0. {
        return Rgba {
            r: l,
            g: l,
            b: l,
            a: 1.,
        };
    }
    let m2 = if l <= 0.5 {
        l * (1. + s)
    } else {
        l + s - l * s
    };
    let m1 = 2. * l - m2;
    let v = |hue: f32| {
        let hue = hue.rem_euclid(1.);
        if hue < 1. / 6. {
            m1 + (m2 - m1) * hue * 6.
        } else if hue < 0.5 {
            m2
        } else if hue < 2. / 3. {
            m1 + (m2 - m1) * (2. / 3. - hue) * 6.
        } else {
            m1
        }
    };
    Rgba {
        r: v(h + 1. / 3.),
        g: v(h),
        b: v(h - 1. / 3.),
        a: 1.,
    }
}

#[cfg(test)]
#[path = "accent/tests.rs"]
mod tests;
