//! tests against the installed system fonts. they never name a font; a test
//! whose script no installed font covers at all reports that and returns, since
//! there is nothing for fallback to find.

use crate::bidi::{Level, resolve_levels};
use crate::{
    FontContext, FontFamily, FontQuery, GenericFamily, Script, ShapeOptions, ShapedRun, TextRun,
};

const SANS: &[FontFamily<'static>] = &[FontFamily::Generic(GenericFamily::SansSerif)];
const MISSING: &[FontFamily<'static>] = &[FontFamily::Named("sp-text-no-such-family")];

fn options() -> ShapeOptions<'static> {
    ShapeOptions {
        size: 16.0,
        language: None,
        features: &[],
    }
}

fn itemize(ctx: &mut FontContext, text: &str, families: &[FontFamily<'_>]) -> Vec<TextRun> {
    let para = resolve_levels(text, None);
    let query = FontQuery {
        families,
        ..Default::default()
    };
    let mut runs = Vec::new();
    ctx.itemize(text, 0..text.len(), &para.levels, &query, &mut runs);
    runs
}

fn shape_alone(ctx: &mut FontContext, text: &str, families: &[FontFamily<'_>]) -> ShapedRun {
    let runs = itemize(ctx, text, families);
    assert_eq!(runs.len(), 1, "{text:?} itemized into {runs:?}");
    ctx.shape(text, &runs[0], &options())
}

/// false (after saying so) when no installed font covers `ch`.
fn system_covers(ctx: &mut FontContext, ch: char) -> bool {
    if ctx.any_system_font_maps(ch) {
        return true;
    }
    eprintln!("skipping: no installed font maps U+{:04X}", ch as u32);
    false
}

fn assert_runs_cover(ctx: &FontContext, text: &str, runs: &[TextRun]) {
    assert_eq!(runs.first().map(|r| r.range.start), Some(0));
    assert_eq!(runs.last().map(|r| r.range.end), Some(text.len()));
    for pair in runs.windows(2) {
        assert_eq!(pair[0].range.end, pair[1].range.start);
    }
    for run in runs {
        for ch in text[run.range.clone()].chars() {
            assert!(
                ctx.has_glyph(run.font, ch),
                "run {run:?} font lacks U+{:04X}",
                ch as u32
            );
        }
    }
}

#[test]
fn latin_one_run() {
    let mut ctx = FontContext::new();
    let text = "Hello, world";
    let runs = itemize(&mut ctx, text, SANS);
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].script, Script::LATIN);
    assert_eq!(runs[0].level, Level::ltr());
    assert_runs_cover(&ctx, text, &runs);

    let shaped = ctx.shape(text, &runs[0], &options());
    assert!(!shaped.rtl);
    assert_eq!(shaped.glyphs.len(), text.len());
    assert!(shaped.glyphs.iter().all(|g| g.id != 0));
    let clusters: Vec<usize> = shaped.glyphs.iter().map(|g| g.cluster).collect();
    assert_eq!(clusters, (0..text.len()).collect::<Vec<_>>());
    assert!(shaped.advance > 0.0);
    let sum: f32 = shaped.glyphs.iter().map(|g| g.advance).sum();
    assert!((sum - shaped.advance).abs() < 1e-3);
    assert!((shaped.advance_of(0..5) + shaped.advance_of(5..12) - shaped.advance).abs() < 1e-3);
}

#[test]
fn shaping_scales_with_size() {
    let mut ctx = FontContext::new();
    let text = "size";
    let runs = itemize(&mut ctx, text, SANS);
    let small = ctx.shape(text, &runs[0], &options());
    let large = ctx.shape(
        text,
        &runs[0],
        &ShapeOptions {
            size: 32.0,
            ..options()
        },
    );
    assert!((large.advance - 2.0 * small.advance).abs() < 1e-2);
}

#[test]
fn missing_family_falls_back() {
    let mut ctx = FontContext::new();
    let text = "fallback";
    let runs = itemize(&mut ctx, text, MISSING);
    assert_runs_cover(&ctx, text, &runs);
}

#[test]
fn arabic_rtl_with_joining() {
    let mut ctx = FontContext::new();
    if !system_covers(&mut ctx, '\u{633}') {
        return;
    }
    // seen, lam, alef, meem: "salaam".
    let text = "\u{633}\u{644}\u{627}\u{645}";
    let runs = itemize(&mut ctx, text, SANS);
    assert_eq!(runs.len(), 1, "{runs:?}");
    assert_runs_cover(&ctx, text, &runs);
    assert_eq!(runs[0].script, Script::from_bytes(*b"Arab"));
    assert!(runs[0].level.is_rtl());

    let word = ctx.shape(text, &runs[0], &options());
    assert!(word.rtl);
    // visual order: the first logical character is drawn last.
    let clusters: Vec<usize> = word.glyphs.iter().map(|g| g.cluster).collect();
    assert!(clusters.windows(2).all(|w| w[0] >= w[1]), "{clusters:?}");
    assert_eq!(clusters.last(), Some(&0));

    // seen joins to lam, so it takes its initial form, not the isolated one.
    let seen = shape_alone(&mut ctx, "\u{633}", SANS);
    let initial = word.glyphs.last().map(|g| g.id);
    assert_ne!(initial, seen.glyphs.first().map(|g| g.id));
}

#[test]
fn devanagari_reorders_i_matra() {
    let mut ctx = FontContext::new();
    if !system_covers(&mut ctx, '\u{915}') {
        return;
    }
    // ka + vowel sign i: the matra is drawn before the consonant.
    let text = "\u{915}\u{93f}";
    let runs = itemize(&mut ctx, text, SANS);
    assert_eq!(runs.len(), 1, "{runs:?}");
    assert_runs_cover(&ctx, text, &runs);
    assert_eq!(runs[0].script, Script::from_bytes(*b"Deva"));

    let ka = shape_alone(&mut ctx, "\u{915}", SANS).glyphs[0].id;
    let shaped = ctx.shape(text, &runs[0], &options());
    let position = shaped.glyphs.iter().position(|g| g.id == ka);
    assert!(
        matches!(position, Some(p) if p > 0),
        "ka at {position:?} in {:?}",
        shaped.glyphs
    );
    assert!(shaped.glyphs.iter().all(|g| g.cluster == 0));
}

#[test]
fn cjk_falls_back_to_some_system_font() {
    let mut ctx = FontContext::new();
    if !system_covers(&mut ctx, '\u{6f22}') {
        return;
    }
    let text = "\u{6f22}\u{5b57}";
    let runs = itemize(&mut ctx, text, MISSING);
    assert_runs_cover(&ctx, text, &runs);
    assert_eq!(runs[0].script, Script::from_bytes(*b"Hani"));
    let shaped = ctx.shape(text, &runs[0], &options());
    assert!(shaped.glyphs.iter().all(|g| g.id != 0));
}

#[test]
fn emoji_falls_back() {
    let mut ctx = FontContext::new();
    if !system_covers(&mut ctx, '\u{1f600}') {
        return;
    }
    let text = "a\u{1f600}b";
    let runs = itemize(&mut ctx, text, SANS);
    assert_runs_cover(&ctx, text, &runs);
    let emoji = runs
        .iter()
        .find(|r| r.range.contains(&1))
        .expect("a run holds the emoji");
    assert!(ctx.has_glyph(emoji.font, '\u{1f600}'));
}

#[test]
fn runs_split_at_bidi_levels() {
    let mut ctx = FontContext::new();
    if !system_covers(&mut ctx, '\u{5d0}') {
        return;
    }
    let text = "abc \u{5d0}\u{5d1}\u{5d2} def";
    let para = resolve_levels(text, Some(Level::ltr()));
    let query = FontQuery {
        families: SANS,
        ..Default::default()
    };
    let mut runs = Vec::new();
    ctx.itemize(text, 0..text.len(), &para.levels, &query, &mut runs);
    assert_runs_cover(&ctx, text, &runs);
    let hebrew: Vec<&TextRun> = runs.iter().filter(|r| r.level.is_rtl()).collect();
    assert_eq!(hebrew.len(), 1, "{runs:?}");
    assert_eq!(&text[hebrew[0].range.clone()], "\u{5d0}\u{5d1}\u{5d2}");
    assert_eq!(hebrew[0].script, Script::from_bytes(*b"Hebr"));
}

#[test]
fn itemize_a_sub_range() {
    let mut ctx = FontContext::new();
    let text = "one two three";
    let para = resolve_levels(text, Some(Level::ltr()));
    let query = FontQuery {
        families: SANS,
        ..Default::default()
    };
    let mut runs = Vec::new();
    ctx.itemize(text, 4..7, &para.levels, &query, &mut runs);
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].range, 4..7);
    let shaped = ctx.shape(text, &runs[0], &options());
    let clusters: Vec<usize> = shaped.glyphs.iter().map(|g| g.cluster).collect();
    assert_eq!(clusters, [4, 5, 6]);
}

#[test]
fn controls_and_joiners_stay_in_the_run() {
    let mut ctx = FontContext::new();
    let text = "a\tb\u{200d}c\n";
    let runs = itemize(&mut ctx, text, SANS);
    assert_eq!(runs.len(), 1, "{runs:?}");
    assert_eq!(runs[0].range, 0..text.len());
}

#[test]
fn metrics_and_font_data() {
    let mut ctx = FontContext::new();
    let query = FontQuery {
        families: SANS,
        ..Default::default()
    };
    let font = ctx.first_available_font(&query).expect("some font loads");
    let metrics = ctx.metrics(font, 16.0).expect("metrics for a loaded font");
    assert!(metrics.units_per_em > 0);
    assert!(metrics.ascent > 0.0 && metrics.ascent < 64.0);
    assert!(metrics.descent >= 0.0 && metrics.descent < 64.0);
    assert!(metrics.line_gap >= 0.0);
    let doubled = ctx.metrics(font, 32.0).expect("metrics");
    assert!((doubled.ascent - 2.0 * metrics.ascent).abs() < 1e-3);

    let data = ctx.font_data(font).expect("data for a loaded font");
    assert!(data.blob.len() > 12);
    assert_eq!(ctx.first_available_font(&query), Some(font));
}

#[test]
fn bold_request_gets_a_distinct_instance() {
    let mut ctx = FontContext::new();
    let regular = ctx
        .first_available_font(&FontQuery {
            families: SANS,
            ..Default::default()
        })
        .expect("a font");
    let bold = ctx
        .first_available_font(&FontQuery {
            families: SANS,
            weight: 700.0,
            ..Default::default()
        })
        .expect("a font");
    // either another face, a variable instance or synthetic bold.
    let data = ctx.font_data(bold).expect("data");
    assert!(
        bold != regular || data.embolden || data.variations.iter().any(|(tag, _)| tag == b"wght")
    );
}

#[test]
fn unknown_font_id_is_harmless() {
    let mut ctx = FontContext::new();
    let bogus = crate::FontId::from_u32(u32::MAX);
    assert!(ctx.font_data(bogus).is_none());
    assert!(ctx.metrics(bogus, 16.0).is_none());
    assert!(!ctx.has_glyph(bogus, 'a'));
    let run = TextRun {
        range: 0..1,
        font: bogus,
        script: Script::LATIN,
        level: Level::ltr(),
    };
    assert!(ctx.shape("a", &run, &options()).glyphs.is_empty());
}
